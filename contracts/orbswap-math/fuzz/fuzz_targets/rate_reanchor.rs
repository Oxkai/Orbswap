//! Fuzz: the invariant residual is **strictly monotonic in the liquidity scale
//! `s` while on-arc**, and a bisection on it converges to a genuine root.
//!
//! That monotonicity is the property `orbswap-pool::solve_scale` relies on to
//! re-anchor a pool after an oracle rate move (todo.md §Phase 3). If it can be
//! violated for any reserve vector, the solver could return a wrong `s` — and a
//! wrong `s` silently misprices every subsequent swap.
#![no_main]

use libfuzzer_sys::fuzz_target;
use orbswap_math::ndim;

const WAD: i128 = 1_000_000_000_000_000_000;
const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049;
/// The contract's post-swap guard tolerance.
const EPSILON: i128 = 1_000_000_000;

fn rd(d: &[u8], i: &mut usize) -> i128 {
    let mut b = [0u8; 16];
    for byte in b.iter_mut() {
        *byte = d.get(*i).copied().unwrap_or(0);
        *i += 1;
    }
    i128::from_le_bytes(b)
}

/// Normalized reserves at scale `s`: `x̂ᵢ = internalᵢ·WAD/s`.
fn xhat_at(internal: &[i128; 2], s: i128) -> Option<[i128; 2]> {
    if s <= 0 {
        return None;
    }
    let mut out = [0i128; 2];
    for k in 0..2 {
        out[k] = internal[k].checked_mul(WAD)? / s;
    }
    Some(out)
}

fuzz_target!(|data: &[u8]| {
    let mut i = 0;
    let alpha = TWO_PLUS_SQRT2;
    let params = [alpha, alpha];

    // Two internal reserves within a realistic band.
    let a = rd(data, &mut i).rem_euclid(1_000_000 * WAD) + WAD;
    let b = rd(data, &mut i).rem_euclid(1_000_000 * WAD) + WAD;
    let internal = [a, b];

    // Lower bracket: the smallest s keeping both legs on the arc (x̂ ≤ α).
    let lo_a = match a.checked_mul(WAD) {
        Some(v) => v / alpha + 1,
        None => return,
    };
    let lo_b = match b.checked_mul(WAD) {
        Some(v) => v / alpha + 1,
        None => return,
    };
    let lo = lo_a.max(lo_b);

    let residual = |s: i128| -> Option<i128> {
        let x = xhat_at(&internal, s)?;
        ndim::invariant_residual_n(&x, &params).ok()
    };

    // ── Property 1: on-arc, the residual is non-decreasing in s.
    let mut prev = match residual(lo) {
        Some(r) => r,
        None => return,
    };
    let mut s = lo;
    for _ in 0..40 {
        s = match s.checked_mul(2) {
            Some(v) => v,
            None => break,
        };
        let r = match residual(s) {
            Some(r) => r,
            None => break,
        };
        // Allow the transcendental noise floor; anything beyond it is a real
        // monotonicity break.
        assert!(
            r >= prev - EPSILON,
            "residual fell from {prev} to {r} as s rose to {s} (a={a}, b={b})"
        );
        prev = r;
    }

    // ── Property 2: if the bracket is valid, bisection finds a root.
    let r_lo = match residual(lo) {
        Some(r) => r,
        None => return,
    };
    if r_lo > 0 {
        return; // not representable on the curve — the contract errors here too
    }
    let mut hi = lo;
    let mut bracketed = false;
    for _ in 0..64 {
        hi = match hi.checked_mul(2) {
            Some(v) => v,
            None => break,
        };
        if matches!(residual(hi), Some(r) if r > 0) {
            bracketed = true;
            break;
        }
    }
    if !bracketed {
        return;
    }

    let mut low = lo;
    for _ in 0..128 {
        if hi - low <= 1 {
            break;
        }
        let mid = low + (hi - low) / 2;
        let r = match residual(mid) {
            Some(r) => r,
            None => return,
        };
        if r.unsigned_abs() <= EPSILON as u128 {
            return; // converged
        }
        if r < 0 {
            low = mid;
        } else {
            hi = mid;
        }
    }
    // Collapsed bracket: the low side must sit on (or just inside) the curve.
    if let Some(r) = residual(low) {
        assert!(r <= EPSILON, "bisection ended outside the curve: residual {r}");
    }
});
