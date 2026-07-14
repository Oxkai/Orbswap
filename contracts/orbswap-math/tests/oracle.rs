//! Integration tests for `oracle.rs` — every edge case from todo.md §1.8.

mod common;

use common::Rng;
use orbswap_math::fixed_point::FIXED_SCALE;
use orbswap_math::oracle::{accumulate, twap, OracleError};

// ---------------------------------------------------------------- basics

#[test]
fn accumulate_adds_price_times_elapsed() {
    let c = accumulate(0, 2 * FIXED_SCALE, 10).unwrap();
    assert_eq!(c, 2 * FIXED_SCALE * 10);
    let c = accumulate(c, 3 * FIXED_SCALE, 5).unwrap();
    assert_eq!(c, 2 * FIXED_SCALE * 10 + 3 * FIXED_SCALE * 5);
}

#[test]
fn constant_price_gives_that_price() {
    // Accumulate a constant price over several intervals; TWAP == price.
    let price = 7 * FIXED_SCALE / 4; // 1.75
    let mut cum = 0i128;
    let start = cum;
    let mut total_t = 0;
    for dt in [3, 11, 1, 20, 7] {
        cum = accumulate(cum, price, dt).unwrap();
        total_t += dt;
    }
    assert_eq!(twap(start, cum, total_t).unwrap(), price);
}

#[test]
fn twap_is_time_weighted_average() {
    // Two intervals: price 1.0 for 10, price 2.0 for 30 → TWAP = (10·1 + 30·2)/40 = 1.75.
    let mut cum = 0i128;
    cum = accumulate(cum, FIXED_SCALE, 10).unwrap();
    let mid = cum;
    cum = accumulate(cum, 2 * FIXED_SCALE, 30).unwrap();
    assert_eq!(twap(0, cum, 40).unwrap(), 7 * FIXED_SCALE / 4);
    // Sub-window (only the second interval) → 2.0.
    assert_eq!(twap(mid, cum, 30).unwrap(), 2 * FIXED_SCALE);
}

// ---------------------------------------------------------------- errors

#[test]
fn zero_window_and_invalid_inputs() {
    assert_eq!(twap(0, 100, 0), Err(OracleError::ZeroWindow));
    assert_eq!(twap(0, 100, -5), Err(OracleError::ZeroWindow));
    assert_eq!(accumulate(0, -1, 10), Err(OracleError::InvalidInput));
    assert_eq!(
        accumulate(0, FIXED_SCALE, -1),
        Err(OracleError::InvalidInput)
    );
    assert_eq!(accumulate(0, i128::MAX, 1_000), Err(OracleError::Overflow));
}

// ---------------------------------------------------------------- wrapping across overflow

#[test]
fn twap_correct_across_cumulative_overflow() {
    // Start the cumulative near i128::MAX so the next accumulate wraps past it.
    // The TWAP over the wrapping window must still be the true price.
    let price = 5 * FIXED_SCALE; // 5.0
    let elapsed = 1_000i128;
    let start = i128::MAX - (price * elapsed) / 2; // wrap lands mid-interval
    let end = accumulate(start, price, elapsed).unwrap();
    // The accumulator wrapped (end < start), but the difference is exact.
    assert!(end < start, "expected wrap-around");
    assert_eq!(twap(start, end, elapsed).unwrap(), price);
}

#[test]
fn twap_matches_oracle_random() {
    let mut rng = Rng::new(0x0DAC1E);
    for _ in 0..50_000 {
        // Single interval: TWAP of one price over dt is exactly that price.
        let price = rng.range_i128(0, 1_000 * FIXED_SCALE);
        let dt = rng.range_i128(1, 1_000_000);
        // Start from an arbitrary (possibly large) cumulative to exercise wrapping.
        let start = rng.range_i128(i128::MIN / 2, i128::MAX / 2);
        let end = accumulate(start, price, dt).unwrap();
        assert_eq!(
            twap(start, end, dt).unwrap(),
            price,
            "price={price} dt={dt}"
        );
    }
}
