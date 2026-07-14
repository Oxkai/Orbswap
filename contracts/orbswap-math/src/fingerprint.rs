//! Liquidity fingerprint (paper Eq. 4) — read-only analytics (off the swap path).
//!
//! The liquidity density in tick space `t` (second derivative of reserves w.r.t.
//! `√price`):
//! ```text
//! L(t) = 2k · e^{3t/2} / (1 + e^{2t})^{3/2}
//! ```
//! A bell curve peaked at `t = 0` with `L(0) = 2k/2^{3/2} = k/√2`, symmetric in
//! `±t` (`L(t) = L(−t)`), decaying to 0 in both tails.
//!
//! # Stable evaluation
//! Evaluated in the algebraically-equal form
//! ```text
//! L(t) = 2k / (eᵗ + e⁻ᵗ)^{3/2}   (= 2k / (2·cosh t)^{3/2})
//! ```
//! which avoids the huge `(1 + e^{2t})^{3/2}` intermediate (that overflows near
//! `t ≈ 20` before the division brings it back down). Domain: `|t| ≲ 31` (where
//! `e^{|t|}` stays inside `exp_fixed`); the tails are effectively 0 well before
//! that. WAD `t` (may be negative) and WAD `k`.

use crate::fixed_point::{exp_fixed, mul_div, pow_fixed, MathError, Rounding, FIXED_SCALE};

/// `L(t) = 2k / (eᵗ + e⁻ᵗ)^{3/2}`, WAD (equal to `2k·e^{3t/2}/(1+e^{2t})^{3/2}`).
pub fn liquidity_at_tick(k: i128, t: i128) -> Result<i128, MathError> {
    if k < 0 {
        return Err(MathError::NegativeInput);
    }
    let e_t = exp_fixed(t)?; // eᵗ
    let e_neg_t = exp_fixed(-t)?; // e⁻ᵗ
    let base = e_t.checked_add(e_neg_t).ok_or(MathError::Overflow)?; // eᵗ + e⁻ᵗ ≥ 2
    let denom = pow_fixed(base, 3 * FIXED_SCALE / 2)?; // (eᵗ + e⁻ᵗ)^{3/2}
    if denom == 0 {
        return Err(MathError::DivByZero);
    }
    let two_k = k.checked_mul(2).ok_or(MathError::Overflow)?;
    mul_div(two_k, FIXED_SCALE, denom, Rounding::Down) // 2k / denom
}
