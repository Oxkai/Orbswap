//! TWAP (time-weighted average price) accumulator math — pure.
//!
//! A cumulative price `Σ price·Δt` is maintained off this module's `accumulate`;
//! a TWAP over `[t0, t1]` is `(cumulative1 − cumulative0) / (t1 − t0)`. Following
//! Uniswap v2, the accumulator uses **wrapping** arithmetic so it keeps working
//! after the cumulative overflows `i128` — the difference between two snapshots is
//! still correct as long as it fits (which it does for any realistic window).
//!
//! `price` is WAD-scaled (from `ccmm`/`csemm::spot_price`); `elapsed`/window are
//! in whatever time unit the caller uses (e.g. ledger seconds), consistently.

use crate::fixed_point::{mul_div, MathError, Rounding};

/// Errors from oracle operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleError {
    /// A negative price or elapsed time.
    InvalidInput,
    /// A zero-length window (would divide by zero).
    ZeroWindow,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for OracleError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => OracleError::Overflow,
            MathError::DivByZero => OracleError::ZeroWindow,
            MathError::DomainError | MathError::NegativeInput => OracleError::InvalidInput,
        }
    }
}

/// Advance the cumulative price by `price · elapsed`, wrapping on overflow.
///
/// Wrapping is intentional (v2-style): the running total may exceed `i128`, but a
/// later [`twap`] recovers the correct average from the wrapped difference.
pub fn accumulate(prev_cumulative: i128, price: i128, elapsed: i128) -> Result<i128, OracleError> {
    if price < 0 || elapsed < 0 {
        return Err(OracleError::InvalidInput);
    }
    let delta = price.checked_mul(elapsed).ok_or(OracleError::Overflow)?;
    Ok(prev_cumulative.wrapping_add(delta))
}

/// TWAP over a window: `(cumulative_end ⊖ cumulative_start) / elapsed`, WAD.
///
/// The subtraction wraps (mirroring [`accumulate`]), so this is correct across an
/// accumulator overflow. `elapsed` must be `> 0`. Rounds down.
pub fn twap(
    cumulative_start: i128,
    cumulative_end: i128,
    elapsed: i128,
) -> Result<i128, OracleError> {
    if elapsed <= 0 {
        return Err(OracleError::ZeroWindow);
    }
    let diff = cumulative_end.wrapping_sub(cumulative_start);
    Ok(mul_div(diff, 1, elapsed, Rounding::Down)?)
}
