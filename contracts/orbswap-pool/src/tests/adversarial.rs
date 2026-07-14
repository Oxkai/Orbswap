//! Adversarial probes (independent audit pass) — attacks the feature tests don't
//! attempt: donation-inflation, dust free-drain, solvency under a chaotic
//! sequence, latecomer-LP fairness, and exact slippage boundaries.

use super::Fixture;
use soroban_sdk::{token, Vec};

/// Classic donation/inflation attack: attacker donates tokens straight to the
/// pool address to skew balance-based accounting. Orbswap tracks reserves in
/// STORAGE (not `balance()`), so a donation must not change share pricing.
#[test]
fn donation_does_not_skew_share_pricing() {
    let f = Fixture::new(7, 10_000_000_000);
    f.init_circular(0);
    let minted_1 = f.deposit_balanced(1_000_000_000);

    // Attacker "donates" a huge amount of token A directly to the pool address.
    token::Client::new(&f.env, &f.token_a).transfer(&f.lp, &f.pool.address, &5_000_000_000);

    // A second identical balanced deposit must mint the SAME shares as the first
    // (reserves in storage are unchanged by the donation).
    let minted_2 = f.deposit_balanced(1_000_000_000);
    assert_eq!(
        minted_2,
        minted_1 + crate::types::MINIMUM_LIQUIDITY, // 2nd deposit has no min-liq deduction
        "donation skewed share pricing"
    );
}

/// Dust swap whose output rounds to zero must be rejected — otherwise a trader
/// could feed dust in and (worse: the pool silently keeps it — acceptable — or
/// state drifts). The contract returns InsufficientLiquidity on out == 0.
#[test]
fn dust_swap_zero_output_rejected() {
    let f = Fixture::new(7, 10_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    // 1 stroop in vs a 100-token reserve → out rounds to 0 → must error, not Ok(0).
    let r = f
        .pool
        .try_swap(&f.lp, &f.token_a, &1, &f.token_b, &0, &u64::MAX);
    assert!(r.is_err(), "zero-output dust swap must revert");
}

/// SuperElliptical pools enforce the fuzz-mandated minimum trade in NORMALIZED
/// space (dx̂ = internal·WAD/s), so the guard scales with pool size: in a large
/// pool a 1-stroop trade is sub-resolution for the csemm math and must revert.
/// (Audit finding: the original guard checked internal units with a threshold
/// below the native quantum — dead code. Now normalized.)
#[test]
fn superelliptical_dust_guard_scales_with_pool() {
    // Big pool: 200k tokens each → s = 2e23 → 1 stroop gives dx̂ = 5e5 < 1e6.
    let f = Fixture::new(7, 4_000_000_000_000);
    f.init_superelliptical(3 * super::_WAD, 5 * super::_WAD, 0);
    f.deposit_balanced(2_000_000_000_000);
    let r = f
        .pool
        .try_swap(&f.lp, &f.token_a, &1, &f.token_b, &0, &u64::MAX);
    assert!(r.is_err(), "sub-resolution csemm trade must revert");

    // Sanity: a normal-sized trade on the same pool still works.
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &1_000_000_000, &f.token_b, &0, &u64::MAX);
    assert!(out > 0);
}

/// Oversized swap (pushes past the reserve extent / drains the out side) must
/// error cleanly — never panic, never mint value.
#[test]
fn oversized_swap_errors_cleanly() {
    let f = Fixture::new(7, 100_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    for amt in [10_000_000_000i128, 50_000_000_000, 100_000_000_000] {
        let r = f
            .pool
            .try_swap(&f.lp, &f.token_a, &amt, &f.token_b, &0, &u64::MAX);
        assert!(r.is_err(), "oversized swap {amt} should error");
    }
}

/// Chaos sequence: deposits, swaps both directions, partial withdrawals — after
/// every step the pool's actual token balances must exactly equal its stored
/// reserves (solvency: no phantom reserves, no leaked tokens).
#[test]
fn solvency_reserves_match_balances_through_chaos() {
    let f = Fixture::new(7, 100_000_000_000);
    f.init_circular(30); // with fees, the harder case
                         // Balance == reserve + protocol owed + LP fees owed (fees sit outside the curve).
    let check = |f: &Fixture| {
        let r = f.pool.get_reserves();
        let po = f.pool.protocol_owed();
        let lo = f.pool.lp_fees_owed();
        assert_eq!(
            f.balance(&f.token_a, &f.pool.address),
            r.get_unchecked(0) + po.get_unchecked(0) + lo.get_unchecked(0),
            "token A balance != reserve+protocol+lp"
        );
        assert_eq!(
            f.balance(&f.token_b, &f.pool.address),
            r.get_unchecked(1) + po.get_unchecked(1) + lo.get_unchecked(1),
            "token B balance != reserve+protocol+lp"
        );
    };

    let minted = f.deposit_balanced(10_000_000_000);
    check(&f);
    f.pool
        .swap(&f.lp, &f.token_a, &500_000_000, &f.token_b, &0, &u64::MAX);
    check(&f);
    f.pool
        .swap(&f.lp, &f.token_b, &900_000_000, &f.token_a, &0, &u64::MAX);
    check(&f);
    // Mid-chaos deposit must be proportional to the (now off-balance) reserves.
    let r = f.pool.get_reserves();
    let d0 = 100_000_000i128;
    let d1 = d0 * r.get_unchecked(1) / r.get_unchecked(0);
    let amounts = Vec::from_array(&f.env, [d0, d1]);
    f.pool.deposit(&f.lp, &amounts, &0, &u64::MAX);
    check(&f);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &(minted / 3), &mins, &u64::MAX);
    check(&f);
    f.pool
        .swap(&f.lp, &f.token_a, &100_000_000, &f.token_b, &0, &u64::MAX);
    check(&f);
}

/// A latecomer LP who deposits after the price moved and withdraws immediately
/// must not extract more value than deposited (no sandwich-the-pool profit).
#[test]
fn latecomer_lp_cannot_profit_by_deposit_withdraw() {
    let f = Fixture::new(7, 100_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(10_000_000_000);
    // Move the price off balance.
    f.pool
        .swap(&f.lp, &f.token_a, &2_000_000_000, &f.token_b, &0, &u64::MAX);

    // Latecomer = same LP address here, but we track deltas around the cycle.
    let a0 = f.balance(&f.token_a, &f.lp);
    let b0 = f.balance(&f.token_b, &f.lp);
    let r = f.pool.get_reserves();
    // Proportional deposit matching current (skewed) reserve ratio.
    let d0 = 1_000_000_000i128;
    let d1 = d0 * r.get_unchecked(1) / r.get_unchecked(0);
    let amounts = Vec::from_array(&f.env, [d0, d1]);
    let minted = f.pool.deposit(&f.lp, &amounts, &0, &u64::MAX);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &minted, &mins, &u64::MAX);

    // Net position after deposit→withdraw: never above start on either token.
    assert!(
        f.balance(&f.token_a, &f.lp) <= a0,
        "extracted extra token A"
    );
    assert!(
        f.balance(&f.token_b, &f.lp) <= b0,
        "extracted extra token B"
    );
}

/// Slippage boundary is exact: min_out == quote succeeds, quote+1 reverts.
#[test]
fn min_out_boundary_is_exact() {
    let f = Fixture::new(7, 10_000_000_000);
    f.init_circular(30);
    f.deposit_balanced(1_000_000_000);
    let q = f.pool.quote(&f.token_a, &100_000_000, &f.token_b);
    // quote+1 must fail…
    let r = f.pool.try_swap(
        &f.lp,
        &f.token_a,
        &100_000_000,
        &f.token_b,
        &(q + 1),
        &u64::MAX,
    );
    assert!(r.is_err(), "min_out = quote+1 must revert");
    // …and exactly quote must succeed with exactly quote out.
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &100_000_000, &f.token_b, &q, &u64::MAX);
    assert_eq!(out, q);
}
