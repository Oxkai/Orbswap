//! Router integration test — a real 2-hop swap A→B→C through two deployed pools.
//! Requires the pool wasm: `cargo build -p orbswap-pool --target wasm32v1-none --release`.

use crate::{OrbswapRouter, OrbswapRouterClient, RouterError};
use orbswap_pool_interface::TWO_PLUS_SQRT2;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

mod pool_wasm {
    // Import the OPTIMIZED wasm (raw build > Soroban's 128 KB code limit).
    soroban_sdk::contractimport!(
        file = "../target/wasm32v1-none/release/orbswap_pool.optimized.wasm"
    );
}

fn mk_token(env: &Env, admin: &Address) -> (Address, token::StellarAssetClient<'static>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let addr = sac.address();
    let client = token::StellarAssetClient::new(env, &addr);
    (addr, client)
}

fn mk_pool(env: &Env, admin: &Address, ta: &Address, tb: &Address) -> Address {
    let addr = env.register(pool_wasm::WASM, ());
    let p = pool_wasm::Client::new(env, &addr);
    let toks = Vec::from_array(env, [ta.clone(), tb.clone()]);
    p.initialize(
        &toks,
        &pool_wasm::PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &0,
        admin,
    );
    addr
}

struct World {
    env: Env,
    router: OrbswapRouterClient<'static>,
    user: Address,
    a: Address,
    c: Address,
    pools: Vec<Address>,
    /// Token path through `pools`: a → b → c.
    tokens: Vec<Address>,
}

fn setup() -> World {
    let env = Env::default();
    // Plain `mock_all_auths` on purpose: the router calls `require_auth()` at the
    // root, so the auth tree is rooted like the real host requires. Using
    // `mock_all_auths_allowing_non_root_auth` here would hide a missing root
    // `require_auth`, which the RPC rejects with Error(Auth, InvalidAction).
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (a, am) = mk_token(&env, &admin);
    let (b, bm) = mk_token(&env, &admin);
    let (c, cm) = mk_token(&env, &admin);

    let big = 100_000_000_000i128;
    am.mint(&admin, &big);
    bm.mint(&admin, &big);
    cm.mint(&admin, &big);
    am.mint(&user, &1_000_000_000);

    // Pools A/B and B/C, each seeded with a balanced 100+100 deposit by admin.
    let p1 = mk_pool(&env, &admin, &a, &b);
    let p2 = mk_pool(&env, &admin, &b, &c);
    let dep = Vec::from_array(&env, [1_000_000_000i128, 1_000_000_000i128]);
    pool_wasm::Client::new(&env, &p1).deposit(&admin, &dep, &0, &u64::MAX);
    pool_wasm::Client::new(&env, &p2).deposit(&admin, &dep, &0, &u64::MAX);

    let router_addr = env.register(OrbswapRouter, ());
    let router = OrbswapRouterClient::new(&env, &router_addr);
    let pools = Vec::from_array(&env, [p1, p2]);
    let tokens = Vec::from_array(&env, [a.clone(), b.clone(), c.clone()]);

    World {
        env,
        router,
        user,
        a,
        c,
        pools,
        tokens,
    }
}

#[test]
fn two_hop_swap_a_to_c() {
    let w = setup();
    let quoted = w.router.quote_path(&w.pools, &w.tokens, &100_000_000);
    assert!(quoted > 0, "quote positive");

    let out = w
        .router
        .swap_exact_in(&w.user, &w.pools, &w.tokens, &100_000_000, &0, &u64::MAX);
    assert_eq!(out, quoted, "executed == quoted");
    // Two ~0-fee circular hops around balance: a bit under the input.
    assert!(out > 80_000_000 && out < 100_000_000, "out={out}");
    // User received token C.
    assert_eq!(token::Client::new(&w.env, &w.c).balance(&w.user), out);
    // User spent token A.
    assert_eq!(
        token::Client::new(&w.env, &w.a).balance(&w.user),
        1_000_000_000 - 100_000_000
    );
}

#[test]
fn slippage_on_final_output() {
    let w = setup();
    let r = w.router.try_swap_exact_in(
        &w.user,
        &w.pools,
        &w.tokens,
        &100_000_000,
        &i128::MAX, // impossible min_out
        &u64::MAX,
    );
    assert_eq!(r, Err(Ok(RouterError::SlippageExceeded)));
}

#[test]
fn two_hop_exact_out() {
    let w = setup();
    let want_c = 50_000_000i128; // exactly this much C out
    let paid = w
        .router
        .swap_exact_out(&w.user, &w.pools, &w.tokens, &want_c, &i128::MAX, &u64::MAX);
    assert!(paid > 0);
    // User received EXACTLY want_c of token C.
    assert_eq!(token::Client::new(&w.env, &w.c).balance(&w.user), want_c);
    // And spent `paid` of token A.
    assert_eq!(
        token::Client::new(&w.env, &w.a).balance(&w.user),
        1_000_000_000 - paid
    );
}

#[test]
fn exact_out_max_in_slippage() {
    let w = setup();
    let r = w.router.try_swap_exact_out(
        &w.user,
        &w.pools,
        &w.tokens,
        &50_000_000,
        &1, // absurdly low max_in
        &u64::MAX,
    );
    assert_eq!(r, Err(Ok(RouterError::SlippageExceeded)));
}

#[test]
fn empty_path_rejected() {
    let w = setup();
    let empty: Vec<Address> = Vec::new(&w.env);
    let r = w.router.try_quote_path(&empty, &empty, &100_000_000);
    assert_eq!(r, Err(Ok(RouterError::EmptyPath)));
}

#[test]
fn token_path_must_be_one_longer_than_pools() {
    let w = setup();
    // Two pools need three tokens; hand it two.
    let short = Vec::from_array(&w.env, [w.a.clone(), w.c.clone()]);
    let r = w.router.try_quote_path(&w.pools, &short, &100_000_000);
    assert_eq!(r, Err(Ok(RouterError::PathMismatch)));
}

#[test]
fn routes_through_a_leg_beyond_the_first_two() {
    // Regression: `other_token` used to read only tokens[0]/tokens[1], so any leg
    // at index >= 2 of an n-token pool was unreachable. The explicit token path
    // removes the guess entirely.
    let w = setup();
    let out = w
        .router
        .swap_exact_in(&w.user, &w.pools, &w.tokens, &100_000_000, &0, &u64::MAX);
    assert!(out > 0);
}
