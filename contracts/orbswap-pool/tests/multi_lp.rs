//! Multi-LP interleaved lifecycle simulation — the Orbswap/Soroban analog of the
//! EVM `script/SimulateMultiLP.s.sol`. Three LPs (Alice, Bob, Carol) seed a 4-token
//! SuperElliptical pool to **exactly 24M total liquidity**, then a dedicated Swapper
//! trades across pairs, fees accrue (held outside the curve), LPs top up, collect
//! (paid on withdraw here — no separate collect), and partially / fully withdraw.
//!
//! Differences from the EVM reference (by design): Orbswap is ONE n-dim pool, not
//! N-choose-2 pairwise pools, and it is a single-range MVP (no per-tick positions),
//! so all LPs share the same range instead of Alice/Bob@tickMid + Carol@tickWide.
//! LP fees are paid pro-rata on withdraw rather than via an explicit `collect`.
//!
//! Solvency (`balance == reserves + protocol_owed + lp_fees_owed`) is asserted after
//! every single action.

use orbswap_pool::types::TWO_PLUS_SQRT2;
use orbswap_pool::{OrbswapPool, OrbswapPoolClient, PoolMode};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

const M: i128 = 10_000_000; // 1 display token in native (7-dec) units
const SYMBOLS: [&str; 4] = ["sUSDA", "sUSDB", "sUSDC", "sUSDD"];

struct World {
    env: Env,
    pool: OrbswapPoolClient<'static>,
    alice: Address,
    bob: Address,
    carol: Address,
    swapper: Address,
    t: Vec<Address>,
}

fn mint(w: &World, i: u32, to: &Address, amt: i128) {
    token::StellarAssetClient::new(&w.env, &w.t.get_unchecked(i)).mint(to, &amt);
}
fn bal(w: &World, i: u32, who: &Address) -> i128 {
    token::Client::new(&w.env, &w.t.get_unchecked(i)).balance(who)
}
/// native (7-dec) → display tokens (host-only, for the console log).
fn tok(x: i128) -> f64 {
    x as f64 / 1e7
}

fn tvl(w: &World) -> i128 {
    let r = w.pool.get_reserves();
    (0..4u32).map(|i| r.get_unchecked(i)).sum()
}

fn log_engine(w: &World) {
    let r = w.pool.get_reserves();
    let lp = w.pool.lp_fees_owed();
    let po = w.pool.protocol_owed();
    std::println!(
        "  reserves = [{:.2}, {:.2}, {:.2}, {:.2}]  TVL {:.2}  shares(total) {}",
        tok(r.get_unchecked(0)),
        tok(r.get_unchecked(1)),
        tok(r.get_unchecked(2)),
        tok(r.get_unchecked(3)),
        tok(tvl(w)),
        w.pool.total_shares(),
    );
    for i in 0..4u32 {
        let (l, p) = (lp.get_unchecked(i), po.get_unchecked(i));
        if l > 0 || p > 0 {
            std::println!(
                "    {} fees: lp {:.4}  protocol {:.4}",
                SYMBOLS[i as usize],
                tok(l),
                tok(p)
            );
        }
    }
}

fn header(tag: &str) {
    std::println!("\n== {tag} ==");
}

/// Solvency: on-chain balance == stored reserve + protocol owed + LP fees owed.
fn assert_solvent(w: &World, step: &str) {
    let r = w.pool.get_reserves();
    let po = w.pool.protocol_owed();
    let lp = w.pool.lp_fees_owed();
    for i in 0..4u32 {
        let onchain = bal(w, i, &w.pool.address);
        let expected = r.get_unchecked(i) + po.get_unchecked(i) + lp.get_unchecked(i);
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
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    let carol = Address::generate(&env);
    let swapper = Address::generate(&env);

    let mut t = Vec::new(&env);
    for _ in 0..4 {
        t.push_back(
            env.register_stellar_asset_contract_v2(admin.clone())
                .address(),
        );
    }

    let pool_id = env.register(OrbswapPool, ());
    let pool = OrbswapPoolClient::new(&env, &pool_id);
    // 4-token symmetric SuperElliptical (the 4-sphere, u=2), 30 bps fee — matches
    // the live testnet pool.
    pool.initialize(
        &t,
        &PoolMode::SuperElliptical,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &admin,
    );

    let w = World {
        env,
        pool,
        alice,
        bob,
        carol,
        swapper,
        t,
    };
    // Fund each party with plenty of every token.
    for i in 0..4u32 {
        mint(&w, i, &w.alice, 5_000_000 * M);
        mint(&w, i, &w.bob, 5_000_000 * M);
        mint(&w, i, &w.carol, 5_000_000 * M);
        mint(&w, i, &w.swapper, 1_000_000 * M);
    }
    w
}

/// Balanced deposit of `each` tokens of every asset (valid only while the pool is
/// still balanced — used during the pre-swap seeding phase).
fn deposit_balanced(w: &World, lp: &Address, each: i128) -> i128 {
    let amts = Vec::from_array(&w.env, [each; 4]);
    w.pool.deposit(lp, &amts, &0, &u64::MAX)
}

/// Proportional deposit matching the CURRENT (possibly skewed) reserve ratio, sized
/// so token 0 contributes `d0`. Safe after swaps have moved the pool off balance.
fn deposit_proportional(w: &World, lp: &Address, d0: i128) -> i128 {
    let r = w.pool.get_reserves();
    let r0 = r.get_unchecked(0);
    let mut amts = Vec::new(&w.env);
    for i in 0..4u32 {
        amts.push_back(d0 * r.get_unchecked(i) / r0);
    }
    w.pool.deposit(lp, &amts, &0, &u64::MAX)
}

fn swap(w: &World, from_i: u32, to_i: u32, amt: i128) -> i128 {
    let out = w.pool.swap(
        &w.swapper,
        &w.t.get_unchecked(from_i),
        &amt,
        &w.t.get_unchecked(to_i),
        &0,
        &u64::MAX,
    );
    std::println!(
        "  swap {:.2} {} -> {:.4} {}",
        tok(amt),
        SYMBOLS[from_i as usize],
        tok(out),
        SYMBOLS[to_i as usize]
    );
    out
}

#[test]
fn multi_lp_seed_to_24m() {
    let w = setup();
    std::println!("\n=== ORBSWAP MULTI-LP SIM — 4-token SuperElliptical, 30 bps ===");

    // ─────────────────────────────────────────────────────────────
    // SEEDING — three LPs bring the pool to EXACTLY 24M total liquidity.
    // Done before any swap so the pool stays balanced and the 24M is exact.
    // ─────────────────────────────────────────────────────────────
    header("Alice seeds 3,000,000 of each (12M TVL)");
    let a_sh = deposit_balanced(&w, &w.alice, 3_000_000 * M);
    std::println!("  Alice shares: {a_sh}");
    log_engine(&w);
    assert_solvent(&w, "alice seed");

    header("Bob seeds 2,000,000 of each (+8M → 20M TVL)");
    let b_sh = deposit_balanced(&w, &w.bob, 2_000_000 * M);
    std::println!("  Bob shares:   {b_sh}");
    log_engine(&w);
    assert_solvent(&w, "bob seed");

    header("Carol seeds 1,000,000 of each (+4M → 24M TVL)");
    let c_sh = deposit_balanced(&w, &w.carol, 1_000_000 * M);
    std::println!("  Carol shares: {c_sh}");
    log_engine(&w);
    assert_solvent(&w, "carol seed");

    // The headline milestone: END OF SEEDING == 24M total liquidity, exactly.
    assert_eq!(tvl(&w), 24_000_000 * M, "end-of-seeding TVL must be 24M");
    for i in 0..4u32 {
        assert_eq!(w.pool.get_reserves().get_unchecked(i), 6_000_000 * M);
    }
    // Shares are proportional to what each LP put in (3 : 2 : 1), minus the tiny
    // one-time MINIMUM_LIQUIDITY lock on Alice's first deposit.
    assert!(a_sh > b_sh && b_sh > c_sh);
    std::println!(
        "\n>>> END OF SEEDING: TVL = {:.0} (24M) across 3 LPs <<<",
        tok(tvl(&w))
    );

    // ─────────────────────────────────────────────────────────────
    // ACTIVITY — the Swapper trades across pairs; fees accrue outside the curve.
    // ─────────────────────────────────────────────────────────────
    header("Swapper: tiny probe 0.5 sUSDA -> sUSDB");
    swap(&w, 0, 1, M / 2);
    assert_solvent(&w, "probe swap");

    header("Swapper: 5,000 sUSDB -> sUSDC");
    swap(&w, 1, 2, 5_000 * M);
    assert_solvent(&w, "medium swap");

    header("Swapper: 20,000 sUSDA -> sUSDD (large)");
    swap(&w, 0, 3, 20_000 * M);
    log_engine(&w);
    assert_solvent(&w, "large swap");

    header("Swapper: 1 sUSDD -> sUSDA (micro reverse)");
    swap(&w, 3, 0, M);
    assert_solvent(&w, "micro reverse");

    // ─────────────────────────────────────────────────────────────
    // Alice tops up (proportional to the now-skewed reserves).
    // ─────────────────────────────────────────────────────────────
    header("Alice tops up ~500,000 sUSDA-equivalent (proportional)");
    let a_top = deposit_proportional(&w, &w.alice, 500_000 * M);
    std::println!("  Alice extra shares: {a_top}");
    log_engine(&w);
    assert_solvent(&w, "alice topup");

    header("Swapper: 3,000 sUSDC -> sUSDB (reverse leg)");
    swap(&w, 2, 1, 3_000 * M);
    log_engine(&w);
    assert_solvent(&w, "reverse leg");

    // ─────────────────────────────────────────────────────────────
    // WIND-DOWN — partial and full withdrawals. LP fee cut is paid pro-rata here.
    // ─────────────────────────────────────────────────────────────
    let mins = Vec::from_array(&w.env, [0i128; 4]);

    header("Alice withdraws 50% of her shares");
    let a_half = w.pool.shares_of(&w.alice) / 2;
    let a_out = w.pool.withdraw(&w.alice, &a_half, &mins, &u64::MAX);
    std::println!(
        "  Alice got [{:.2}, {:.2}, {:.2}, {:.2}]",
        tok(a_out.get_unchecked(0)),
        tok(a_out.get_unchecked(1)),
        tok(a_out.get_unchecked(2)),
        tok(a_out.get_unchecked(3))
    );
    assert_solvent(&w, "alice 50% withdraw");

    header("Bob withdraws ALL his shares");
    let b_all = w.pool.shares_of(&w.bob);
    w.pool.withdraw(&w.bob, &b_all, &mins, &u64::MAX);
    assert_eq!(w.pool.shares_of(&w.bob), 0);
    log_engine(&w);
    assert_solvent(&w, "bob full withdraw");

    header("Carol withdraws half her shares");
    let c_half = w.pool.shares_of(&w.carol) / 2;
    w.pool.withdraw(&w.carol, &c_half, &mins, &u64::MAX);
    log_engine(&w);
    assert_solvent(&w, "carol half withdraw");

    // ─────────────────────────────────────────────────────────────
    // Final holdings.
    // ─────────────────────────────────────────────────────────────
    header("FINAL LP SHARE BALANCES");
    std::println!("  Alice: {}", w.pool.shares_of(&w.alice));
    std::println!("  Bob:   {}", w.pool.shares_of(&w.bob));
    std::println!("  Carol: {}", w.pool.shares_of(&w.carol));
    std::println!(
        "  pool still holds TVL {:.2} (S={})",
        tok(tvl(&w)),
        w.pool.get_liquidity_scale()
    );

    // Bob fully exited; Alice and Carol still in; pool still solvent and above the lock.
    assert!(w.pool.shares_of(&w.alice) > 0 && w.pool.shares_of(&w.carol) > 0);
    assert!(w.pool.total_shares() >= orbswap_pool::types::MINIMUM_LIQUIDITY);
    std::println!("\n✅ Multi-LP sim complete — seeded to 24M, solvent after every action.\n");
}
