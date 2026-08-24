//! Curve calibration — how the shape parameter `alpha` trades quote tightness
//! against depeg resistance, measured rather than asserted.
//!
//! `alpha` selects a member of the superelliptical family: toward `2` the curve
//! approaches a **constant-sum line** (flat at the balanced point, near-zero
//! slippage, but no resistance to being drained); at `2+sqrt(2)` it is exactly the
//! **circle**; larger values approach a boxy LMSR.
//!
//! This matters for rate-aware pools specifically. An FX settlement pool takes its
//! *price* from the oracle, so the curve's remaining job is **inventory
//! management** — quote tight while balanced, widen as the pool skews. That argues
//! for a much flatter shape than the circle the demo pools ship with.
//!
//! Measured at a 10,000-unit pool, 30 bps fee (see `alpha_sweep` output):
//!
//! | alpha | 10% trade | 40% trade |
//! |-------|-----------|-----------|
//! | 2.001 |     0 bps |     2 bps |
//! | 2.01  |     7 bps |    29 bps |
//! | 2.05  |    34 bps |   138 bps |
//! | 2.20  |   118 bps |   465 bps |
//! | 3.414 |   396 bps |  1432 bps |  <- the circle
//!
//! At 10% of reserves `alpha = 2.01` quotes **56x tighter than the circle**. The
//! cost of going flatter is depeg resistance: a near-constant-sum pool will trade
//! all the way down at par, so the oracle guards (deviation bound, breaker) carry
//! proportionally more of the safety burden.

use orbswap_pool::{OrbswapPool, OrbswapPoolClient, PoolMode};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049;
const M: i128 = 10_000_000;
const FEE_BPS: i128 = 30;
const MAXU64: u64 = u64::MAX;

fn probe(alpha: i128, seed: i128, frac_pct: i128) -> (i128, i128, i128) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let tr = Address::generate(&env);

    let mk = |e: &Env, a: &Address| {
        let sac = e.register_stellar_asset_contract_v2(a.clone());
        let addr = sac.address();
        (addr.clone(), token::StellarAssetClient::new(e, &addr))
    };
    let (a_t, a_adm) = mk(&env, &admin);
    let (b_t, b_adm) = mk(&env, &admin);
    for w in [&lp, &tr] {
        a_adm.mint(w, &(seed * 100 * M));
        b_adm.mint(w, &(seed * 100 * M));
    }

    let pid = env.register(OrbswapPool, ());
    let p = OrbswapPoolClient::new(&env, &pid);
    p.initialize(
        &Vec::from_array(&env, [a_t.clone(), b_t.clone()]),
        &PoolMode::SuperElliptical,
        &alpha,
        &alpha,
        &FEE_BPS,
        &admin,
    );
    p.deposit(
        &lp,
        &Vec::from_array(&env, [seed * M, seed * M]),
        &0,
        &MAXU64,
    );

    let spend = seed * M * frac_pct / 100;
    let got = p.swap(&tr, &a_t, &spend, &b_t, &0, &MAXU64);
    let back = p.swap(&tr, &b_t, &got, &a_t, &0, &MAXU64);

    // Solvency must still hold exactly.
    let res = p.get_reserves();
    let owed = p.lp_fees_owed();
    let prot = p.protocol_owed();
    for (i, t) in [a_t.clone(), b_t.clone()].iter().enumerate() {
        let k = i as u32;
        assert_eq!(
            token::Client::new(&env, t).balance(&p.address),
            res.get_unchecked(k) + owed.get_unchecked(k) + prot.get_unchecked(k),
            "solvency broken leg {i}"
        );
    }
    ((spend - back) * 10_000 / spend, spend, back)
}

/// A round trip must always cost the trader, at every shape and size.
///
/// Note the measured cost lands *below* two nominal fees on large trades. That is
/// correct, not a leak: the second fee is levied on a notional already reduced by
/// leg-one slippage. It is also why round-trip cost is a poor proxy for quote
/// tightness — see [`alpha_sets_quote_tightness_single_leg`].
#[test]
fn a_round_trip_always_costs_the_trader() {
    println!("\n=== round-trip cost, parity pool (two nominal fees = 60 bps) ===");
    for frac in [1i128, 10, 40, 70] {
        let (c1, s, b) = probe(2_050_000_000_000_000_000, 10_000, frac);
        let (c2, ..) = probe(TWO_PLUS_SQRT2, 10_000, frac);
        let (c3, ..) = probe(6_000_000_000_000_000_000, 10_000, frac);
        println!(
            "trade={frac:>2}% of reserves | a=2.05 {c1:>4} bps | circle {c2:>4} bps | a=6.0 {c3:>4} bps   (spend={s} back={b})"
        );
    }
    // The load-bearing claim: a round trip must never RETURN more than it cost.
    for frac in [1i128, 10, 40, 70] {
        for a in [
            2_050_000_000_000_000_000,
            TWO_PLUS_SQRT2,
            6_000_000_000_000_000_000,
        ] {
            let (_, spend, back) = probe(a, 10_000, frac);
            assert!(
                back < spend,
                "value created: spend={spend} back={back} alpha={a} frac={frac}"
            );
        }
    }
}

/// Single-leg execution vs the frictionless expectation. Unlike a round trip,
/// this is not confounded by the second fee being levied on a slippage-reduced
/// base, so it actually measures quote tightness.
fn slippage_bps(alpha: i128, seed: i128, frac_pct: i128) -> i128 {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let lp = Address::generate(&env);
    let tr = Address::generate(&env);
    let mk = |e: &Env, a: &Address| {
        let sac = e.register_stellar_asset_contract_v2(a.clone());
        let addr = sac.address();
        (addr.clone(), token::StellarAssetClient::new(e, &addr))
    };
    let (a_t, a_adm) = mk(&env, &admin);
    let (b_t, b_adm) = mk(&env, &admin);
    for w in [&lp, &tr] {
        a_adm.mint(w, &(seed * 100 * M));
        b_adm.mint(w, &(seed * 100 * M));
    }
    let pid = env.register(OrbswapPool, ());
    let p = OrbswapPoolClient::new(&env, &pid);
    p.initialize(
        &Vec::from_array(&env, [a_t.clone(), b_t.clone()]),
        &PoolMode::SuperElliptical,
        &alpha,
        &alpha,
        &FEE_BPS,
        &admin,
    );
    p.deposit(
        &lp,
        &Vec::from_array(&env, [seed * M, seed * M]),
        &0,
        &MAXU64,
    );

    let spend = seed * M * frac_pct / 100;
    let out = p.swap(&tr, &a_t, &spend, &b_t, &0, &MAXU64);
    // At a 1:1 balanced pool the frictionless fill is spend, less the fee.
    let ideal = spend * (10_000 - FEE_BPS) / 10_000;
    (ideal - out) * 10_000 / ideal
}

/// Quote tightness by shape, measured on a single leg.
#[test]
fn alpha_sets_quote_tightness_single_leg() {
    println!("\n=== single-leg slippage vs frictionless, by curve shape ===");
    println!(
        "{:>6} | {:>12} | {:>12} | {:>12}",
        "trade", "a=2.05 flat", "circle 3.41", "a=6.0 boxy"
    );
    for frac in [1i128, 10, 25, 40, 70] {
        let f = slippage_bps(2_050_000_000_000_000_000, 10_000, frac);
        let c = slippage_bps(TWO_PLUS_SQRT2, 10_000, frac);
        let w = slippage_bps(6_000_000_000_000_000_000, 10_000, frac);
        println!("{frac:>5}% | {f:>12} | {c:>12} | {w:>12}");
    }
    // At a size that stresses the curve, a flatter shape must quote tighter.
    let f = slippage_bps(2_050_000_000_000_000_000, 10_000, 40);
    let c = slippage_bps(TWO_PLUS_SQRT2, 10_000, 40);
    let w = slippage_bps(6_000_000_000_000_000_000, 10_000, 40);
    assert!(
        f < c && c < w,
        "expected flat < circle < boxy, got {f} / {c} / {w}"
    );
}

/// The full sweep behind the table in this module's docs.
#[test]
fn alpha_sweep() {
    println!("\n=== how tight can alpha go? (slippage bps at 10% / 40% of reserves) ===");
    for (name, a) in [
        ("2.001", 2_001_000_000_000_000_000i128),
        ("2.01", 2_010_000_000_000_000_000),
        ("2.05", 2_050_000_000_000_000_000),
        ("2.20", 2_200_000_000_000_000_000),
        ("2.50", 2_500_000_000_000_000_000),
        ("3.414", TWO_PLUS_SQRT2),
    ] {
        let s10 = slippage_bps(a, 10_000, 10);
        let s40 = slippage_bps(a, 10_000, 40);
        println!("alpha={name:>6}  10%: {s10:>5} bps   40%: {s40:>5} bps");
    }
}
