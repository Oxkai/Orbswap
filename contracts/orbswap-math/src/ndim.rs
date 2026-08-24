//! N-dimensional invariant — the paper's headline claim (title, contribution #5).
//!
//! # Derivation (not printed in the paper; anchored on Orbital [5])
//! Generalizing the 2-token CSEMM (Eq. 2) to `n` tokens:
//! ```text
//! Σᵢ |xᵢ/αᵢ − 1|^u(αᵢ) = 1
//! ```
//! This is the correct generalization because it reduces to **both** anchors:
//! - `n = 2` → exactly [`crate::csemm`] (the sum is the two CSEMM terms).
//! - `u = 2`, all `αᵢ = k` → `Σ(xᵢ/k − 1)² = 1` ⇒ `Σ(xᵢ − k)² = k²`, which is
//!   **Orbital's n-sphere** `Σ(xᵢ − r)² = r²` (radius `r = k`).
//!
//! Solving the invariant for one output reserve (all others fixed) gives, on the
//! lower arc `x_out < α_out`:
//! ```text
//! x_out = α_out · (1 − (1 − S)^{1/u(α_out)}),   S = Σ_{j≠out} (1 − xⱼ/αⱼ)^{u(αⱼ)}
//! ```
//! the direct n-dim analogue of `csemm::partner`. The output is solved **exactly**
//! from the invariant — never the linearized `ΔI/Δx` shortcut the reference impl
//! [7] uses (see `docs/INVARIANT_MATH.md` appendix).
//!
//! Reuses [`crate::csemm::u`] and [`CsemmError`]; all values are WAD.

use crate::csemm::{u, CsemmError};
use crate::fixed_point::{mul_div, pow_fixed, Rounding, FIXED_SCALE};

/// `(1 − xⱼ/αⱼ)^{u(αⱼ)}` for a reserve strictly on the arc (`0 ≤ x ≤ α`).
#[inline]
fn arc_term(x: i128, alpha: i128, u_a: i128) -> Result<i128, CsemmError> {
    if x < 0 || x > alpha {
        return Err(CsemmError::OutOfRange);
    }
    let ratio = mul_div(x, FIXED_SCALE, alpha, Rounding::Down)?; // x/α ∈ [0,1]
    let base = FIXED_SCALE - ratio; // 1 − x/α ≥ 0
    Ok(pow_fixed(base, u_a)?)
}

/// `|xⱼ/αⱼ − 1|^{u(αⱼ)}` (magnitude form, valid off-arc too) — for the checker.
#[inline]
fn mag_term(x: i128, alpha: i128, u_a: i128) -> Result<i128, CsemmError> {
    if x < 0 || alpha <= 0 {
        return Err(CsemmError::OutOfRange);
    }
    let ratio = mul_div(x, FIXED_SCALE, alpha, Rounding::Down)?;
    let base = (ratio - FIXED_SCALE).abs();
    Ok(pow_fixed(base, u_a)?)
}

/// Validate the parallel `reserves`/`params` slices describe an `n ≥ 2` pool with
/// distinct in/out indices.
#[inline]
fn check_shape(
    reserves: &[i128],
    params: &[i128],
    i_in: usize,
    i_out: usize,
) -> Result<usize, CsemmError> {
    let n = reserves.len();
    if n < 2 || params.len() != n {
        return Err(CsemmError::OutOfRange);
    }
    if i_in >= n || i_out >= n || i_in == i_out {
        return Err(CsemmError::OutOfRange);
    }
    Ok(n)
}

/// Swap `amount_in` of token `i_in` for token `i_out` in an n-token pool; all
/// other reserves are held fixed.
///
/// Returns `(amount_out, new_x_in, new_x_out)`. `reserves[j]` for `j ∉ {i_in,
/// i_out}` are unchanged. Output rounds down (favors pool). All WAD.
///
/// Errors: `OutOfRange` (bad shape/indices/reserves, or an infeasible trade where
/// `S > 1`), `InvalidAmount` (`amount_in ≤ 0`), `PriceOutOfRange`
/// (`x_in + amount_in > α_in`), `DomainError`/`Overflow` from the math.
pub fn swap_out_n(
    reserves: &[i128],
    params: &[i128],
    i_in: usize,
    i_out: usize,
    amount_in: i128,
) -> Result<(i128, i128, i128), CsemmError> {
    let n = check_shape(reserves, params, i_in, i_out)?;
    if amount_in <= 0 {
        return Err(CsemmError::InvalidAmount);
    }
    let alpha_in = params[i_in];
    let new_x_in = reserves[i_in]
        .checked_add(amount_in)
        .ok_or(CsemmError::Overflow)?;
    // u(alpha_in) also validates alpha_in ≥ 2.
    let _ = u(alpha_in)?;
    if new_x_in > alpha_in {
        return Err(CsemmError::PriceOutOfRange);
    }

    // S = Σ_{j≠out} (1 − xⱼ/αⱼ)^{u(αⱼ)}, using the post-trade x_in.
    let mut s: i128 = 0;
    for j in 0..n {
        if j == i_out {
            continue;
        }
        let xj = if j == i_in { new_x_in } else { reserves[j] };
        let uj = u(params[j])?;
        s = s
            .checked_add(arc_term(xj, params[j], uj)?)
            .ok_or(CsemmError::Overflow)?;
    }
    if s > FIXED_SCALE {
        // 1 − S < 0: the trade would push x_out off the arc — infeasible.
        return Err(CsemmError::OutOfRange);
    }

    let inner = FIXED_SCALE - s; // 1 − S ≥ 0
    let alpha_out = params[i_out];
    let u_out = u(alpha_out)?;
    let inv_u_out = mul_div(FIXED_SCALE, FIXED_SCALE, u_out, Rounding::Down)?;
    let outer = pow_fixed(inner, inv_u_out)?; // (1 − S)^{1/u_out}
                                              // x_out = α_out·(1 − outer), rounded up (pool-favoring).
    let mut new_x_out = mul_div(alpha_out, FIXED_SCALE - outer, FIXED_SCALE, Rounding::Up)?;

    let x_out_old = reserves[i_out];
    let amount_out = if new_x_out >= x_out_old {
        new_x_out = x_out_old;
        0
    } else {
        x_out_old - new_x_out
    };
    Ok((amount_out, new_x_in, new_x_out))
}

/// Signed invariant residual `Σᵢ |xᵢ/αᵢ − 1|^u(αᵢ) − 1` (WAD).
///
/// Zero exactly on the curve, negative inside it, positive outside. Uses the
/// magnitude form, so it is defined off-arc too.
///
/// **Monotonicity.** Each term `|x/α − 1|^u` decreases as `x` rises from `0` to
/// `α`. So on the arc (`0 ≤ xᵢ ≤ αᵢ`) the residual is strictly decreasing in every
/// `xᵢ` — which is what lets a caller solve for the liquidity scale by bisection.
/// Past `x > α` the magnitude turns and the property is lost, so a solver must
/// keep its bracket on the arc.
pub fn invariant_residual_n(reserves: &[i128], params: &[i128]) -> Result<i128, CsemmError> {
    let n = reserves.len();
    if n < 2 || params.len() != n {
        return Err(CsemmError::OutOfRange);
    }
    let mut s: i128 = 0;
    for j in 0..n {
        let uj = u(params[j])?;
        s = s
            .checked_add(mag_term(reserves[j], params[j], uj)?)
            .ok_or(CsemmError::Overflow)?;
    }
    Ok(s - FIXED_SCALE)
}

/// Whether `reserves` satisfy the n-dim invariant within `epsilon` (WAD):
/// `| Σᵢ |xᵢ/αᵢ − 1|^u(αᵢ) − 1 | ≤ epsilon`. `false` on any error.
pub fn invariant_holds_n(reserves: &[i128], params: &[i128], epsilon: i128) -> bool {
    match invariant_residual_n(reserves, params) {
        Ok(r) => r.unsigned_abs() <= epsilon.unsigned_abs(),
        Err(_) => false,
    }
}
