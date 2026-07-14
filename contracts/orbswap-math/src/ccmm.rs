//! Concentrated Circular Market Maker (CCMM) — the `u = 2` special case of the
//! Orbswap superellipse. See `docs/INVARIANT_MATH.md` §1–2.
//!
//! Invariant (paper Eq. 1):
//! ```text
//! (x − k)² + (y − k)² = k²
//! ```
//! A circle centered at `(k, k)` with radius `k`. Reserves live on the lower-left
//! arc from `(0, k)` to `(k, 0)`; the balanced (price = 1) point is
//! `x = y = k(1 − 1/√2)`. The region past `x = k` is the negative-price fold and
//! is rejected for stablecoin use.
//!
//! # Units
//! `x`, `y`, `k` are in the caller's chosen integer unit; they must all share it.
//! The swap math is **homogeneous** (scaling `x, y, k` by any λ preserves the
//! invariant), so the unit cancels and no fixed-point scaling is applied here.
//! Only [`spot_price`] returns a WAD-scaled (1e18) value, since a price is a ratio.
//!
//! # Rounding
//! Both swap directions round via [`isqrt`] (floor). In `swap_out` that makes the
//! output an under-estimate; in `swap_in` it makes the required input an
//! over-estimate — both **favor the pool** (never the trader).

use crate::fixed_point::{isqrt, mul_div, MathError, Rounding, FIXED_SCALE};

/// Errors from CCMM operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcmmError {
    /// A swap amount was zero or negative.
    InvalidAmount,
    /// A reserve/offset was negative, off the usable arc, or a swap asked for
    /// more than the pool holds.
    OutOfRange,
    /// The swap would push a reserve past `k` into the negative-price fold.
    PriceOutOfRange,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for CcmmError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => CcmmError::Overflow,
            MathError::NegativeInput | MathError::DomainError | MathError::DivByZero => {
                CcmmError::OutOfRange
            }
        }
    }
}

/// `a · (2k − a)` (the radicand of `√(2ka − a²)`), overflow-checked.
///
/// For `0 ≤ a ≤ k` the result is non-negative (`2k − a ≥ k ≥ 0`). Callers
/// guarantee that domain; this only guards against `i128` overflow.
#[inline]
fn radicand(a: i128, k: i128) -> Result<i128, CcmmError> {
    let two_k = k.checked_mul(2).ok_or(CcmmError::Overflow)?;
    let inner = two_k.checked_sub(a).ok_or(CcmmError::Overflow)?;
    a.checked_mul(inner).ok_or(CcmmError::Overflow)
}

/// The lower-branch reserve paired with `a` on the circle: `k − √(2ka − a²)`
/// (paper Eq. 5). `√` floors, so the result is an over-estimate of the true
/// paired reserve — the pool-favoring direction for both swap flows.
#[inline]
fn paired_reserve(a: i128, k: i128) -> Result<i128, CcmmError> {
    let root = isqrt(radicand(a, k)?);
    // k ≥ root ≥ 0 since radicand ≤ k²; subtraction cannot go negative, but stay
    // defensive against a caller passing an off-domain `a`.
    k.checked_sub(root).ok_or(CcmmError::Overflow)
}

/// Validate that `(x, y, k)` describe a usable pool state on the arc.
#[inline]
fn check_state(x: i128, y: i128, k: i128) -> Result<(), CcmmError> {
    if k <= 0 {
        return Err(CcmmError::OutOfRange);
    }
    if x < 0 || y < 0 || x > k || y > k {
        return Err(CcmmError::OutOfRange);
    }
    Ok(())
}

/// Swap `amount_in` of the input token into the pool, receiving the output token.
///
/// Given input reserve `x`, output reserve `y`, and radius `k`, returns
/// `(amount_out, new_x, new_y)` where `new_x = x + amount_in` and
/// `(new_x, new_y)` lies on the circle. Output is rounded **down** (favors pool).
///
/// Errors: `InvalidAmount` (`amount_in ≤ 0`), `OutOfRange` (bad state or
/// off-curve `y`), `PriceOutOfRange` (`new_x > k`, the negative-price fold),
/// `Overflow`.
pub fn swap_out(
    x: i128,
    y: i128,
    k: i128,
    amount_in: i128,
) -> Result<(i128, i128, i128), CcmmError> {
    check_state(x, y, k)?;
    if amount_in <= 0 {
        return Err(CcmmError::InvalidAmount);
    }
    let new_x = x.checked_add(amount_in).ok_or(CcmmError::Overflow)?;
    if new_x > k {
        return Err(CcmmError::PriceOutOfRange);
    }
    let new_y = paired_reserve(new_x, k)?;
    // On-curve pools always have new_y ≤ y here; guard against off-curve input.
    let amount_out = y.checked_sub(new_y).ok_or(CcmmError::Overflow)?;
    if amount_out < 0 {
        return Err(CcmmError::OutOfRange);
    }
    Ok((amount_out, new_x, new_y))
}

/// Swap to receive exactly `amount_out` of the output token, computing the
/// required input.
///
/// Returns `(amount_in, new_x, new_y)` where `new_y = y − amount_out` and
/// `(new_x, new_y)` lies on the circle. Required input is rounded **up** (favors
/// pool) as a consequence of `√` flooring.
///
/// Errors: `InvalidAmount` (`amount_out ≤ 0`), `OutOfRange` (bad state, or
/// `amount_out > y` so `new_y < 0`), `Overflow`.
pub fn swap_in(
    x: i128,
    y: i128,
    k: i128,
    amount_out: i128,
) -> Result<(i128, i128, i128), CcmmError> {
    check_state(x, y, k)?;
    if amount_out <= 0 {
        return Err(CcmmError::InvalidAmount);
    }
    let new_y = y.checked_sub(amount_out).ok_or(CcmmError::Overflow)?;
    if new_y < 0 {
        return Err(CcmmError::OutOfRange);
    }
    // Circle is symmetric in x,y: new_x = k − √(2k·new_y − new_y²).
    let new_x = paired_reserve(new_y, k)?;
    let amount_in = new_x.checked_sub(x).ok_or(CcmmError::Overflow)?;
    if amount_in < 0 {
        return Err(CcmmError::OutOfRange);
    }
    Ok((amount_in, new_x, new_y))
}

/// Whether `(x, y)` satisfies the invariant within `epsilon` (in unit²):
/// `|(x−k)² + (y−k)² − k²| ≤ epsilon`.
///
/// After a swap the integer-`√` truncation leaves a residual of at most ~`2k`,
/// so callers checking post-swap state should budget `epsilon ≈ 2k`. Returns
/// `false` if an intermediate overflows `i128` (cannot be verified).
pub fn invariant_holds(x: i128, y: i128, k: i128, epsilon: i128) -> bool {
    let residual = || -> Option<i128> {
        let dx = x.checked_sub(k)?;
        let dy = y.checked_sub(k)?;
        let dx2 = dx.checked_mul(dx)?;
        let dy2 = dy.checked_mul(dy)?;
        let k2 = k.checked_mul(k)?;
        dx2.checked_add(dy2)?.checked_sub(k2)
    };
    match residual() {
        Some(r) => r.unsigned_abs() <= epsilon.unsigned_abs(),
        None => false,
    }
}

/// Marginal price of the input token in units of the output token, WAD-scaled
/// (`1e18` = price 1.0): `p = (x − k) / (y − k)`.
///
/// On the usable arc both differences are ≤ 0, so `p ≥ 0`; `p = 1` at the
/// balanced point. Saturates to `i128::MAX` (as `+∞`) at the `x = 0` boundary
/// (`y = k`) or if the ratio overflows near it.
pub fn spot_price(x: i128, y: i128, k: i128) -> i128 {
    let den = y - k;
    if den == 0 {
        return i128::MAX; // x = 0 boundary: price → +∞
    }
    let num = x - k;
    match mul_div(num, FIXED_SCALE, den, Rounding::Down) {
        Ok(p) => p,
        Err(_) => i128::MAX,
    }
}
