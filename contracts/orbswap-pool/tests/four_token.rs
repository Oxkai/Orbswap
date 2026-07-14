//! End-to-end "dry run" of a **deep** 4-token pool (~20M total liquidity: 5M of
//! each token) with small 100–150 token swaps — the realistic stablecoin case
//! where slippage is tiny and the 0.3% fee is the dominant cost. Multiple LPs add
//! liquidity, a trader swaps across pairs, the protocol fee accrues + is collected,
//! shares are transferred, and LPs withdraw — with a full solvency check
//! (`balance == reserve + protocol_owed + lp_fees_owed`) after every action. Fees
//! are held OUTSIDE the curve, so swaps price exactly per the paper's invariant
//! (e.g. 150 in ⇒ 149.5463 out at the 0.3% fee, not a fee-inflated 149.8463).

use orbswap_pool::types::TWO_PLUS_SQRT2;
use orbswap_pool::{OrbswapPool, OrbswapPoolClient, PoolMode};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

// 7-decimal tokens: 1 display token = 1e7 native stroops.
const M: i128 = 10_000_000; // 1 token in native units
const FIVE_M: i128 = 5_000_000 * M; // 5,000,000 tokens (per-token seed) → 20M TVL

struct World {
    env: Env,
    pool: OrbswapPoolClient<'static>,
    lp1: Address,
    lp2: Address,
    lp3: Address,
    trader: Address,
    t: Vec<Address>,
}

fn mint(w: &World, i: u32, to: &Address, amt: i128) {
    token::StellarAssetClient::new(&w.env, &w.t.get_unchecked(i)).mint(to, &amt);
}
fn bal(w: &World, i: u32, who: &Address) -> i128 {
    token::Client::new(&w.env, &w.t.get_unchecked(i)).balance(who)
}
/// Native (7-decimal) amount → display tokens (host-only; for the console log).
fn tok(x: i128) -> f64 {
    x as f64 / 1e7
}

fn log_reserves(w: &World, label: &str) {
    let r = w.pool.get_reserves();
    std::println!(
        "    {label}: reserves = [{:.2}, {:.2}, {:.2}, {:.2}]  (TVL {:.2})",
        tok(r.get_unchecked(0)),
        tok(r.get_unchecked(1)),
        tok(r.get_unchecked(2)),
        tok(r.get_unchecked(3)),
        tok(r.get_unchecked(0) + r.get_unchecked(1) + r.get_unchecked(2) + r.get_unchecked(3)),
    );
}

/// Solvency: on-chain balance == stored reserve + protocol owed + LP fees owed.
fn assert_solvent(w: &World, step: &str) {
    let reserves = w.pool.get_reserves();
    let owed = w.pool.protocol_owed();
    let lp_owed = w.pool.lp_fees_owed();
    for i in 0..4u32 {
        let onchain = bal(w, i, &w.pool.address);
        let expected = reserves.get_unchecked(i) + owed.get_unchecked(i) + lp_owed.get_unchecked(i);
        assert_eq!(
            onchain, expected,
            "[{step}] token {i}: balance {onchain} != reserve+protocol+lp {expected}"
        );
    }
}

fn setup() -> World {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let lp1 = Address::generate(&env);
    let lp2 = Address::generate(&env);
    let lp3 = Address::generate(&env);
    let trader = Address::generate(&env);

    let mut t = Vec::new(&env);
    for _ in 0..4 {
        t.push_back(
            env.register_stellar_asset_contract_v2(admin.clone())
                .address(),
        );
    }

    let pool_id = env.register(OrbswapPool, ());
    let pool = OrbswapPoolClient::new(&env, &pool_id);
    // 4-token symmetric SuperElliptical (the 4-sphere, u=2), 30 bps fee.
    pool.initialize(
        &t,
        &PoolMode::SuperElliptical,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &admin,
    );
    let _ = admin;

    let w = World {
        env,
        pool,
        lp1,
        lp2,
        lp3,
        trader,
        t,
    };
    // Fund each player with 30M of every token.
    for i in 0..4u32 {
        mint(&w, i, &w.lp1, 30_000_000 * M);
        mint(&w, i, &w.lp2, 30_000_000 * M);
        mint(&w, i, &w.trader, 30_000_000 * M);
    }
    w
}

#[test]
fn four_token_dry_run() {
    let w = setup();
    let deadline = u64::MAX;
    std::println!("\n=== ORBSWAP 4-TOKEN DRY RUN — deep pool, 30 bps fee ===");
    std::println!("(1 token = 1e7 native stroops; target ~20M total liquidity)\n");

    // ---- 1. LP1 seeds a deep pool: 5,000,000 of each (20M TVL) ------------------
    let seed = Vec::from_array(&w.env, [FIVE_M; 4]);
    let sh1 = w.pool.deposit(&w.lp1, &seed, &0, &deadline);
    std::println!("1) LP1 adds liquidity 5,000,000 of each token (20M TVL) → {sh1} shares");
    assert!(sh1 > 0);
    let r = w.pool.get_reserves();
    for i in 0..4u32 {
        assert_eq!(r.get_unchecked(i), FIVE_M);
    }
    log_reserves(&w, "pool");
    assert_solvent(&w, "after LP1 seed");

    // ---- 2. LP2 adds proportional liquidity (1,000,000 of each) -----------------
    let add = Vec::from_array(&w.env, [1_000_000 * M; 4]);
    let sh2 = w.pool.deposit(&w.lp2, &add, &0, &deadline);
    std::println!("\n2) LP2 adds liquidity 1,000,000 of each (proportional) → {sh2} shares");
    // 1M added onto 5M ⇒ ~1/5 of LP1's stake.
    assert!(
        sh2 * 4 < sh1 && sh2 * 6 > sh1,
        "LP2 shares off: {sh2} vs {sh1}"
    );
    assert_eq!(w.pool.get_reserves().get_unchecked(0), 6_000_000 * M);
    log_reserves(&w, "pool");
    assert_solvent(&w, "after LP2 add");

    // ---- 3. Trader makes small swaps (100–150 tokens) around the deep pool ------
    let swap = |from_i: u32, to_i: u32, amt: i128| -> i128 {
        w.pool.swap(
            &w.trader,
            &w.t.get_unchecked(from_i),
            &amt,
            &w.t.get_unchecked(to_i),
            &0,
            &deadline,
        )
    };
    std::println!("\n3) Trader makes small swaps (deep pool ⇒ ~0 slippage, 0.3% fee dominates):");
    let out02 = swap(0, 2, 100 * M); // 100 T0 -> T2
    std::println!(
        "   100 T0 -> T2 = {:.4} T2   (implied rate {:.5})",
        tok(out02),
        tok(out02) / 100.0
    );
    assert!(out02 > 0);
    assert_solvent(&w, "after swap 0->2");

    let out13 = swap(1, 3, 150 * M); // 150 T1 -> T3
    std::println!(
        "   150 T1 -> T3 = {:.4} T3   (implied rate {:.5})",
        tok(out13),
        tok(out13) / 150.0
    );
    assert!(out13 > 0);
    assert_solvent(&w, "after swap 1->3");

    let out30 = swap(3, 0, 100 * M); // 100 T3 -> T0
    std::println!(
        "   100 T3 -> T0 = {:.4} T0   (implied rate {:.5})",
        tok(out30),
        tok(out30) / 100.0
    );
    assert!(out30 > 0);
    assert_solvent(&w, "after swap 3->0");
    log_reserves(&w, "pool after swaps");

    // ---- 4. Protocol fee on, swap 150, collect ---------------------------------
    std::println!("\n4) Protocol fee = 50% of the 30 bps fee; trader swaps 150 T2 -> T1:");
    w.pool.set_protocol_fee_bps(&5_000);
    let out21 = swap(2, 1, 150 * M);
    let owed = w.pool.protocol_owed();
    std::println!(
        "   out = {:.4} T1;  protocol owed {:.4} T2 (50% of the 0.45 T2 fee on 150)",
        tok(out21),
        tok(owed.get_unchecked(2))
    );
    assert!(owed.get_unchecked(2) > 0);
    assert_solvent(&w, "after fee swap");
    let treasury = Address::generate(&w.env);
    let collected = w.pool.collect_protocol_fees(&treasury);
    std::println!(
        "   collect_protocol_fees → treasury got {:.4} T2",
        tok(collected.get_unchecked(2))
    );
    assert_eq!(collected.get_unchecked(2), owed.get_unchecked(2));
    for i in 0..4u32 {
        assert_eq!(w.pool.protocol_owed().get_unchecked(i), 0);
    }
    assert_solvent(&w, "after protocol collect");

    // ---- 5. LP1 transfers shares to LP3, who withdraws --------------------------
    let gift = sh1 / 4;
    w.pool.transfer_shares(&w.lp1, &w.lp3, &gift);
    std::println!("\n5) LP1 transfers 1/4 of its shares to LP3, who withdraws:");
    let mins = Vec::from_array(&w.env, [0i128; 4]);
    let got = w.pool.withdraw(&w.lp3, &gift, &mins, &deadline);
    std::println!(
        "   LP3 received [{:.2}, {:.2}, {:.2}, {:.2}]",
        tok(got.get_unchecked(0)),
        tok(got.get_unchecked(1)),
        tok(got.get_unchecked(2)),
        tok(got.get_unchecked(3))
    );
    for i in 0..4u32 {
        assert!(got.get_unchecked(i) > 0);
    }
    assert_eq!(w.pool.shares_of(&w.lp3), 0);
    assert_solvent(&w, "after LP3 withdraw");

    // ---- 6. Round trip can't profit --------------------------------------------
    let dx = 100 * M;
    let y = swap(0, 1, dx);
    let back = swap(1, 0, y);
    std::println!(
        "\n6) Round trip: 100 T0 -> {:.4} T1 -> {:.4} T0 back  (≤ 100 ✓ no free money)",
        tok(y),
        tok(back)
    );
    assert!(back <= dx, "round trip profited: dx={dx} back={back}");
    assert_solvent(&w, "after round trip");

    // ---- 7. Everyone exits; the locked MINIMUM_LIQUIDITY remains ----------------
    w.pool
        .withdraw(&w.lp1, &w.pool.shares_of(&w.lp1), &mins, &deadline);
    w.pool
        .withdraw(&w.lp2, &w.pool.shares_of(&w.lp2), &mins, &deadline);
    std::println!("\n7) LP1 & LP2 exit fully; only the locked MINIMUM_LIQUIDITY dust remains:");
    log_reserves(&w, "pool");
    assert!(w.pool.total_shares() >= orbswap_pool::types::MINIMUM_LIQUIDITY);
    assert!(w.pool.get_liquidity_scale() > 0);
    assert_solvent(&w, "after full exit");
    std::println!(
        "\n✅ Every action solvent (balance == reserve + protocol_owed + lp_fees_owed). Dry run OK.\n"
    );
}
