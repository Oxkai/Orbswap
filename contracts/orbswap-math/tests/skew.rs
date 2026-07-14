//! Integration tests for `skew.rs` — every edge case from todo.md §1.6.

mod common;

use common::*;
use orbswap_math::ccmm;
use orbswap_math::fixed_point::FIXED_SCALE;
use orbswap_math::skew::{apply_skew, unapply_skew, SkewError};

const ONE: i128 = FIXED_SCALE; // skew factor 1.0

// ---------------------------------------------------------------- identity / ccmm regression

#[test]
fn identity_a_b_one_recovers_circle() {
    // a=b=1 ⇒ apply_skew is the identity, and the point stays on the CCMM circle.
    let l = 1_000i128;
    // Circle points on (u−L)²+(v−L)²=L² (radius L, k=L).
    for &(u, v) in &[(0, 1_000), (1_000, 0), (2_000, 1_000), (1_000, 2_000)] {
        let (x, y) = apply_skew(u, v, ONE, ONE).unwrap();
        assert_eq!((x, y), (u, v), "identity at ({u},{v})");
        assert!(
            ccmm::invariant_holds(x, y, l, 0),
            "on-circle after identity skew"
        );
    }
}

#[test]
fn identity_random_points() {
    let mut rng = Rng::new(0x5CEE);
    for _ in 0..10_000 {
        let u = rng.range_i128(0, 1_000_000_000_000_000_000);
        let v = rng.range_i128(0, 1_000_000_000_000_000_000);
        assert_eq!(apply_skew(u, v, ONE, ONE).unwrap(), (u, v));
        assert_eq!(unapply_skew(u, v, ONE, ONE).unwrap(), (u, v));
    }
}

// ---------------------------------------------------------------- asymmetric extents

#[test]
fn asymmetric_extents() {
    // Rightmost circle point (2L, L) with L=1000, a=1.5, b=0.5 →
    // ellipse (a·2L, b·L) = (2aL, bL) = (3000, 500).
    let (a, b) = (3 * ONE / 2, ONE / 2); // 1.5, 0.5
    let (x, y) = apply_skew(2_000, 1_000, a, b).unwrap();
    assert_eq!((x, y), (3_000, 500), "x extent = 2aL, y = bL");
    // Top circle point (L, 2L) → (aL, 2bL) = (1500, 1000).
    let (x, y) = apply_skew(1_000, 2_000, a, b).unwrap();
    assert_eq!((x, y), (1_500, 1_000));
}

#[test]
fn apply_matches_scaling_oracle() {
    let mut rng = Rng::new(31);
    for _ in 0..10_000 {
        let u = rng.range_i128(0, 1_000_000_000);
        let v = rng.range_i128(0, 1_000_000_000);
        let a = rng.range_i128(ONE / 100, 100 * ONE); // 0.01 .. 100
        let b = rng.range_i128(ONE / 100, 100 * ONE);
        let (x, y) = apply_skew(u, v, a, b).unwrap();
        // Oracle: ⌊a·u/1e18⌋, ⌊b·v/1e18⌋ via i128 (a·u fits: ≤ 1e20·1e9 = 1e29).
        assert_eq!(x, (a * u) / FIXED_SCALE, "x = a·u");
        assert_eq!(y, (b * v) / FIXED_SCALE, "y = b·v");
    }
}

// ---------------------------------------------------------------- roundtrip

#[test]
fn apply_then_unapply_roundtrips() {
    let mut rng = Rng::new(0xF00D);
    for _ in 0..10_000 {
        let u = rng.range_i128(0, 1_000_000_000_000);
        let v = rng.range_i128(0, 1_000_000_000_000);
        let a = rng.range_i128(ONE / 10, 10 * ONE);
        let b = rng.range_i128(ONE / 10, 10 * ONE);
        let (x, y) = apply_skew(u, v, a, b).unwrap();
        let (u2, v2) = unapply_skew(x, y, a, b).unwrap();
        // Recovered ≤ original (pool-favoring, both floors). The un-skew divides by
        // a, so a 1-unit drop in apply amplifies to ≤ SCALE/a units back.
        let tol_u = FIXED_SCALE / a + 2;
        let tol_v = FIXED_SCALE / b + 2;
        assert!(
            u2 <= u && u - u2 <= tol_u,
            "u roundtrip u={u} u2={u2} a={a}"
        );
        assert!(
            v2 <= v && v - v2 <= tol_v,
            "v roundtrip v={v} v2={v2} b={b}"
        );
    }
}

// ---------------------------------------------------------------- guards

#[test]
fn rejects_non_positive_skew() {
    assert_eq!(apply_skew(100, 100, 0, ONE), Err(SkewError::InvalidSkew));
    assert_eq!(apply_skew(100, 100, ONE, 0), Err(SkewError::InvalidSkew));
    assert_eq!(apply_skew(100, 100, -ONE, ONE), Err(SkewError::InvalidSkew));
    assert_eq!(unapply_skew(100, 100, ONE, -1), Err(SkewError::InvalidSkew));
}
