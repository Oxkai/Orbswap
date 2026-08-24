//! Phase 4 — oracle safety (todo.md §Phase 4).
//!
//! Directly answers the Feb 2026 YieldBlox/Blend V2 precedent: an oracle that
//! faithfully reported a manipulated price. Every guard here closes the pool
//! rather than repricing, and **withdrawals stay open through all of them.**

use super::{move_rate, rate_pool, ONE_FEED};
use crate::OrbswapError;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::Vec;

const SWAP_IN: i128 = 10_000_000;

/// The contract error a small A→B swap returns, or `None` if it succeeds.
fn swap_err(f: &super::Fixture) -> Option<OrbswapError> {
    match f
        .pool
        .try_swap(&f.lp, &f.token_a, &SWAP_IN, &f.token_b, &0, &u64::MAX)
    {
        Err(Ok(e)) => Some(e),
        _ => None,
    }
}

// ─── staleness: a soft close that self-heals ─────────────────────────────────

#[test]
fn stale_rate_blocks_swap() {
    let (f, _) = rate_pool();
    f.env.ledger().with_mut(|l| l.timestamp += 3_601);
    assert_eq!(swap_err(&f), Some(OrbswapError::RateStale));
}

#[test]
fn stale_rate_blocks_deposit() {
    let (f, _) = rate_pool();
    f.env.ledger().with_mut(|l| l.timestamp += 3_601);
    let amounts = Vec::from_array(&f.env, [10_000_000i128, 10_000_000_000i128]);
    assert_eq!(
        f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX),
        Err(Ok(OrbswapError::RateStale))
    );
}

#[test]
fn stale_rate_allows_withdraw() {
    let (f, _) = rate_pool();
    f.env.ledger().with_mut(|l| l.timestamp += 3_601);
    let shares = f.pool.shares_of(&f.lp);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let got = f.pool.withdraw(&f.lp, &(shares / 2), &mins, &u64::MAX);
    assert!(got.get_unchecked(0) > 0 && got.get_unchecked(1) > 0);
}

#[test]
fn a_fresh_poke_reopens_a_stale_pool_without_an_admin() {
    let (f, feed) = rate_pool();
    f.env.ledger().with_mut(|l| l.timestamp += 3_601);
    assert_eq!(swap_err(&f), Some(OrbswapError::RateStale));

    // Same price, fresh timestamp: no repeg needed, pool reopens on its own.
    feed.set_timestamp(&f.env.ledger().timestamp());
    f.pool.poke_rate();
    assert_eq!(
        swap_err(&f),
        None,
        "staleness is lag, not manipulation — it must not need a human"
    );
}

// ─── deviation: a hard latch ─────────────────────────────────────────────────

#[test]
fn hundredfold_move_trips_the_breaker_and_is_not_adopted() {
    let (f, feed) = rate_pool();
    let before = f.pool.get_rate(&f.token_b);
    feed.set_price(&f.token_b, &(ONE_FEED / 10)); // 100x
    f.pool.poke_rate();

    let (_, _, _, breaker) = f.pool.rate_status();
    assert!(breaker, "the YieldBlox shape must latch the breaker");
    assert_eq!(f.pool.get_rate(&f.token_b), before);
}

#[test]
fn breaker_blocks_swap_and_deposit() {
    let (f, _) = rate_pool();
    f.pool.set_breaker(&true);
    assert_eq!(swap_err(&f), Some(OrbswapError::RateBreakerTripped));
    let amounts = Vec::from_array(&f.env, [10_000_000i128, 10_000_000_000i128]);
    assert_eq!(
        f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX),
        Err(Ok(OrbswapError::RateBreakerTripped))
    );
}

#[test]
fn breaker_allows_withdraw() {
    let (f, _) = rate_pool();
    f.pool.set_breaker(&true);
    let shares = f.pool.shares_of(&f.lp);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let got = f.pool.withdraw(&f.lp, &(shares / 2), &mins, &u64::MAX);
    assert!(
        got.get_unchecked(0) > 0 && got.get_unchecked(1) > 0,
        "the exit path must survive a tripped breaker"
    );
}

#[test]
fn breaker_does_not_self_clear_when_the_price_returns() {
    let (f, feed) = rate_pool();
    feed.set_price(&f.token_b, &(ONE_FEED / 10));
    f.pool.poke_rate(); // trips

    // Price comes back to normal — the breaker must NOT release on its own.
    feed.set_price(&f.token_b, &(ONE_FEED / 1_000));
    assert_eq!(
        f.pool.try_poke_rate(),
        Err(Ok(OrbswapError::RateBreakerTripped))
    );
    let (_, _, _, breaker) = f.pool.rate_status();
    assert!(breaker);
}

#[test]
fn admin_can_clear_the_breaker_and_trading_resumes() {
    let (f, feed) = rate_pool();
    feed.set_price(&f.token_b, &(ONE_FEED / 10));
    f.pool.poke_rate();
    assert_eq!(swap_err(&f), Some(OrbswapError::RateBreakerTripped));

    f.pool.set_breaker(&false);
    assert_eq!(swap_err(&f), None);
}

#[test]
fn oracle_unavailable_blocks_the_poke_and_never_falls_back() {
    let (f, feed) = rate_pool();
    let before = f.pool.get_rate(&f.token_b);
    feed.set_down(&true);
    assert_eq!(
        f.pool.try_poke_rate(),
        Err(Ok(OrbswapError::OracleUnavailable))
    );
    assert_eq!(
        f.pool.get_rate(&f.token_b),
        before,
        "a down feed must never produce a substituted rate"
    );
}

// ─── bounds ──────────────────────────────────────────────────────────────────

#[test]
fn tightening_the_deviation_bound_takes_effect() {
    let (f, feed) = rate_pool();
    f.pool.set_rate_bounds(&3_600, &50); // 0.5%
    feed.set_price(&f.token_b, &(ONE_FEED / 1_000 * 102 / 100)); // 2%
    f.pool.poke_rate();
    let (_, _, _, breaker) = f.pool.rate_status();
    assert!(breaker, "a 2% move must trip a 0.5% bound");
}

#[test]
fn shortening_the_staleness_window_takes_effect() {
    let (f, _) = rate_pool();
    f.pool.set_rate_bounds(&60, &500);
    f.env.ledger().with_mut(|l| l.timestamp += 61);
    assert_eq!(swap_err(&f), Some(OrbswapError::RateStale));
}

#[test]
fn rate_bounds_reject_degenerate_values() {
    let (f, _) = rate_pool();
    assert_eq!(
        f.pool.try_set_rate_bounds(&0, &500),
        Err(Ok(OrbswapError::InvalidRateConfig))
    );
    assert_eq!(
        f.pool.try_set_rate_bounds(&3_600, &0),
        Err(Ok(OrbswapError::InvalidRateConfig))
    );
}

#[test]
fn rate_bounds_require_a_configured_pool() {
    let f = super::Fixture::new(7, super::RATE_MINT);
    f.init_superelliptical(
        crate::types::TWO_PLUS_SQRT2,
        crate::types::TWO_PLUS_SQRT2,
        super::RATE_FEE_BPS,
    );
    assert_eq!(
        f.pool.try_set_rate_bounds(&3_600, &500),
        Err(Ok(OrbswapError::InvalidRateConfig))
    );
}

// ─── the guards compose ──────────────────────────────────────────────────────

#[test]
fn breaker_outranks_a_pending_reanchor() {
    let (f, feed) = rate_pool();
    move_rate(&f, &feed, 101, 100); // pool now off-curve
    f.pool.set_breaker(&true);
    assert_eq!(
        swap_err(&f),
        Some(OrbswapError::RateBreakerTripped),
        "the most severe condition should surface first"
    );
}

#[test]
fn clearing_the_breaker_does_not_reopen_an_off_curve_pool() {
    let (f, feed) = rate_pool();
    move_rate(&f, &feed, 101, 100);
    f.pool.set_breaker(&true);
    f.pool.set_breaker(&false);
    assert_eq!(
        swap_err(&f),
        Some(OrbswapError::OffCurve),
        "an admin clearing the breaker must not skip the repeg"
    );
    f.pool.re_anchor(&u64::MAX);
    assert_eq!(swap_err(&f), None);
}
