//! Multimodal liquidity fingerprint (paper §4 Eq. 10) — the paper's own "Further
//! Research" contribution (#6). Optional / experimental analytics.
//!
//! Polar radius as a function of angle:
//! ```text
//! r(θ) = L / (β · √(1 − ½·sin²(αθ)))
//! ```
//! `α ∈ {4, 6, 8, …}` sets the number of modes (α=4 ≈ Curve, α=6 bimodal, α=8
//! trimodal — good for CDP stablecoins). Since `1 − ½sin²(αθ) ∈ [½, 1]`, the
//! radius is **bounded** in `[L/β, √2·L/β]` (no singularities).
//!
//! # Note on `β`
//! [`r_theta`] takes `β` **explicitly**, so it is unaffected by the source-text
//! ambiguity over the `β`–`α` relation (`α/2` vs `α²`, ⚠ flagged in todo.md /
//! `INVARIANT_MATH.md`). [`preset_beta`] applies one interpretation (`β = α/2`)
//! and is documented as provisional pending the Desmos check.
//!
//! `l`, `beta`, and the result are WAD; `theta` is in integer degrees (via the
//! shared trig table). `alpha` is a small integer multiplier.

use crate::fixed_point::{isqrt, mul_div, MathError, Rounding, FIXED_SCALE};
use crate::polar::sin_deg;

/// `r(θ) = L / (β·√(1 − ½sin²(αθ)))`, WAD. `theta` in degrees, `alpha` a small
/// integer. Requires `l ≥ 0`, `beta > 0`.
pub fn r_theta(l: i128, alpha: i128, beta: i128, theta: i128) -> Result<i128, MathError> {
    if l < 0 || beta <= 0 {
        return Err(MathError::DomainError);
    }
    let s = sin_deg(alpha.wrapping_mul(theta)); // sin(αθ), WAD
    let s2 = mul_div(s, s, FIXED_SCALE, Rounding::Down)?; // sin² ∈ [0,1]
    let radicand = FIXED_SCALE - s2 / 2; // 1 − ½sin² ∈ [½, 1]
                                         // √ in WAD.
    let scaled = radicand
        .checked_mul(FIXED_SCALE)
        .ok_or(MathError::Overflow)?;
    let root = isqrt(scaled); // √(1 − ½sin²) ∈ [1/√2, 1]
    let denom = mul_div(beta, root, FIXED_SCALE, Rounding::Down)?; // β·√
    if denom == 0 {
        return Err(MathError::DivByZero);
    }
    mul_div(l, FIXED_SCALE, denom, Rounding::Down) // L / (β·√)
}

/// Provisional `β` for a preset `α` (⚠ `β = α/2`, pending Desmos confirmation of
/// the `α/2` vs `α²` reading). WAD.
pub fn preset_beta(alpha: i128) -> i128 {
    (alpha * FIXED_SCALE) / 2
}
