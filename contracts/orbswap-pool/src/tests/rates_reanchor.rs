//! Phase 3 — `re_anchor` and the off-curve trading gate (todo.md §Phase 3).
//!
//! These are the tests that close the Phase 0 defect: a rate move must never
//! leave the pool open and off-curve, because the first trader takes the whole
//! revaluation at any trade size.

use super::{move_rate, rate_pool as seeded, Fixture, RATE_FEE_BPS as FEE_BPS, RATE_MINT as MINT};
use crate::types::TWO_PLUS_SQRT2 as ALPHA;
use crate::OrbswapError;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::Vec;

// ─── the pool closes on a rate move ──────────────────────────────────────────

#[test]
fn seeded_pool_starts_on_curve_and_open() {
    let (f, _) = seeded();
    assert!(f.pool.is_on_curve(), "a fresh balanced deposit is on-curve");
    assert!(!f.pool.needs_reanchor());
}

#[test]
fn accepted_rate_move_closes_the_pool() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 101, 100); // +1%
    assert!(
        f.pool.needs_reanchor(),
        "an accepted rate move must close the pool"
    );
    assert_eq!(
        f.pool
            .try_swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX),
        Err(Ok(OrbswapError::OffCurve)),
        "this is the Phase 0 pot; it must not be payable"
    );
}

#[test]
fn rate_move_actually_takes_the_pool_off_curve() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 101, 100);
    assert!(
        !f.pool.is_on_curve(),
        "the revaluation moves the state off the invariant"
    );
    assert!(f.pool.curve_drift() != 0);
}

#[test]
fn unchanged_rate_does_not_close_the_pool() {
    let (f, feed) = seeded();
    feed.set_timestamp(&(f.env.ledger().timestamp() + 1));
    f.pool.poke_rate();
    assert!(
        !f.pool.needs_reanchor(),
        "a poke that changes nothing must not close the pool"
    );
}

#[test]
fn every_trading_path_is_closed_while_off_curve() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 101, 100);
    let off = Err(Ok(OrbswapError::OffCurve));

    assert_eq!(
        f.pool
            .try_swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX),
        off
    );
    assert_eq!(
        f.pool.try_swap_exact_out(
            &f.lp,
            &f.token_a,
            &f.token_b,
            &1_000_000,
            &i128::MAX,
            &u64::MAX
        ),
        off
    );
    let amounts = Vec::from_array(&f.env, [10_000_000i128, 10_000_000_000i128]);
    assert_eq!(f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX), off);
}

#[test]
fn withdraw_stays_open_while_off_curve() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 101, 100);
    let shares = f.pool.shares_of(&f.lp);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let got = f.pool.withdraw(&f.lp, &(shares / 2), &mins, &u64::MAX);
    assert!(
        got.get_unchecked(0) > 0 && got.get_unchecked(1) > 0,
        "an LP must always be able to exit"
    );
}

// ─── re_anchor restores it ───────────────────────────────────────────────────

#[test]
fn reanchor_reopens_the_pool_on_the_new_curve() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 101, 100);
    f.pool.re_anchor(&u64::MAX);

    assert!(!f.pool.needs_reanchor());
    assert!(f.pool.is_on_curve(), "re_anchor must restore the invariant");
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(out > 0, "trading resumes");
}

#[test]
fn reanchor_moves_s_but_not_total_shares() {
    let (f, feed) = seeded();
    let shares_before = f.pool.total_shares();
    move_rate(&f, &feed, 110, 100); // +10%
    let s_before = f.pool.liquidity_scale();
    let s_after = f.pool.re_anchor(&u64::MAX);

    assert_ne!(s_after, s_before, "s must move");
    assert_eq!(
        f.pool.total_shares(),
        shares_before,
        "LP claims must not be diluted by an FX move"
    );
}

#[test]
fn reanchor_preserves_relative_lp_claims() {
    let (f, feed) = seeded();
    let lp_shares = f.pool.shares_of(&f.lp);
    let total = f.pool.total_shares();
    move_rate(&f, &feed, 105, 100);
    f.pool.re_anchor(&u64::MAX);
    assert_eq!(f.pool.shares_of(&f.lp), lp_shares);
    assert_eq!(f.pool.total_shares(), total);
}

#[test]
fn reanchor_marks_share_value_to_market() {
    // The quote leg depreciates 10%; a withdrawal afterwards should reflect it.
    let (f, feed) = seeded();
    let shares = f.pool.shares_of(&f.lp);
    move_rate(&f, &feed, 90, 100);
    f.pool.re_anchor(&u64::MAX);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let got = f.pool.withdraw(&f.lp, &(shares / 2), &mins, &u64::MAX);
    assert!(
        got.get_unchecked(0) > 0 && got.get_unchecked(1) > 0,
        "withdrawal still returns both legs after a repeg"
    );
}

#[test]
fn reanchor_preserves_solvency() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 103, 100);
    f.pool.re_anchor(&u64::MAX);

    // balance == reserves + ProtocolOwed + LpFeesOwed, exact to the integer.
    for (i, token) in [f.token_a.clone(), f.token_b.clone()].iter().enumerate() {
        let bal = f.balance(token, &f.pool.address);
        let res = f.pool.get_reserves().get_unchecked(i as u32);
        let owed = f.pool.lp_fees_owed().get_unchecked(i as u32);
        let prot = f.pool.protocol_owed().get_unchecked(i as u32);
        assert_eq!(bal, res + owed + prot, "solvency broken on token {i}");
    }
}

#[test]
fn reanchor_then_roundtrip_never_profits() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 102, 100);
    f.pool.re_anchor(&u64::MAX);

    let start = 10_000_000i128;
    let mid = f
        .pool
        .swap(&f.lp, &f.token_a, &start, &f.token_b, &0, &u64::MAX);
    let back = f
        .pool
        .swap(&f.lp, &f.token_b, &mid, &f.token_a, &0, &u64::MAX);
    assert!(back < start, "round trip must not profit after a repeg");
}

#[test]
fn reanchor_on_empty_pool_is_a_noop() {
    let f = Fixture::new(7, MINT);
    f.init_superelliptical(ALPHA, ALPHA, FEE_BPS);
    assert_eq!(f.pool.re_anchor(&u64::MAX), 0);
    assert!(!f.pool.needs_reanchor());
}

#[test]
fn reanchor_is_idempotent() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 104, 100);
    let first = f.pool.re_anchor(&u64::MAX);
    let second = f.pool.re_anchor(&u64::MAX);
    assert_eq!(
        first, second,
        "re-anchoring an on-curve pool must not drift"
    );
}

#[test]
fn reanchor_survives_repeated_rate_moves() {
    let (f, feed) = seeded();
    for step in [101i128, 102, 100, 101, 103] {
        move_rate(&f, &feed, step, 100);
        f.pool.re_anchor(&u64::MAX);
        assert!(f.pool.is_on_curve(), "drifted after a {step}% step");
        let out = f
            .pool
            .swap(&f.lp, &f.token_a, &1_000_000, &f.token_b, &0, &u64::MAX);
        assert!(out > 0);
    }
}

#[test]
fn reanchor_respects_the_deadline() {
    let (f, feed) = seeded();
    move_rate(&f, &feed, 101, 100);
    // Ledger time starts at 0, so a deadline is only in the past once time moves.
    f.env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(
        f.pool.try_re_anchor(&50),
        Err(Ok(OrbswapError::Expired)),
        "an expired re_anchor must not land"
    );
    assert!(
        f.pool.needs_reanchor(),
        "a rejected re_anchor must leave the pool closed"
    );
}
