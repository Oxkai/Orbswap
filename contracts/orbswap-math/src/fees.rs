//! Swap-fee math (basis points).
//!
//! The swap fee is taken from the input and **rounds up**, so any rounding dust
//! stays with the pool (never the trader). The protocol's cut of that fee rounds
//! **down**, so the dust in the split stays with the LPs. Both operations preserve
//! their sum exactly.

use crate::fixed_point::{mul_div, MathError, Rounding};

/// Basis-points denominator (100% = 10000 bps).
pub const BPS_DENOM: i128 = 10_000;

/// Errors from fee operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeeError {
    /// A negative amount.
    InvalidAmount,
    /// A bps value outside `[0, 10000]`.
    InvalidBps,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for FeeError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => FeeError::Overflow,
            MathError::DomainError | MathError::NegativeInput | MathError::DivByZero => {
                FeeError::InvalidAmount
            }
        }
    }
}

/// Split `amount_in` into `(amount_in_less_fee, fee_amount)` for a `fee_bps` fee.
///
/// `fee_amount = ⌈amount_in · fee_bps / 10000⌉` (rounds up, favoring the pool);
/// `amount_in_less_fee = amount_in − fee_amount ≥ 0`. Their sum is exactly
/// `amount_in`.
pub fn apply_fee(amount_in: i128, fee_bps: i128) -> Result<(i128, i128), FeeError> {
    if amount_in < 0 {
        return Err(FeeError::InvalidAmount);
    }
    if !(0..=BPS_DENOM).contains(&fee_bps) {
        return Err(FeeError::InvalidBps);
    }
    let fee = mul_div(amount_in, fee_bps, BPS_DENOM, Rounding::Up)?;
    // fee ≤ amount_in because fee_bps ≤ 10000, so this cannot underflow.
    let net = amount_in - fee;
    Ok((net, fee))
}

/// Split a collected `fee_amount` into `(lp_fee, protocol_fee)` for a
/// `protocol_bps` protocol cut.
///
/// `protocol_fee = ⌊fee_amount · protocol_bps / 10000⌋` (rounds down, so the dust
/// goes to LPs); `lp_fee = fee_amount − protocol_fee`. Their sum is exactly
/// `fee_amount`.
pub fn split_protocol_fee(fee_amount: i128, protocol_bps: i128) -> Result<(i128, i128), FeeError> {
    if fee_amount < 0 {
        return Err(FeeError::InvalidAmount);
    }
    if !(0..=BPS_DENOM).contains(&protocol_bps) {
        return Err(FeeError::InvalidBps);
    }
    let protocol = mul_div(fee_amount, protocol_bps, BPS_DENOM, Rounding::Down)?;
    let lp = fee_amount - protocol;
    Ok((lp, protocol))
}
