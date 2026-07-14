//! Polar swap function for the circular pool (CCMM only — the superellipse has no
//! closed-form polar solution, paper §2.3). See `docs/INVARIANT_MATH.md` §6.
//!
//! Rotating around a focal point at distance `L` (= liquidity), the input reserve
//! is exchanged through a trigonometric conversion (paper Appendix 3.3):
//! ```text
//! Δx = −L·√(1 − (y_in/L)²) − L·cos(angle)
//! ```
//! Verified against the paper's test vector: `L=10, y_in=6.07106781187,
//! angle=135° → −0.875`.
//!
//! # Units & angle
//! `l`, `y_in`/`x_in`, and the returned delta are **WAD** (`FIXED_SCALE`). `angle`
//! is in **integer degrees**; cos/sin come from a compile-time WAD table generated
//! by `build.rs` (the paper discretizes ticks into degrees). Angles are reduced
//! mod 360, so any `i128` is valid.
//!
//! # Tick boundary
//! The radicand `1 − (y_in/L)²` requires `|y_in| ≤ L`. Exceeding it means the swap
//! has consumed the whole tick, so we return [`PolarError::TickBoundary`] rather
//! than producing a NaN-equivalent — the signal for `ticks.rs` to cross.

use crate::fixed_point::{isqrt, mul_div, MathError, Rounding, FIXED_SCALE};

// COS_WAD / SIN_WAD : [i128; 360], cos/sin(θ°)·1e18.
include!(concat!(env!("OUT_DIR"), "/trig_table.rs"));

/// Errors from polar operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolarError {
    /// `l ≤ 0`.
    InvalidLiquidity,
    /// `|input| > L`: the swap has run past the tick edge (radicand < 0).
    TickBoundary,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for PolarError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => PolarError::Overflow,
            MathError::DomainError | MathError::NegativeInput | MathError::DivByZero => {
                PolarError::TickBoundary
            }
        }
    }
}

/// `cos(angle°)` in WAD, angle reduced mod 360.
pub fn cos_deg(angle: i128) -> i128 {
    COS_WAD[normalize_deg(angle)]
}

/// `sin(angle°)` in WAD, angle reduced mod 360.
pub fn sin_deg(angle: i128) -> i128 {
    SIN_WAD[normalize_deg(angle)]
}

#[inline]
fn normalize_deg(angle: i128) -> usize {
    // Euclidean modulo keeps the result in 0..360 for negative angles too.
    angle.rem_euclid(360) as usize
}

/// The radial part `−L·√(1 − (input/L)²)`, shared by both axes.
///
/// `TickBoundary` if `|input| > L`. WAD in/out.
fn radial(l: i128, input: i128) -> Result<i128, PolarError> {
    if l <= 0 {
        return Err(PolarError::InvalidLiquidity);
    }
    // ratio = input/L (WAD); |ratio| ≤ 1 required.
    let ratio = mul_div(input, FIXED_SCALE, l, Rounding::Down)?;
    if ratio.abs() > FIXED_SCALE {
        return Err(PolarError::TickBoundary);
    }
    let ratio2 = mul_div(ratio, ratio, FIXED_SCALE, Rounding::Down)?; // (input/L)² ∈ [0,1]
    let radicand = FIXED_SCALE - ratio2; // 1 − (input/L)² ∈ [0,1]
                                         // √ in WAD: √(radicand/1e18)·1e18 = √(radicand·1e18). radicand·1e18 ≤ 1e36.
    let scaled = radicand
        .checked_mul(FIXED_SCALE)
        .ok_or(PolarError::Overflow)?;
    let root = isqrt(scaled); // WAD √(1 − (input/L)²)
                              // −L·root/1e18.
    let l_root = mul_div(l, root, FIXED_SCALE, Rounding::Down)?;
    Ok(-l_root)
}

/// `Δx = −L·√(1 − (y_in/L)²) − L·cos(angle)` (paper Appendix 3.3), WAD.
pub fn get_delta_x(l: i128, y_in: i128, angle: i128) -> Result<i128, PolarError> {
    let rad = radial(l, y_in)?;
    let l_cos = mul_div(l, cos_deg(angle), FIXED_SCALE, Rounding::Down)?;
    rad.checked_sub(l_cos).ok_or(PolarError::Overflow)
}

/// `Δy = −L·√(1 − (x_in/L)²) − L·sin(angle)`, the axis-mirror of [`get_delta_x`]
/// (`sin(θ) = cos(90°−θ)`), WAD.
pub fn get_delta_y(l: i128, x_in: i128, angle: i128) -> Result<i128, PolarError> {
    let rad = radial(l, x_in)?;
    let l_sin = mul_div(l, sin_deg(angle), FIXED_SCALE, Rounding::Down)?;
    rad.checked_sub(l_sin).ok_or(PolarError::Overflow)
}
