//! N-token pool tests (3-token SuperElliptical via the `ndim` math).

use crate::types::TWO_PLUS_SQRT2;
use crate::{OrbswapPool, OrbswapPoolClient, PoolMode};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

struct Tri {
    env: Env,
    pool: OrbswapPoolClient<'static>,
    lp: Address,
    tokens: Vec<Address>,
}

fn setup3() -> Tri {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let lp = Address::generate(&env);

    let mut tokens = Vec::new(&env);
    for _ in 0..3 {
        let sac = env.register_stellar_asset_contract_v2(admin.clone());
        let addr = sac.address();
        token::StellarAssetClient::new(&env, &addr).mint(&lp, &10_000_000_000);
        tokens.push_back(addr);
    }

    let pool_id = env.register(OrbswapPool, ());
    let pool = OrbswapPoolClient::new(&env, &pool_id);
    // Symmetric 3-token pool: α=β=2+√2 (the n-sphere, u=2).
    pool.initialize(
        &tokens,
        &PoolMode::SuperElliptical,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &0,
        &admin,
    );
    Tri {
        env,
        pool,
        lp,
        tokens,
    }
}

fn bal(t: &Tri, i: u32, who: &Address) -> i128 {
    token::Client::new(&t.env, &t.tokens.get_unchecked(i)).balance(who)
}

#[test]
fn three_token_deposit_and_swap() {
    let t = setup3();
    // Balanced first deposit of 100 each.
    let amounts = Vec::from_array(&t.env, [1_000_000_000i128; 3]);
    let minted = t.pool.deposit(&t.lp, &amounts, &0, &u64::MAX);
    assert!(minted > 0);
    assert_eq!(t.pool.get_reserves().len(), 3);

    // Swap token0 → token2.
    let t0 = t.tokens.get_unchecked(0);
    let t2 = t.tokens.get_unchecked(2);
    let quoted = t.pool.quote(&t0, &100_000_000, &t2);
    assert!(quoted > 0, "3-token quote positive");

    let b2_before = bal(&t, 2, &t.lp);
    let out = t.pool.swap(&t.lp, &t0, &100_000_000, &t2, &0, &u64::MAX);
    assert_eq!(out, quoted);
    // ~10% swap near balance → a bit under 10 tokens out.
    assert!(out > 85_000_000 && out < 100_000_000, "out={out}");
    assert_eq!(bal(&t, 2, &t.lp), b2_before + out);

    // Untouched token1 reserve unchanged; solvency on all three.
    let r = t.pool.get_reserves();
    assert_eq!(r.get_unchecked(1), 1_000_000_000, "middle token untouched");
    for i in 0..3u32 {
        assert_eq!(
            token::Client::new(&t.env, &t.tokens.get_unchecked(i)).balance(&t.pool.address),
            r.get_unchecked(i),
            "solvency token {i}"
        );
    }
}

#[test]
fn three_token_roundtrip_no_profit() {
    let t = setup3();
    let amounts = Vec::from_array(&t.env, [1_000_000_000i128; 3]);
    t.pool.deposit(&t.lp, &amounts, &0, &u64::MAX);
    let t0 = t.tokens.get_unchecked(0);
    let t2 = t.tokens.get_unchecked(2);

    let dx = 50_000_000i128;
    let out2 = t.pool.swap(&t.lp, &t0, &dx, &t2, &0, &u64::MAX);
    let back0 = t.pool.swap(&t.lp, &t2, &out2, &t0, &0, &u64::MAX);
    assert!(
        back0 <= dx,
        "trader profited across 3-token pool: dx={dx} back={back0}"
    );
}

#[test]
fn n_token_exact_out_unsupported() {
    let t = setup3();
    let amounts = Vec::from_array(&t.env, [1_000_000_000i128; 3]);
    t.pool.deposit(&t.lp, &amounts, &0, &u64::MAX);
    // Exact-out has no n-dim curve inverse yet → MathDomain error.
    let r = t.pool.try_quote_exact_out(
        &t.tokens.get_unchecked(0),
        &t.tokens.get_unchecked(2),
        &10_000_000,
    );
    assert!(r.is_err(), "n-token exact-out should be unsupported");
}

#[test]
#[should_panic] // InvalidConfig: circle is 2-token only
fn circular_rejects_three_tokens() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let mut tokens = Vec::new(&env);
    for _ in 0..3 {
        tokens.push_back(
            env.register_stellar_asset_contract_v2(admin.clone())
                .address(),
        );
    }
    let pool = OrbswapPoolClient::new(&env, &env.register(OrbswapPool, ()));
    pool.initialize(
        &tokens,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &0,
        &admin,
    );
}
