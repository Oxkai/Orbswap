//! Reentrancy probe. A malicious token, when the pool calls its `transfer`,
//! attempts to re-enter the pool. Two layers of safety are demonstrated: (1)
//! Soroban forbids reentrancy at the platform level — a contract already on the
//! call stack cannot be called again, so the attempt traps and reverts the entire
//! transaction (no partial/stale state is observable); and (2) belt-and-suspenders,
//! the pool writes all state before any external token call anyway. The probe arms
//! a reentrant call and asserts the whole swap reverts.

use crate::types::TWO_PLUS_SQRT2;
use crate::PoolMode;
use crate::{OrbswapPool, OrbswapPoolClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Symbol, Vec};

// -------- a minimal SEP-41-ish token that spies on the pool during transfer ------

#[contract]
pub struct EvilToken;

const POOL: Symbol = symbol_short!("pool");
const SEEN: Symbol = symbol_short!("seen");

#[contractimpl]
impl EvilToken {
    pub fn decimals(_env: Env) -> u32 {
        7
    }
    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().persistent().get(&id).unwrap_or(0)
    }
    pub fn mint(env: Env, to: Address, amount: i128) {
        let b = Self::balance(env.clone(), to.clone());
        env.storage().persistent().set(&to, &(b + amount));
    }
    /// Arm the callback to re-enter `pool` on the next transfer.
    pub fn arm(env: Env, pool: Address) {
        env.storage().instance().set(&POOL, &pool);
    }
    /// Reserves observed inside the transfer callback.
    pub fn seen(env: Env) -> Vec<i128> {
        env.storage()
            .instance()
            .get(&SEEN)
            .unwrap_or_else(|| Vec::new(&env))
    }
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        let bf = Self::balance(env.clone(), from.clone());
        env.storage().persistent().set(&from, &(bf - amount));
        let bt = Self::balance(env.clone(), to.clone());
        env.storage().persistent().set(&to, &(bt + amount));
        // Re-enter the pool mid-transfer and snapshot its reserves.
        if let Some(pool) = env.storage().instance().get::<Symbol, Address>(&POOL) {
            let reserves: Vec<i128> =
                env.invoke_contract(&pool, &Symbol::new(&env, "get_reserves"), Vec::new(&env));
            env.storage().instance().set(&SEEN, &reserves);
        }
    }
}

#[test]
fn state_is_written_before_external_transfer() {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();
    env.cost_estimate().budget().reset_unlimited();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);

    // token A = evil (spy), token B = normal SAC.
    let evil_id = env.register(EvilToken, ());
    let evil = EvilTokenClient::new(&env, &evil_id);
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let sac_id = sac.address();
    let sac_admin = token::StellarAssetClient::new(&env, &sac_id);

    evil.mint(&lp, &2_000_000_000);
    sac_admin.mint(&lp, &2_000_000_000);

    let pool_id = env.register(OrbswapPool, ());
    let pool = OrbswapPoolClient::new(&env, &pool_id);
    let tokens = Vec::from_array(&env, [evil_id.clone(), sac_id.clone()]);
    pool.initialize(
        &tokens,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &0,
        &admin,
    );
    let dep = Vec::from_array(&env, [1_000_000_000i128, 1_000_000_000i128]);
    pool.deposit(&lp, &dep, &0, &u64::MAX);

    // Baseline: the evil token behaves as a normal token while unarmed.
    let out = pool.swap(&lp, &evil_id, &100_000_000, &sac_id, &0, &u64::MAX);
    assert!(out > 0, "unarmed swap should work");
    let reserves_before = pool.get_reserves();

    // Arm the reentrant call. Now the swap's transfer re-enters the pool, which
    // Soroban rejects → the whole swap traps and reverts.
    evil.arm(&pool_id);
    let r = pool.try_swap(&lp, &evil_id, &50_000_000, &sac_id, &0, &u64::MAX);
    assert!(r.is_err(), "reentrant swap must be rejected by the host");

    // Reverted cleanly: reserves unchanged from before the armed attempt.
    assert_eq!(
        pool.get_reserves(),
        reserves_before,
        "failed reentrant swap must not mutate state"
    );
}
