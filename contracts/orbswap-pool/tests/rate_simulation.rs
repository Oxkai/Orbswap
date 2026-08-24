//! End-to-end simulation of a rate-aware pool over a realistic FX sequence.
//!
//! The unit tests prove each guard in isolation. This drives the whole system the
//! way a live deployment would: an anchor seeds inventory, a currency drifts on a
//! feed, a keeper pokes and repegs on a cadence, and traders swap in both
//! directions throughout.
//!
//! What it asserts, at **every step**:
//!
//! 1. **Solvency** — `balance == reserves + ProtocolOwed + LpFeesOwed`, exact to
//!    the integer, on both legs.
//! 2. **Never open off-curve** — the Phase 0 pot is never payable.
//! 3. **No free value** — a trader who round-trips always ends up worse off.
//! 4. **The pool survives** — it keeps quoting across the whole sequence.

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{token, Address, Env, Vec};

use orbswap_pool::{OrbswapPool, OrbswapPoolClient, PoolMode};

mod feed {
    soroban_sdk::contractimport!(
        file = "../target/wasm32v1-none/release/orbswap_feed_stub.optimized.wasm"
    );
}

const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049;
const M: i128 = 10_000_000; // 1.0 at 7 decimals
const FEED_DEC: u32 = 14;
const ONE_FEED: i128 = 100_000_000_000_000;
const FEE_BPS: i128 = 30;
const MAX_AGE: u64 = 3_600;
const MAX_DEV_BPS: i128 = 500;
const MAXU64: u64 = u64::MAX;
const WAD: i128 = 1_000_000_000_000_000_000;

/// Starting rate: 1 quote unit = 0.001 numeraire (e.g. NGNC/USDC).
const START_PRICE: i128 = ONE_FEED / 1_000;
const SEED_NUM: i128 = 10_000; // 10k numeraire
const SEED_QUOTE: i128 = 10_000_000; // equal VALUE at 0.001

struct Sim {
    env: Env,
    pool: OrbswapPoolClient<'static>,
    feed: feed::Client<'static>,
    num: Address,
    quote: Address,
    lp: Address,
    trader: Address,
    price: i128,
}

fn mint(
    env: &Env,
    admin: &Address,
    decimals: u32,
) -> (Address, token::StellarAssetClient<'static>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let addr = sac.address();
    let _ = decimals;
    (addr.clone(), token::StellarAssetClient::new(env, &addr))
}

fn setup() -> Sim {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();

    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let trader = Address::generate(&env);

    let (num, num_admin) = mint(&env, &admin, 7);
    let (quote, quote_admin) = mint(&env, &admin, 7);
    for who in [&lp, &trader] {
        num_admin.mint(who, &(SEED_NUM * 100 * M));
        quote_admin.mint(who, &(SEED_QUOTE * 100 * M));
    }

    // SEP-40 feed, denominated in the numeraire.
    let feed_id = env.register(feed::WASM, ());
    let feed = feed::Client::new(&env, &feed_id);
    feed.initialize(&admin, &FEED_DEC, &num);
    feed.set_price(&quote, &START_PRICE);
    feed.set_price(&num, &ONE_FEED);

    // Rate-aware pool: index 0 = numeraire, index 1 = quote.
    let pool_id = env.register(OrbswapPool, ());
    let pool = OrbswapPoolClient::new(&env, &pool_id);
    pool.initialize(
        &Vec::from_array(&env, [num.clone(), quote.clone()]),
        &PoolMode::SuperElliptical,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &FEE_BPS,
        &admin,
    );
    pool.configure_rates(&feed_id, &1, &0, &false, &MAX_AGE, &MAX_DEV_BPS);

    // Anchor seeds equal VALUE.
    pool.deposit(
        &lp,
        &Vec::from_array(&env, [SEED_NUM * M, SEED_QUOTE * M]),
        &0,
        &MAXU64,
    );

    Sim {
        env,
        pool,
        feed,
        num,
        quote,
        lp,
        trader,
        price: START_PRICE,
    }
}

impl Sim {
    fn bal(&self, t: &Address, who: &Address) -> i128 {
        token::Client::new(&self.env, t).balance(who)
    }

    /// `balance == reserves + ProtocolOwed + LpFeesOwed`, both legs, exact.
    fn assert_solvent(&self, step: &str) {
        let reserves = self.pool.get_reserves();
        let lp_owed = self.pool.lp_fees_owed();
        let prot = self.pool.protocol_owed();
        for (i, t) in [self.num.clone(), self.quote.clone()].iter().enumerate() {
            let k = i as u32;
            assert_eq!(
                self.bal(t, &self.pool.address),
                reserves.get_unchecked(k) + lp_owed.get_unchecked(k) + prot.get_unchecked(k),
                "solvency broken on leg {i} at {step}"
            );
        }
    }

    /// If the pool accepts a trade, it must be on-curve. This is the Phase 0
    /// invariant restated as a runtime check.
    fn assert_never_open_off_curve(&self, step: &str) {
        let open = !self.pool.needs_reanchor();
        if open {
            assert!(
                self.pool.is_on_curve(),
                "pool was OPEN while off-curve at {step} — the pot is payable"
            );
        }
    }

    /// Advance the ledger and republish the price so the rate stays fresh.
    fn advance(&mut self, secs: u64) {
        self.env.ledger().with_mut(|l| l.timestamp += secs);
        self.feed.set_price(&self.quote, &self.price);
        self.feed.set_price(&self.num, &ONE_FEED);
    }

    /// One keeper tick: poke, then repeg if the accepted rate moved the pool.
    fn keeper_tick(&self) -> bool {
        let _ = self.pool.try_poke_rate();
        if self.pool.needs_reanchor() {
            self.pool.re_anchor(&MAXU64);
            true
        } else {
            false
        }
    }

    fn try_swap(&self, from_num: bool, amount: i128) -> Option<i128> {
        let (ti, to) = if from_num {
            (self.num.clone(), self.quote.clone())
        } else {
            (self.quote.clone(), self.num.clone())
        };
        match self
            .pool
            .try_swap(&self.trader, &ti, &amount, &to, &0, &MAXU64)
        {
            Ok(Ok(v)) => Some(v),
            _ => None,
        }
    }
}

// ─── the simulation ──────────────────────────────────────────────────────────

/// 60 keeper cycles of a depreciating currency with two-way trade flow.
#[test]
fn sixty_cycles_of_a_drifting_currency() {
    let mut sim = setup();
    sim.assert_solvent("seed");
    assert!(sim.pool.is_on_curve(), "seeded pool must start on-curve");

    let start_shares = sim.pool.total_shares();
    let mut repegs = 0;
    let mut swaps = 0;
    let mut refused = 0;

    // Deterministic pseudo-random walk (SplitMix64) — reproducible, no deps.
    let mut seed: u64 = 0x5EED_1234_ABCD_0001;
    let mut next = || {
        seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };

    for cycle in 0..60 {
        // The currency drifts: a downward bias plus noise, always inside the
        // 500 bps per-update bound so the breaker should never fire.
        let noise = (next() % 300) as i128; // 0..3.00%
        let delta = 10_000 - 60 - noise / 2 + (next() % 60) as i128; // bps of par
        sim.price = (sim.price * delta / 10_000).max(ONE_FEED / 100_000);

        sim.advance(600); // 10-minute cadence, matching Lightecho
        if sim.keeper_tick() {
            repegs += 1;
        }
        sim.assert_never_open_off_curve(&format!("cycle {cycle} post-keeper"));
        sim.assert_solvent(&format!("cycle {cycle} post-keeper"));

        // Two-way trade flow between keeper ticks.
        for _ in 0..3 {
            let from_num = next() % 2 == 0;
            let size = if from_num {
                ((next() % 50) as i128 + 1) * M
            } else {
                ((next() % 50_000) as i128 + 1) * M
            };
            match sim.try_swap(from_num, size) {
                Some(out) => {
                    assert!(out > 0, "an accepted swap must return something");
                    swaps += 1;
                }
                None => refused += 1,
            }
            sim.assert_solvent(&format!("cycle {cycle} post-swap"));
            sim.assert_never_open_off_curve(&format!("cycle {cycle} post-swap"));
        }
    }

    let (_, _, _, breaker) = sim.pool.rate_status();
    assert!(!breaker, "no in-bound move should have tripped the breaker");
    assert_eq!(
        sim.pool.total_shares(),
        start_shares,
        "repegging must never dilute LP claims"
    );
    assert!(sim.pool.is_on_curve(), "pool must end on-curve");
    assert!(swaps > 60, "expected sustained trading, got {swaps}");

    println!(
        "\n60 cycles: {repegs} repegs, {swaps} swaps landed, {refused} refused, \
         final rate {} WAD, s={} total_shares={}",
        sim.pool.get_rate(&sim.quote),
        sim.pool.liquidity_scale(),
        sim.pool.total_shares()
    );
}

/// A trader round-tripping across a repeg gets the market move **minus** the
/// pool's spread — never the market move **plus** the Phase 0 pot.
///
/// Note what is *not* asserted: that the trader loses money. Holding a currency
/// that appreciates 3% and selling it back is directional exposure, and profiting
/// from it is correct. The property that matters is that the pool never pays more
/// than a frictionless conversion at the new oracle rate would.
///
/// The test also records how the spread behaves as the pool skews. Each cycle the
/// trader buys the same leg while that leg appreciates, so the pool accumulates
/// more of what it is long — and the superellipse should quote progressively
/// wider. That widening *is* the automated inventory management, so it is
/// asserted rather than tolerated.
#[test]
fn a_repeg_pays_the_market_move_and_no_more() {
    let mut sim = setup();
    let mut shortfalls: std::vec::Vec<i128> = std::vec::Vec::new();

    for cycle in 0..12 {
        let got = sim.try_swap(true, 50 * M).expect("buy leg");

        sim.price = sim.price * 103 / 100;
        sim.advance(600);
        sim.keeper_tick();

        // Frictionless benchmark: `got` quote units valued at the NEW rate. Both
        // legs are 7-decimal, so the rate multiply is the whole conversion.
        let rate_after = sim.pool.get_rate(&sim.quote);
        let oracle_value = got * rate_after / WAD;

        let back = sim.try_swap(false, got).expect("sell leg");

        // THE safety property: the pool can never beat the oracle rate.
        assert!(
            back < oracle_value,
            "cycle {cycle}: pool paid {back} vs frictionless {oracle_value}"
        );

        let shortfall_bps = (oracle_value - back) * 10_000 / oracle_value;
        // Sanity ceiling: a skewing pool widens, but never to absurdity.
        assert!(
            shortfall_bps < 2_000,
            "cycle {cycle}: shortfall {shortfall_bps} bps is beyond plausible slippage"
        );
        shortfalls.push(shortfall_bps);
        sim.assert_solvent(&format!("cycle {cycle}"));
    }

    // On a balanced pool the spread should be fee-sized plus modest slippage.
    // NOTE: these fixtures run at alpha = beta = 2+sqrt(2), which is the *circle*
    // — the least concentrated member of the superelliptical family. Tightening
    // alpha toward 2 flattens the curve at the peg; see
    // `alpha_controls_quote_tightness` for the measured effect.
    assert!(
        shortfalls[0] < 300,
        "first round trip on a balanced pool cost {} bps",
        shortfalls[0]
    );
    // And it should widen as inventory skews, not stay flat or invert.
    assert!(
        shortfalls[11] > shortfalls[0],
        "spread did not widen as the pool skewed: {} → {}",
        shortfalls[0],
        shortfalls[11]
    );
    println!(
        "\nspread by cycle (bps): first={} mid={} last={}  \
         — widening is the inventory-management behaviour",
        shortfalls[0], shortfalls[6], shortfalls[11]
    );
}

/// With the rate held fixed, a round trip is a strict loss — fees and
/// pool-favoring rounding, every time.
#[test]
fn a_flat_rate_round_trip_always_loses() {
    let mut sim = setup();
    sim.advance(600);
    sim.keeper_tick();
    for cycle in 0..12 {
        let before = sim.bal(&sim.num, &sim.trader);
        let got = sim.try_swap(true, 50 * M).expect("buy leg");
        let back = sim.try_swap(false, got).expect("sell leg");
        assert!(back > 0);
        assert!(
            sim.bal(&sim.num, &sim.trader) < before,
            "cycle {cycle}: a flat round trip must never profit"
        );
        sim.assert_solvent(&format!("cycle {cycle}"));
    }
}

/// An attacker watching the feed cannot land a trade in the off-curve window.
#[test]
fn the_off_curve_window_is_never_tradeable() {
    let mut sim = setup();
    for cycle in 0..15 {
        sim.price = sim.price * 102 / 100;
        sim.advance(600);
        sim.pool.poke_rate(); // rate accepted, pool now closed

        assert!(sim.pool.needs_reanchor(), "cycle {cycle}: pool stayed open");
        // Try to take the pot at every size, both directions.
        for size in [1i128, 1_000, M, 1_000 * M] {
            assert!(
                sim.try_swap(true, size).is_none(),
                "cycle {cycle}: A→B size {size} slipped through"
            );
            assert!(
                sim.try_swap(false, size).is_none(),
                "cycle {cycle}: B→A size {size} slipped through"
            );
        }
        sim.pool.re_anchor(&MAXU64);
        assert!(
            sim.try_swap(true, M).is_some(),
            "cycle {cycle}: never reopened"
        );
    }
}

/// A shock beyond the bound halts trading, and withdrawals stay open throughout.
#[test]
fn a_shock_halts_trading_but_never_traps_the_lp() {
    let mut sim = setup();
    sim.advance(600);
    sim.keeper_tick();

    // 100x — the YieldBlox shape.
    sim.price = ONE_FEED / 10;
    sim.advance(600);
    sim.pool.poke_rate();

    let (_, _, _, breaker) = sim.pool.rate_status();
    assert!(breaker, "a 100x move must latch the breaker");
    assert!(sim.try_swap(true, M).is_none(), "trading must halt");
    assert!(sim.try_swap(false, M).is_none(), "both directions");

    // The LP can still exit, in full.
    let shares = sim.pool.shares_of(&sim.lp);
    let mins = Vec::from_array(&sim.env, [0i128, 0i128]);
    let got = sim.pool.withdraw(&sim.lp, &(shares / 2), &mins, &MAXU64);
    assert!(
        got.get_unchecked(0) > 0 && got.get_unchecked(1) > 0,
        "the exit path must survive a halted pool"
    );
    sim.assert_solvent("post-shock withdraw");
}

/// A pool left alone goes stale, refuses to trade, and recovers on its own once
/// the feed catches up — no operator required.
#[test]
fn a_stale_pool_recovers_without_an_operator() {
    let mut sim = setup();
    sim.env.ledger().with_mut(|l| l.timestamp += MAX_AGE + 1);

    assert!(
        sim.try_swap(true, M).is_none(),
        "a stale pool must not quote"
    );
    let shares = sim.pool.shares_of(&sim.lp);
    let mins = Vec::from_array(&sim.env, [0i128, 0i128]);
    assert!(
        sim.pool
            .try_withdraw(&sim.lp, &(shares / 4), &mins, &MAXU64)
            .is_ok(),
        "withdrawals stay open while stale"
    );

    sim.advance(1);
    sim.keeper_tick();
    assert!(
        sim.try_swap(true, M).is_some(),
        "a fresh feed must reopen the pool with no admin action"
    );
}

/// The anchor is the sole LP; anyone may trade against their inventory.
#[test]
fn operator_mode_holds_across_a_full_cycle() {
    let mut sim = setup();
    sim.pool.set_operator(&sim.lp, &true);
    sim.pool.set_operator_mode(&true);

    for cycle in 0..10 {
        sim.price = sim.price * 101 / 100;
        sim.advance(600);
        sim.keeper_tick();

        // A stranger trades freely...
        assert!(
            sim.try_swap(true, 10 * M).is_some(),
            "cycle {cycle}: trading must stay open to everyone"
        );
        // ...but cannot provide liquidity.
        let amounts = Vec::from_array(&sim.env, [M, 1_000 * M]);
        assert!(
            sim.pool
                .try_deposit(&sim.trader, &amounts, &0, &MAXU64)
                .is_err(),
            "cycle {cycle}: a non-operator must not provide liquidity"
        );
        sim.assert_solvent(&format!("cycle {cycle}"));
    }
}

// Curve-shape calibration (how `alpha` trades quote tightness against depeg
// resistance) lives in `tests/curve_calibration.rs` — it is a property of the
// curve, independent of the rate machinery exercised here.
