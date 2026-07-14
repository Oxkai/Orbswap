//! Integration tests for `fingerprint.rs` — todo.md §1.11.

mod common;

use common::*;
use orbswap_math::fingerprint::liquidity_at_tick;
use orbswap_math::fixed_point::{MathError, FIXED_SCALE};

fn l_f(k: f64, t: f64) -> f64 {
    2.0 * k * (1.5 * t).exp() / (1.0 + (2.0 * t).exp()).powf(1.5)
}

#[test]
fn peak_at_zero_is_k_over_sqrt2() {
    // L(0) = 2k/2^{3/2} = k/√2.
    let k = 1000 * FIXED_SCALE;
    let got = liquidity_at_tick(k, 0).unwrap();
    let want = f64_to_wad(1000.0 / 2f64.sqrt());
    assert_close(got, want, 1_000_000_000, "L(0)=k/√2"); // 1e-9
}

#[test]
fn symmetric_in_t() {
    // L(t) = L(−t).
    let k = 500 * FIXED_SCALE;
    for t_int in 1..=40 {
        let t = t_int * FIXED_SCALE / 10; // 0.1 .. 4.0
        let pos = liquidity_at_tick(k, t).unwrap();
        let neg = liquidity_at_tick(k, -t).unwrap();
        // Symmetry holds up to transcendental rounding.
        assert!(
            (pos - neg).abs() <= pos / 1_000_000 + 4,
            "asymmetry at t={t}: {pos} vs {neg}"
        );
    }
}

#[test]
fn decays_in_tails_and_matches_oracle() {
    let k = 1000.0f64;
    let kw = f64_to_wad(k);
    let peak = liquidity_at_tick(kw, 0).unwrap();
    let mut prev = peak;
    for t_int in 1..=50 {
        let t = t_int * FIXED_SCALE / 10; // increasing t
        let got = liquidity_at_tick(kw, t).unwrap();
        // Monotonic decay for t > 0.
        assert!(got <= prev, "not decaying at t={t}");
        prev = got;
        // vs f64 oracle.
        let want = l_f(k, (t as f64) / 1e18);
        assert!(
            ((got as f64) / 1e18 - want).abs() <= want * 1e-6 + 1e-6,
            "oracle t={t}: got {} want {want}",
            (got as f64) / 1e18
        );
    }
    // Far tail is near zero (t=25 is well within the stable domain |t|≲31).
    let tail = liquidity_at_tick(kw, 25 * FIXED_SCALE).unwrap();
    assert!(tail < peak / 1_000_000, "tail not decayed: {tail}");
}

#[test]
fn guards() {
    assert_eq!(liquidity_at_tick(-1, 0), Err(MathError::NegativeInput));
}
