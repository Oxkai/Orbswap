//! Concentrated Super-Elliptical Market Maker (CSEMM) — the general Orbswap curve.
//! See `docs/INVARIANT_MATH.md` §3–5.
//!
//! Invariant (paper Eq. 2), written with magnitudes so it is real-valued on the
//! usable arc:
//! ```text
//! |x/α − 1|^u(α) + |y/β − 1|^u(β) = 1,   u(x) = ln2 / ln(x/(x−1))
//! ```
//! `α, β` are the shape parameters (≥ 2); they also set the reserve extents
//! (`x ∈ [0, α]`, `y ∈ [0, β]`). At `α = β = 2+√2`, `u = 2` and this reduces to
//! the [`crate::ccmm`] circle (the "ladder"); as `α,β → 2` it becomes constant-sum.
//!
//! # Units
//! Unlike CCMM, this curve is **not** homogeneous (`u` depends on `α`), so all
//! arguments are **WAD fixed-point** (`FIXED_SCALE` = 1e18): reserves live in the
//! normalized space where `α, β` are O(1) and `x = S·x̂` is de-normalized by the
//! contract (todo.md Architecture §A).
//!
//! # Negative base (critical)
//! On the lower arc `x < α`, `x/α − 1 < 0`; a non-integer power of a negative base
//! is undefined in the reals. We therefore always feed [`pow_fixed`] the
//! **magnitude** `1 − x/α ≥ 0`. `pow_fixed` itself rejects a negative base with
//! `DomainError`, so a bug here fails loudly rather than silently.
//!
//! # Rounding / precision
//! The partner reserve is rounded so the pool is favored (output down / input up).
//! Because `ln`/`exp`/`pow` are approximate (~1e-13 relative), results are accurate
//! but not exact to the ULP; the contract layer adds a fee/epsilon margin. Tests
//! assert accuracy against an f64 oracle and the exact ccmm ladder.

use crate::fixed_point::{ln_fixed, mul_div, pow_fixed, MathError, Rounding, FIXED_SCALE, LN2};

/// Minimum shape parameter: `α, β ≥ 2` (`u(2) = 1`). Below 2, `u < 1` gives a
/// concave, star-shaped curve unusable as a pool.
pub const MIN_SHAPE: i128 = 2 * FIXED_SCALE;

/// Errors from CSEMM operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CsemmError {
    /// A swap amount was zero or negative.
    InvalidAmount,
    /// A reserve was negative or beyond its extent (`x > α` / `y > β`), or a swap
    /// asked for more than the pool holds.
    OutOfRange,
    /// A swap would push a reserve past its extent into the fold.
    PriceOutOfRange,
    /// A shape parameter or reserve was outside the mathematical domain
    /// (`α, β < 2`, base ≤ 0 into `pow`, etc.).
    DomainError,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for CsemmError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => CsemmError::Overflow,
            MathError::DomainError | MathError::NegativeInput | MathError::DivByZero => {
                CsemmError::DomainError
            }
        }
    }
}

/// The exponent function `u(x) = ln2 / ln(x/(x−1))` in WAD.
///
/// Domain: `x ≥ 2` (WAD). `u(2) = 1` exactly, `u(2+√2) = 2`, and `u → ∞` as
/// `x → ∞`. Returns `DomainError` for `x < 2` and `Overflow` for astronomically
/// large `x` (where `1/ln(x/(x−1))` overflows).
pub fn u(x: i128) -> Result<i128, CsemmError> {
    if x < MIN_SHAPE {
        return Err(CsemmError::DomainError);
    }
    let denom = x - FIXED_SCALE; // x − 1 > 0
                                 // x/(x−1) > 1.
    let ratio = mul_div(x, FIXED_SCALE, denom, Rounding::Down)?;
    let ln_r = ln_fixed(ratio)?; // > 0 for ratio > 1
    if ln_r <= 0 {
        return Err(CsemmError::Overflow);
    }
    Ok(mul_div(LN2, FIXED_SCALE, ln_r, Rounding::Down)?)
}

/// `1 − a/A` (the magnitude `|a/A − 1|` on the arc), guarding `0 ≤ a ≤ A`.
#[inline]
fn one_minus_ratio(a: i128, cap: i128) -> Result<i128, CsemmError> {
    if a < 0 || a > cap {
        return Err(CsemmError::OutOfRange);
    }
    let ratio = mul_div(a, FIXED_SCALE, cap, Rounding::Down)?; // a/A ∈ [0,1]
    Ok(FIXED_SCALE - ratio)
}

/// Partner reserve on the curve: `B·[1 − (1 − (1 − a/A)^{u_a})^{inv_u_b}]`.
///
/// `u_a = u(A)` and `inv_u_b = 1/u(B)`, both WAD. With the paper's Eq. 7 this is
/// `y_of_x` (`A=α, B=β`); with Eq. 9's reversed order it is `x_of_y` (`A=β, B=α`,
/// `u`'s swapped). Rounded **up** so the pool is favored.
fn partner(
    a: i128,
    cap_a: i128,
    scale_b: i128,
    u_a: i128,
    inv_u_b: i128,
) -> Result<i128, CsemmError> {
    let base = one_minus_ratio(a, cap_a)?; // 1 − a/A ∈ [0,1]
    let term = pow_fixed(base, u_a)?; // (1 − a/A)^{u_a} ∈ [0,1]
    let inner = FIXED_SCALE - term; // 1 − term ≥ 0 (outer-root base)
    let outer = pow_fixed(inner, inv_u_b)?; // ∈ [0,1]
    let one_minus_outer = FIXED_SCALE - outer;
    // B·(1 − outer), rounded up (pool-favoring).
    Ok(mul_div(
        scale_b,
        one_minus_outer,
        FIXED_SCALE,
        Rounding::Up,
    )?)
}

/// Validate shape params and reserve ranges; returns `(u(α), u(β))`.
fn shapes(x: i128, y: i128, alpha: i128, beta: i128) -> Result<(i128, i128), CsemmError> {
    let ua = u(alpha)?;
    let ub = u(beta)?;
    if x < 0 || x > alpha || y < 0 || y > beta {
        return Err(CsemmError::OutOfRange);
    }
    Ok((ua, ub))
}

#[inline]
fn inverse(u_val: i128) -> Result<i128, CsemmError> {
    // 1/u in WAD; u ≥ 1·WAD so the result ≤ 1·WAD.
    Ok(mul_div(FIXED_SCALE, FIXED_SCALE, u_val, Rounding::Down)?)
}

/// Swap `amount_in` of the input token in, receiving the output token (Eq. 8).
///
/// Returns `(amount_out, new_x, new_y)` with `new_x = x + amount_in` on the curve.
/// Output rounds down (favors pool). All values WAD.
pub fn swap_out(
    x: i128,
    y: i128,
    alpha: i128,
    beta: i128,
    amount_in: i128,
) -> Result<(i128, i128, i128), CsemmError> {
    let (ua, ub) = shapes(x, y, alpha, beta)?;
    if amount_in <= 0 {
        return Err(CsemmError::InvalidAmount);
    }
    let new_x = x.checked_add(amount_in).ok_or(CsemmError::Overflow)?;
    if new_x > alpha {
        return Err(CsemmError::PriceOutOfRange);
    }
    let new_y = partner(new_x, alpha, beta, ua, inverse(ub)?)?;
    // Rounding-up of the partner can, for dust inputs, put new_y a hair above y;
    // clamp so amount_out ≥ 0 (the contract rejects the resulting 0-output dust).
    let (new_y, amount_out) = if new_y >= y {
        (y, 0)
    } else {
        (new_y, y - new_y)
    };
    Ok((amount_out, new_x, new_y))
}

/// Swap to receive exactly `amount_out`, computing the required input (Eq. 9).
///
/// The `u`-order is **reversed** vs [`swap_out`]: solving Eq. 2 for `x` swaps the
/// roles `α↔β` and `u(α)↔u(β)`. Returns `(amount_in, new_x, new_y)` with
/// `new_y = y − amount_out`. Input rounds up (favors pool). All values WAD.
pub fn swap_in(
    x: i128,
    y: i128,
    alpha: i128,
    beta: i128,
    amount_out: i128,
) -> Result<(i128, i128, i128), CsemmError> {
    let (ua, ub) = shapes(x, y, alpha, beta)?;
    if amount_out <= 0 {
        return Err(CsemmError::InvalidAmount);
    }
    let new_y = y.checked_sub(amount_out).ok_or(CsemmError::Overflow)?;
    if new_y < 0 {
        return Err(CsemmError::OutOfRange);
    }
    // x_of_y: A=β, B=α, exponents reversed (u(β) inside, 1/u(α) outside).
    let new_x = partner(new_y, beta, alpha, ub, inverse(ua)?)?;
    let amount_in = new_x.checked_sub(x).ok_or(CsemmError::Overflow)?;
    if amount_in < 0 {
        return Err(CsemmError::OutOfRange);
    }
    Ok((amount_in, new_x, new_y))
}

/// Whether `(x, y)` satisfies the invariant within `epsilon` (WAD):
/// `| |x/α−1|^u(α) + |y/β−1|^u(β) − 1 | ≤ epsilon`.
///
/// Uses magnitudes, so it is valid on both arcs. Returns `false` on any domain
/// or overflow error. Because of transcendental rounding, on-curve points hold
/// only to ~1e-13 relative — budget `epsilon` accordingly (e.g. `1e8` WAD).
pub fn invariant_holds(x: i128, y: i128, alpha: i128, beta: i128, epsilon: i128) -> bool {
    let residual = || -> Result<i128, CsemmError> {
        let ua = u(alpha)?;
        let ub = u(beta)?;
        let tx = pow_fixed(mag_ratio(x, alpha)?, ua)?;
        let ty = pow_fixed(mag_ratio(y, beta)?, ub)?;
        Ok((tx + ty) - FIXED_SCALE)
    };
    match residual() {
        Ok(r) => r.unsigned_abs() <= epsilon.unsigned_abs(),
        Err(_) => false,
    }
}

/// `|a/A − 1|` in WAD (works off-arc too, unlike [`one_minus_ratio`]).
#[inline]
fn mag_ratio(a: i128, cap: i128) -> Result<i128, CsemmError> {
    if a < 0 || cap <= 0 {
        return Err(CsemmError::OutOfRange);
    }
    let ratio = mul_div(a, FIXED_SCALE, cap, Rounding::Down)?;
    Ok((ratio - FIXED_SCALE).abs())
}

/// Marginal price `p = −dy/dx` of the input token, WAD-scaled.
///
/// From Eq. 2: `p = [(u(α)/α)(1−x/α)^{u(α)−1}] / [(u(β)/β)(1−y/β)^{u(β)−1}]`.
/// `p = 1` at the balanced point of a symmetric pool. Saturates to `i128::MAX`
/// (`+∞`) at the `y = β` boundary and returns `0` at `x = α`.
pub fn spot_price(x: i128, y: i128, alpha: i128, beta: i128) -> i128 {
    let compute = || -> Result<i128, CsemmError> {
        let ua = u(alpha)?;
        let ub = u(beta)?;
        let bx = one_minus_ratio(x, alpha)?; // 1 − x/α
        let by = one_minus_ratio(y, beta)?; // 1 − y/β
        if bx == 0 {
            return Ok(0); // x = α → price 0
        }
        if by == 0 {
            return Ok(i128::MAX); // y = β → price +∞
        }
        // termA = (u(α)/α) · (1−x/α)^{u(α)−1}
        let coef_a = mul_div(ua, FIXED_SCALE, alpha, Rounding::Down)?;
        let pa = pow_fixed(bx, ua - FIXED_SCALE)?;
        let term_a = mul_div(coef_a, pa, FIXED_SCALE, Rounding::Down)?;
        let coef_b = mul_div(ub, FIXED_SCALE, beta, Rounding::Down)?;
        let pb = pow_fixed(by, ub - FIXED_SCALE)?;
        let term_b = mul_div(coef_b, pb, FIXED_SCALE, Rounding::Down)?;
        if term_b == 0 {
            return Ok(i128::MAX);
        }
        Ok(mul_div(term_a, FIXED_SCALE, term_b, Rounding::Down)?)
    };
    compute().unwrap_or(i128::MAX)
}
