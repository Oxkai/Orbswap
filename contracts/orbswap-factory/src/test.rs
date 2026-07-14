//! Factory tests — deploys real pools from the imported pool wasm.
//!
//! Requires the pool wasm to be built first:
//!   cargo build -p orbswap-pool --target wasm32v1-none --release

use crate::{FactoryError, OrbswapFactory, OrbswapFactoryClient};
use orbswap_pool_interface::{PoolMode, TWO_PLUS_SQRT2};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};

// Import the compiled pool contract (wasm bytes + a typed Client).
mod pool_wasm {
    // Import the OPTIMIZED wasm (what deploys on-chain) — the raw build exceeds
    // Soroban's 128 KB contract-code limit. Run `stellar contract optimize` on the
    // pool wasm before these tests (see docs/AGENT_PLAYBOOK.md).
    soroban_sdk::contractimport!(
        file = "../target/wasm32v1-none/release/orbswap_pool.optimized.wasm"
    );
}

struct Setup {
    env: Env,
    factory: OrbswapFactoryClient<'static>,
    admin: Address,
    token_a: Address,
    token_b: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    // Uploading the pool wasm + nested init calls exceed the default metered
    // budget; tests run with the budget lifted.
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);

    // Two mock SEP-41 tokens.
    let token_a = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let token_b = env
        .register_stellar_asset_contract_v2(admin.clone())
        .address();

    // Factory + uploaded pool wasm hash.
    let factory_id = env.register(OrbswapFactory, ());
    let factory = OrbswapFactoryClient::new(&env, &factory_id);
    let hash = env.deployer().upload_contract_wasm(pool_wasm::WASM);
    factory.initialize(&admin, &hash);

    Setup {
        env,
        factory,
        admin,
        token_a,
        token_b,
    }
}

fn tokens(s: &Setup) -> Vec<Address> {
    Vec::from_array(&s.env, [s.token_a.clone(), s.token_b.clone()])
}

#[test]
fn create_pool_deploys_working_pool() {
    let s = setup();
    let t = tokens(&s);
    let pool = s.factory.create_pool(
        &t,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &s.admin,
    );

    assert_eq!(s.factory.pool_count(), 1);
    assert_eq!(s.factory.all_pools().len(), 1);
    assert_eq!(
        s.factory.get_pool(
            &t,
            &PoolMode::Circular,
            &TWO_PLUS_SQRT2,
            &TWO_PLUS_SQRT2,
            &30
        ),
        Some(pool.clone())
    );

    // The deployed pool is real and initialized.
    let pc = pool_wasm::Client::new(&s.env, &pool);
    let cfg = pc.get_config();
    assert_eq!(cfg.fee_bps, 30);
    assert_eq!(cfg.tokens.len(), 2);
}

#[test]
fn duplicate_pool_rejected() {
    let s = setup();
    let t = tokens(&s);
    s.factory.create_pool(
        &t,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &s.admin,
    );
    // Same identity → DuplicatePool.
    let r = s.factory.try_create_pool(
        &t,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &s.admin,
    );
    assert_eq!(r, Err(Ok(FactoryError::DuplicatePool)));
}

#[test]
fn different_fee_is_a_different_pool() {
    let s = setup();
    let t = tokens(&s);
    let p30 = s.factory.create_pool(
        &t,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &s.admin,
    );
    let p50 = s.factory.create_pool(
        &t,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &50,
        &s.admin,
    );
    assert_ne!(p30, p50, "distinct fee ⇒ distinct pool");
    assert_eq!(s.factory.pool_count(), 2);
}

#[test]
fn token_order_is_canonical() {
    let s = setup();
    let ab = Vec::from_array(&s.env, [s.token_a.clone(), s.token_b.clone()]);
    let ba = Vec::from_array(&s.env, [s.token_b.clone(), s.token_a.clone()]);

    let pool = s.factory.create_pool(
        &ab,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &s.admin,
    );
    // Looking up with the reversed order finds the same pool.
    assert_eq!(
        s.factory.get_pool(
            &ba,
            &PoolMode::Circular,
            &TWO_PLUS_SQRT2,
            &TWO_PLUS_SQRT2,
            &30
        ),
        Some(pool)
    );
    // Creating with the reversed order is a duplicate.
    let r = s.factory.try_create_pool(
        &ba,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &s.admin,
    );
    assert_eq!(r, Err(Ok(FactoryError::DuplicatePool)));
    assert_eq!(s.factory.pool_count(), 1);
}

#[test]
fn get_pool_none_when_absent() {
    let s = setup();
    let t = tokens(&s);
    assert_eq!(
        s.factory.get_pool(
            &t,
            &PoolMode::Circular,
            &TWO_PLUS_SQRT2,
            &TWO_PLUS_SQRT2,
            &30
        ),
        None
    );
}
