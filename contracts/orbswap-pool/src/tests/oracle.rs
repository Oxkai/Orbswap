//! Tests for the price oracle (todo.md §2.7).

use super::Fixture;
use crate::types::WAD;
use soroban_sdk::testutils::Ledger as _;

fn advance(f: &Fixture, secs: u64) {
    f.env.ledger().with_mut(|l| l.timestamp += secs);
}

#[test]
fn spot_price_is_one_at_balance() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    // Balanced pool → price ≈ 1.0 (WAD).
    let p = f.pool.get_spot_price();
    let diff = (p - WAD).abs();
    assert!(diff < WAD / 1_000_000, "spot not ~1.0: {p}");
}

#[test]
fn cumulative_grows_and_twap_tracks_price() {
    let f = Fixture::new(7, 5_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);

    // No swaps yet → cumulative flat.
    let (cum0, t0) = f.pool.price_cumulative();

    // Advance time, then a swap accumulates the (pre-swap ~1.0) price over the gap.
    advance(&f, 100);
    f.pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    let (cum1, t1) = f.pool.price_cumulative();
    assert!(cum1 > cum0, "cumulative did not grow");
    assert_eq!(t1, t0 + 100);

    // TWAP over the first window ≈ 1.0 (the balanced price that held).
    let twap = (cum1 - cum0) / ((t1 - t0) as i128);
    assert!((twap - WAD).abs() < WAD / 100_000, "twap not ~1.0: {twap}");

    // Second window: price is now off-balance (post-swap). Accumulate again.
    advance(&f, 50);
    f.pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    let (cum2, t2) = f.pool.price_cumulative();
    let twap2 = (cum2 - cum1) / ((t2 - t1) as i128);
    // token0 got cheaper in token1 after selling token0 in → price of token0 < 1.
    assert!(
        twap2 < WAD,
        "price should drop after selling token0: {twap2}"
    );
    assert!(twap2 > 0);
}

#[test]
fn no_update_without_time_passing() {
    let f = Fixture::new(7, 5_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    // Two swaps in the same ledger time → cumulative unchanged between them.
    f.pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    let (cum_a, _) = f.pool.price_cumulative();
    f.pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    let (cum_b, _) = f.pool.price_cumulative();
    assert_eq!(cum_a, cum_b, "cumulative changed with no elapsed time");
}
