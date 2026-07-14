//! Integration tests for `ccmm.rs` — every edge case from todo.md §1.2.
//!
//! f64 is a host-side reference oracle only. On-curve integer points use scaled
//! Pythagorean triples so the invariant holds *exactly* (epsilon 0).

mod common;

use common::*;
use orbswap_math::ccmm::{invariant_holds, spot_price, swap_in, swap_out, CcmmError};
use orbswap_math::fixed_point::FIXED_SCALE;

/// True (real-valued) paired reserve on the lower arc: `k − √(2kx − x²)`.
fn true_paired(x: f64, k: f64) -> f64 {
    k - (2.0 * k * x - x * x).sqrt()
}

// ---------------------------------------------------------------- endpoints

#[test]
fn swap_out_full_arc_endpoints() {
    // At (0, k), swapping the full k of X lands exactly on (k, 0), out = k.
    let k = 100;
    let (out, nx, ny) = swap_out(0, k, k, k).unwrap();
    assert_eq!((out, nx, ny), (k, k, 0));
    // The resulting point is on-curve.
    assert!(invariant_holds(nx, ny, k, 0));
}

#[test]
fn swap_out_known_vector() {
    // todo.md golden vector: k=100, x=0, y=100, in=50 → out=86 (isqrt(7500)=86).
    let (out, nx, ny) = swap_out(0, 100, 100, 50).unwrap();
    assert_eq!((out, nx, ny), (86, 50, 14));
}

// ---------------------------------------------------------------- monotonicity

#[test]
fn swap_out_monotonic_and_bounded() {
    let k = 1_000_000_000;
    let mut prev_out = -1;
    for amt in (1..k).step_by((k / 200) as usize) {
        let (out, _, ny) = swap_out(0, k, k, amt).unwrap();
        assert!(
            out > prev_out,
            "monotonic at amt={amt}: {out} !> {prev_out}"
        );
        assert!(out <= k, "out {out} exceeds reserve k");
        assert!(ny >= 0);
        prev_out = out;
    }
}

// ---------------------------------------------------------------- rounding favors pool

#[test]
fn swap_out_rounds_toward_pool() {
    // Rigorous, f64-free: pool-favoring ⇔ `new_y = k − isqrt(rad)` is the tight
    // ceiling, i.e. `root² ≤ rad < (root+1)²` with `root = k − new_y`. This proves
    // `out ≤ true_out` exactly (new_y ≥ true_new_y). k ≤ 1e9 keeps `rad = k²` in i128.
    let mut rng = Rng::new(0xC0FFEE);
    for _ in 0..30_000 {
        let k = rng.range_i128(1_000, 1_000_000_000);
        let x = rng.range_i128(0, k - 1);
        // On-curve y for this x.
        let y = if x == 0 {
            k
        } else {
            swap_out(0, k, k, x).unwrap().2
        };
        let amt = rng.range_i128(1, k - x);
        let (out, nx, ny) = swap_out(x, y, k, amt).unwrap();

        let rad = 2 * k * nx - nx * nx; // = k² − (nx−k)² ≥ 0, fits since k ≤ 1e9
        let root = k - ny;
        assert!(root * root <= rad, "root² > rad (k={k} x={x} amt={amt})");
        assert!(
            (root + 1) * (root + 1) > rad,
            "not tight floor (k={k} x={x} amt={amt})"
        );
        // out is bounded by the reserve and consistent with the recomputed state.
        assert!(out <= y && out == y - ny);
    }
}

// ---------------------------------------------------------------- swap_in inverse

#[test]
fn swap_in_charges_at_least_true_and_stays_on_curve() {
    // The real guarantee (not "recovers dx", which is geometrically amplified near
    // steep arc regions): swap_in charges ≥ the true required input (pool-favoring)
    // and the resulting state is the tight floor on the curve. f64-free; k ≤ 1e9.
    let mut rng = Rng::new(7);
    for _ in 0..30_000 {
        let k = rng.range_i128(1_000, 1_000_000_000);
        // Start from an on-curve point (x0, y0) with x0 < k.
        let x0 = rng.range_i128(0, k - 1);
        let y0 = if x0 == 0 {
            k
        } else {
            swap_out(0, k, k, x0).unwrap().2
        };
        if y0 <= 1 {
            continue;
        }
        let amount_out = rng.range_i128(1, y0);
        let (in_req, nx, ny) = swap_in(x0, y0, k, amount_out).unwrap();

        assert_eq!(ny, y0 - amount_out, "new_y must be exact");
        // Tight floor: root = k − nx = isqrt(rad(ny)); root² ≤ rad < (root+1)².
        let rad = 2 * k * ny - ny * ny;
        let root = k - nx;
        assert!(
            root * root <= rad,
            "root² > rad (k={k} x0={x0} out={amount_out})"
        );
        assert!((root + 1) * (root + 1) > rad, "not tight floor");
        // Pool charges at least the true required input: nx ≥ true x for ny.
        // true_x = k − √rad ≤ k − root = nx  ⇔  root ≤ √rad  ⇔ root² ≤ rad (above).
        assert!(in_req == nx - x0 && in_req >= 0);
    }
}

#[test]
fn roundtrip_never_profits_trader() {
    // Swap X→Y, then swap that Y back to X: recovered X ≤ original in (no free money).
    let mut rng = Rng::new(0xBADDCAFE);
    for _ in 0..20_000 {
        let k = rng.range_i128(10_000, 1_000_000_000_000);
        let dx = rng.range_i128(1, k / 2);
        let (out, nx, ny) = swap_out(0, k, k, dx).unwrap();
        if out <= 0 {
            continue;
        }
        // Put `out` of Y back in (roles swapped): reserves are now (ny_in=nx? ) —
        // the Y reserve is ny, the X reserve is nx.
        let (back, ..) = swap_out(ny, nx, k, out).unwrap();
        assert!(
            back <= dx,
            "trader profited: dx={dx} back={back} (k={k} out={out})"
        );
    }
}

// ---------------------------------------------------------------- invariant_holds

#[test]
fn invariant_exact_on_pythagorean_points() {
    // (1,2) with k=5 is exactly on the circle: 4²+3²=5². Scale by λ (homogeneous).
    for lam in [1, 2, 7, 100, 1_000_000, 1_000_000_000_000] {
        let k = 5 * lam;
        for &(x, y) in &[(0, 5), (1, 2), (2, 1), (5, 0)] {
            assert!(
                invariant_holds(x * lam, y * lam, k, 0),
                "exact point ({},{}) k={k} should hold",
                x * lam,
                y * lam
            );
        }
    }
}

#[test]
fn invariant_within_epsilon_after_swap() {
    let mut rng = Rng::new(123);
    for _ in 0..10_000 {
        let k = rng.range_i128(1_000, 1_000_000_000_000);
        let dx = rng.range_i128(1, k);
        let (_, nx, ny) = swap_out(0, k, k, dx).unwrap();
        // Residual after integer-√ truncation is bounded by ~2k.
        assert!(
            invariant_holds(nx, ny, k, 2 * k + 2),
            "post-swap off-curve beyond 2k: k={k} dx={dx} → ({nx},{ny})"
        );
    }
}

#[test]
fn invariant_rejects_off_curve() {
    let k = 1_000_000;
    // Clearly off the circle by a lot.
    assert!(!invariant_holds(k / 2, k / 2, k, 0));
    assert!(!invariant_holds(0, 0, k, 0));
    // (1,2)·1 with k=5 holds; nudging y by 1 breaks it beyond epsilon 0.
    assert!(invariant_holds(1, 2, 5, 0));
    assert!(!invariant_holds(1, 3, 5, 0));
}

// ---------------------------------------------------------------- spot_price

#[test]
fn spot_price_is_one_when_balanced() {
    // x == y ⇒ (x−k)/(y−k) = 1 exactly, for any x < k.
    for &(x, k) in &[(30, 100), (1, 5), (999, 1_000_000)] {
        assert_eq!(
            spot_price(x, x, k),
            FIXED_SCALE,
            "balanced price = 1 (x={x} k={k})"
        );
    }
}

#[test]
fn spot_price_boundaries() {
    let k = 1_000;
    // x = 0 ⇒ y = k ⇒ price → +∞ sentinel.
    assert_eq!(spot_price(0, k, k), i128::MAX);
    // x = k ⇒ y = 0 ⇒ price = 0.
    assert_eq!(spot_price(k, 0, k), 0);
}

#[test]
fn spot_price_matches_ratio_oracle() {
    // On a scaled Pythagorean point (1,2)·λ, k=5λ: price = (1−5)/(2−5) = 4/3.
    let lam = 1_000_000_000i128;
    let (x, y, k) = (lam, 2 * lam, 5 * lam);
    let p = spot_price(x, y, k);
    // Exact integer expectation (the f64 4.0/3.0 is *less* precise than mul_div).
    assert_eq!(p, 4 * FIXED_SCALE / 3, "price = 4/3 on (1,2)·λ");
}

// ---------------------------------------------------------------- errors

#[test]
fn errors_invalid_amount() {
    assert_eq!(swap_out(0, 100, 100, 0), Err(CcmmError::InvalidAmount));
    assert_eq!(swap_out(0, 100, 100, -5), Err(CcmmError::InvalidAmount));
    assert_eq!(swap_in(50, 50, 100, 0), Err(CcmmError::InvalidAmount));
    assert_eq!(swap_in(50, 50, 100, -1), Err(CcmmError::InvalidAmount));
}

#[test]
fn errors_out_of_range_state() {
    // x > k, y > k, negative reserves, non-positive k.
    assert_eq!(swap_out(150, 50, 100, 1), Err(CcmmError::OutOfRange));
    assert_eq!(swap_out(50, 150, 100, 1), Err(CcmmError::OutOfRange));
    assert_eq!(swap_out(-1, 50, 100, 1), Err(CcmmError::OutOfRange));
    assert_eq!(swap_out(50, 50, 0, 1), Err(CcmmError::OutOfRange));
    assert_eq!(swap_out(50, 50, -100, 1), Err(CcmmError::OutOfRange));
}

#[test]
fn errors_price_out_of_range_fold() {
    // new_x = x + amount_in > k enters the negative-price fold.
    assert_eq!(swap_out(50, 14, 100, 60), Err(CcmmError::PriceOutOfRange));
    // Exactly reaching k is allowed (lands on (k,0)); one past is not.
    assert!(swap_out(0, 100, 100, 100).is_ok());
    assert_eq!(swap_out(0, 100, 100, 101), Err(CcmmError::PriceOutOfRange));
}

#[test]
fn errors_swap_in_over_withdraw() {
    // Asking for more Y than the pool holds → new_y < 0.
    assert_eq!(swap_in(0, 100, 100, 101), Err(CcmmError::OutOfRange));
    // Exactly y is allowed (drains Y, lands on (k,0)).
    assert!(swap_in(0, 100, 100, 100).is_ok());
}

#[test]
fn errors_overflow_on_huge_reserves() {
    // k = 1e21, in = 5e20 ⇒ radicand ≈ 7.5e41 ≫ i128::MAX → Overflow.
    let k = 1_000_000_000_000_000_000_000;
    let amt = 500_000_000_000_000_000_000;
    assert_eq!(swap_out(0, k, k, amt), Err(CcmmError::Overflow));
}

// ---------------------------------------------------------------- swap vs oracle

#[test]
fn swap_out_matches_f64_oracle() {
    let mut rng = Rng::new(2718);
    for _ in 0..20_000 {
        let k = rng.range_i128(1_000, 100_000_000_000_000);
        let dx = rng.range_i128(1, k);
        let (out, nx, ny) = swap_out(0, k, k, dx).unwrap();
        let want_ny = true_paired(nx as f64, k as f64);
        assert!(
            (ny as f64 - want_ny).abs() <= 1.5,
            "ny oracle: got {ny} want {want_ny} (k={k} dx={dx})"
        );
        assert_eq!(out, k - ny); // out consistency (started at y=k)
    }
}
