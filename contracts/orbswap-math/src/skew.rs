//! Elliptical liquidity skew (paper Eq. 3) — the Cartesian approximation used
//! because the superelliptical skew has no closed-form polar solution (§2.3).
//!
//! Ellipse: `(x/a − L)² + (y/b − L)² = L²`. Substituting `u = x/a`, `v = y/b`
//! recovers the CCMM circle `(u − L)² + (v − L)² = L²`, so the skew is exactly
//! **independent per-axis scaling**:
//! ```text
//! circle (u, v)  ──apply_skew──▶  ellipse (a·u, b·v)
//! ellipse (x, y) ──unapply_skew─▶  circle (x/a, y/b)
//! ```
//! `a = b = 1` is the identity (the plain circle, `k = L`). This is L-independent,
//! so the `l` from the planning sketch is intentionally omitted — the radius `L`
//! is shared by both the circle and its skewed ellipse and never enters the
//! transform.
//!
//! # Units
//! `a, b` are **WAD** skew factors (`FIXED_SCALE` = 1.0, i.e. no skew). Reserves
//! `u, v` / `x, y` may be in any consistent unit (the factor is applied via
//! `mul_div`). Contract flow for a skewed pool: `unapply_skew` → `ccmm` swap →
//! `apply_skew`.

use crate::fixed_point::{mul_div, MathError, Rounding, FIXED_SCALE};

/// Errors from skew operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkewError {
    /// A skew factor was `≤ 0`.
    InvalidSkew,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for SkewError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => SkewError::Overflow,
            MathError::DomainError | MathError::NegativeInput | MathError::DivByZero => {
                SkewError::InvalidSkew
            }
        }
    }
}

#[inline]
fn check(a: i128, b: i128) -> Result<(), SkewError> {
    if a <= 0 || b <= 0 {
        return Err(SkewError::InvalidSkew);
    }
    Ok(())
}

/// Map circle reserves `(u, v)` to skewed ellipse reserves `(a·u, b·v)`.
///
/// With `a = b = FIXED_SCALE` this is the identity. WAD factors `a, b`; reserves
/// in any consistent unit. Rounds down.
pub fn apply_skew(u: i128, v: i128, a: i128, b: i128) -> Result<(i128, i128), SkewError> {
    check(a, b)?;
    let x = mul_div(a, u, FIXED_SCALE, Rounding::Down)?;
    let y = mul_div(b, v, FIXED_SCALE, Rounding::Down)?;
    Ok((x, y))
}

/// Inverse of [`apply_skew`]: map ellipse reserves `(x, y)` back to the circle
/// `(x/a, y/b)` so the CCMM math can be applied. Rounds down.
pub fn unapply_skew(x: i128, y: i128, a: i128, b: i128) -> Result<(i128, i128), SkewError> {
    check(a, b)?;
    let u = mul_div(x, FIXED_SCALE, a, Rounding::Down)?;
    let v = mul_div(y, FIXED_SCALE, b, Rounding::Down)?;
    Ok((u, v))
}
