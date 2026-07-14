#![no_std]
//! Orbswap factory — deploys pool instances from an uploaded pool wasm hash and
//! keeps a registry keyed by `(tokens, mode, shape, fee)` so duplicates are
//! rejected and pools are enumerable.
//!
//! Tokens are **canonically ordered** (sorted by XDR bytes) before hashing and
//! deployment, so `[A,B]` and `[B,A]` resolve to the same pool.

use orbswap_pool_interface::{PoolClient, PoolMode};
use soroban_sdk::xdr::ToXdr;
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, BytesN, Env, Vec,
};

#[cfg(test)]
mod test;

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FactoryError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    DuplicatePool = 4,
    InvalidTokens = 5,
}

/// The identity of a pool — hashed to a fixed-size storage key (a full struct key
/// with the tokens `Vec` exceeds Soroban's 250-byte key limit).
#[contracttype]
#[derive(Clone)]
pub struct PoolKey {
    pub tokens: Vec<Address>,
    pub mode: PoolMode,
    pub alpha: i128,
    pub beta: i128,
    pub fee_bps: i128,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    WasmHash,
    Count,
    AllPools,
    /// `sha256(PoolKey)` → pool `Address`.
    Pool(BytesN<32>),
}

/// Canonically order a 2-token vector (sort by XDR bytes) so token order does not
/// create distinct pools.
fn canonical(env: &Env, tokens: &Vec<Address>) -> Vec<Address> {
    let a = tokens.get_unchecked(0);
    let b = tokens.get_unchecked(1);
    if a.clone().to_xdr(env) <= b.clone().to_xdr(env) {
        Vec::from_array(env, [a, b])
    } else {
        Vec::from_array(env, [b, a])
    }
}

/// Deterministic identity hash of a pool's `(tokens, mode, α, β, fee)`.
fn pool_hash(
    env: &Env,
    tokens: &Vec<Address>,
    mode: &PoolMode,
    alpha: i128,
    beta: i128,
    fee_bps: i128,
) -> BytesN<32> {
    let key = PoolKey {
        tokens: tokens.clone(),
        mode: *mode,
        alpha,
        beta,
        fee_bps,
    };
    env.crypto().sha256(&key.to_xdr(env)).to_bytes()
}

/// Emitted when a pool is created.
#[contractevent]
pub struct PoolCreated {
    pub admin: Address,
    pub pool: Address,
}

#[contract]
pub struct OrbswapFactory;

#[contractimpl]
impl OrbswapFactory {
    /// One-time setup: `admin` (can rotate the wasm hash) and the uploaded pool
    /// wasm hash used for every deployment.
    pub fn initialize(
        env: Env,
        admin: Address,
        pool_wasm_hash: BytesN<32>,
    ) -> Result<(), FactoryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(FactoryError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WasmHash, &pool_wasm_hash);
        env.storage().instance().set(&DataKey::Count, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::AllPools, &Vec::<Address>::new(&env));
        bump(&env);
        Ok(())
    }

    /// Deploy + initialize a new pool. Permissionless. Reverts on a duplicate
    /// `(tokens, mode, alpha, beta, fee_bps)`.
    pub fn create_pool(
        env: Env,
        tokens: Vec<Address>,
        mode: PoolMode,
        alpha: i128,
        beta: i128,
        fee_bps: i128,
        pool_admin: Address,
    ) -> Result<Address, FactoryError> {
        if tokens.len() != 2 {
            return Err(FactoryError::InvalidTokens);
        }
        let tokens = canonical(&env, &tokens);
        let pool_key = DataKey::Pool(pool_hash(&env, &tokens, &mode, alpha, beta, fee_bps));
        if env.storage().persistent().has(&pool_key) {
            return Err(FactoryError::DuplicatePool);
        }

        let wasm_hash: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::WasmHash)
            .ok_or(FactoryError::NotInitialized)?;
        let count: u32 = env.storage().instance().get(&DataKey::Count).unwrap_or(0);

        // Deterministic salt from the counter → distinct address per pool.
        let deployed = env
            .deployer()
            .with_current_contract(salt_from_count(&env, count))
            .deploy_v2(wasm_hash, ());

        // Initialize the freshly deployed pool.
        PoolClient::new(&env, &deployed).initialize(
            &tokens,
            &mode,
            &alpha,
            &beta,
            &fee_bps,
            &pool_admin,
        );

        // Register.
        env.storage().persistent().set(&pool_key, &deployed);
        env.storage().instance().set(&DataKey::Count, &(count + 1));
        let mut all: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AllPools)
            .unwrap_or_else(|| Vec::new(&env));
        all.push_back(deployed.clone());
        env.storage().instance().set(&DataKey::AllPools, &all);
        bump(&env);

        PoolCreated {
            admin: pool_admin,
            pool: deployed.clone(),
        }
        .publish(&env);
        Ok(deployed)
    }

    /// The pool address for a given identity, if it exists.
    pub fn get_pool(
        env: Env,
        tokens: Vec<Address>,
        mode: PoolMode,
        alpha: i128,
        beta: i128,
        fee_bps: i128,
    ) -> Option<Address> {
        let tokens = canonical(&env, &tokens);
        let key = DataKey::Pool(pool_hash(&env, &tokens, &mode, alpha, beta, fee_bps));
        env.storage().persistent().get(&key)
    }

    pub fn all_pools(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::AllPools)
            .unwrap_or_else(|| Vec::new(&env))
    }

    pub fn pool_count(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Count).unwrap_or(0)
    }

    /// Rotate the pool wasm hash (admin only) — future pools use the new code.
    pub fn set_pool_wasm_hash(env: Env, new_hash: BytesN<32>) -> Result<(), FactoryError> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(FactoryError::NotInitialized)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::WasmHash, &new_hash);
        bump(&env);
        Ok(())
    }
}

fn salt_from_count(env: &Env, count: u32) -> BytesN<32> {
    let mut bytes = [0u8; 32];
    bytes[28..32].copy_from_slice(&count.to_be_bytes());
    BytesN::from_array(env, &bytes)
}

fn bump(env: &Env) {
    env.storage().instance().extend_ttl(431_000, 518_400);
}
