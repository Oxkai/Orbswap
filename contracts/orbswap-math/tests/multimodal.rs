//! Integration tests for `multimodal.rs` — todo.md §1.10.

mod common;

use common::*;
use orbswap_math::fixed_point::{MathError, FIXED_SCALE};
use orbswap_math::multimodal::r_theta;

#[test]
fn r_theta_bounded() {
    // r ∈ [L/β, √2·L/β] for all θ (denominator ∈ [½,1] → no singularity).
    let (l_f, beta_f) = (1000.0f64, 2.0f64);
    let (l, beta) = (f64_to_wad(l_f), f64_to_wad(beta_f));
    let min = f64_to_wad(l_f / beta_f); // L/β
    let max = f64_to_wad(l_f / beta_f * 2f64.sqrt()); // √2·L/β
    for alpha in [4i128, 6, 8] {
        for theta in 0..360 {
            let r = r_theta(l, alpha, beta, theta).unwrap();
            assert!(
                r >= min - 1_000_000 && r <= max + 1_000_000,
                "out of band α={alpha} θ={theta}: r={r} [{min},{max}]"
            );
        }
    }
}

#[test]
fn r_theta_matches_f64_oracle() {
    let (l_f, beta_f) = (1000.0f64, 3.0f64);
    let (l, beta) = (f64_to_wad(l_f), f64_to_wad(beta_f));
    for alpha in [4i128, 6, 8] {
        for theta in 0..360 {
            let got = r_theta(l, alpha, beta, theta).unwrap();
            let ang = ((alpha * theta) as f64).to_radians();
            let want = l_f / (beta_f * (1.0 - 0.5 * ang.sin().powi(2)).sqrt());
            assert!(
                ((got as f64) / 1e18 - want).abs() <= want * 1e-6 + 1e-6,
                "α={alpha} θ={theta}: got {} want {want}",
                (got as f64) / 1e18
            );
        }
    }
}

#[test]
fn r_theta_periodicity_encodes_mode_structure() {
    // sin²(αθ) has period 180°/α, so r(θ) = r(θ + 180/α) *exactly* (sin(x+180)=
    // −sin(x), squared is identical). Higher α ⇒ shorter period ⇒ more modes.
    // (Counting minima at integer degrees aliases for α=8, whose period 22.5° is
    // non-integer — periodicity is the robust invariant.)
    let (l, beta) = (f64_to_wad(1000.0), f64_to_wad(2.0));
    // α=4 → period 45°, α=6 → period 30° (both integer).
    for (alpha, period) in [(4i128, 45i128), (6, 30)] {
        for theta in 0..360 {
            // ~ULP tolerance: sin(x+180) and −sin(x) come from independent f64
            // evaluations in build.rs, so they differ by ≤1 ULP → tiny r diff.
            assert_close(
                r_theta(l, alpha, beta, theta).unwrap(),
                r_theta(l, alpha, beta, theta + period).unwrap(),
                1_000_000_000,
                "periodicity",
            );
        }
    }
    // (Shorter period 30° for α=6 vs 45° for α=4 ⇒ higher mode frequency.)
}

#[test]
fn r_theta_guards() {
    assert_eq!(r_theta(-1, 4, FIXED_SCALE, 0), Err(MathError::DomainError));
    assert_eq!(r_theta(FIXED_SCALE, 4, 0, 0), Err(MathError::DomainError));
}
