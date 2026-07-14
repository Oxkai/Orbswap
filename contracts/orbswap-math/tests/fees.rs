//! Integration tests for `fees.rs` — every edge case from todo.md §1.7.

mod common;

use common::Rng;
use orbswap_math::fees::{apply_fee, split_protocol_fee, FeeError, BPS_DENOM};

// ---------------------------------------------------------------- apply_fee

#[test]
fn apply_fee_zero_and_max() {
    // 0 bps → no fee.
    assert_eq!(apply_fee(1_000_000, 0), Ok((1_000_000, 0)));
    // 10000 bps (100%) → entire input is fee.
    assert_eq!(apply_fee(1_000_000, BPS_DENOM), Ok((0, 1_000_000)));
}

#[test]
fn apply_fee_known_values() {
    // 0.3% of 1_000_000 = 3000 exactly.
    assert_eq!(apply_fee(1_000_000, 30), Ok((997_000, 3_000)));
    // Rounds up: 0.3% of 1_000_001 = 3000.003 → 3001.
    assert_eq!(apply_fee(1_000_001, 30), Ok((997_000, 3_001)));
    // Dust: any positive amount with a tiny bps yields at least 1 unit of fee.
    assert_eq!(apply_fee(1, 1), Ok((0, 1)));
}

#[test]
fn apply_fee_rounds_up_and_sum_invariant() {
    let mut rng = Rng::new(0xFEE5);
    for _ in 0..50_000 {
        let amount = rng.range_i128(0, 1_000_000_000_000_000_000);
        let bps = rng.range_i128(0, BPS_DENOM);
        let (net, fee) = apply_fee(amount, bps).unwrap();
        // Exact sum.
        assert_eq!(net + fee, amount, "sum amount={amount} bps={bps}");
        // Non-negative.
        assert!(net >= 0 && fee >= 0);
        // Rounds up: fee == ⌈amount·bps/10000⌉.
        let exact_ceil = (amount * bps + BPS_DENOM - 1) / BPS_DENOM; // amount·bps fits? see note
                                                                     // amount up to 1e18, bps up to 1e4 → product up to 1e22 < i128::MAX. OK.
        assert_eq!(fee, exact_ceil, "ceil amount={amount} bps={bps}");
    }
}

#[test]
fn apply_fee_errors() {
    assert_eq!(apply_fee(-1, 30), Err(FeeError::InvalidAmount));
    assert_eq!(apply_fee(100, -1), Err(FeeError::InvalidBps));
    assert_eq!(apply_fee(100, BPS_DENOM + 1), Err(FeeError::InvalidBps));
}

// ---------------------------------------------------------------- split_protocol_fee

#[test]
fn split_zero_and_full() {
    // 0% protocol → all to LPs.
    assert_eq!(split_protocol_fee(1_000, 0), Ok((1_000, 0)));
    // 100% protocol → all to protocol.
    assert_eq!(split_protocol_fee(1_000, BPS_DENOM), Ok((0, 1_000)));
}

#[test]
fn split_rounds_down_to_lp_and_sum_invariant() {
    let mut rng = Rng::new(0x5F11);
    for _ in 0..50_000 {
        let fee = rng.range_i128(0, 1_000_000_000_000_000_000);
        let bps = rng.range_i128(0, BPS_DENOM);
        let (lp, protocol) = split_protocol_fee(fee, bps).unwrap();
        assert_eq!(lp + protocol, fee, "sum fee={fee} bps={bps}");
        assert!(lp >= 0 && protocol >= 0);
        // Protocol rounds down: dust stays with LPs.
        let exact_floor = (fee * bps) / BPS_DENOM;
        assert_eq!(protocol, exact_floor, "floor fee={fee} bps={bps}");
    }
}

#[test]
fn split_errors() {
    assert_eq!(split_protocol_fee(-1, 30), Err(FeeError::InvalidAmount));
    assert_eq!(split_protocol_fee(100, -1), Err(FeeError::InvalidBps));
    assert_eq!(
        split_protocol_fee(100, BPS_DENOM + 1),
        Err(FeeError::InvalidBps)
    );
}
