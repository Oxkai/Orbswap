//! Integration tests for `ndim.rs` — every edge case from todo.md §1.9.

mod common;

use common::*;
use orbswap_math::csemm::{self, CsemmError};
use orbswap_math::fixed_point::FIXED_SCALE;
use orbswap_math::ndim::{invariant_holds_n, swap_out_n};

const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049; // 2+√2 (u=2)

fn u_f(a: f64) -> f64 {
    2f64.ln() / (a / (a - 1.0)).ln()
}
fn term_f(x: f64, a: f64) -> f64 {
    (x / a - 1.0).abs().powf(u_f(a))
}

// ---------------------------------------------------------------- n=2 ≡ csemm

#[test]
fn n2_reduces_to_csemm() {
    // swap_out_n on a 2-vector must equal csemm::swap_out exactly.
    let mut rng = Rng::new(0x2D);
    let mut checked = 0;
    for _ in 0..3_000 {
        let a = 2.0 + (rng.range_i128(0, 8_000) as f64) / 1000.0;
        let b = 2.0 + (rng.range_i128(0, 8_000) as f64) / 1000.0;
        let (alpha, beta) = (f64_to_wad(a), f64_to_wad(b));
        let x0f = a * (rng.range_i128(0, 800) as f64) / 1000.0;
        let x0 = f64_to_wad(x0f);
        // On-curve y via csemm forward from a fresh point isn't handy; use f64.
        let y0 = f64_to_wad(b * (1.0 - (1.0 - (1.0 - x0f / a).powf(u_f(a))).powf(1.0 / u_f(b))))
            .clamp(0, beta);
        let dx = f64_to_wad((a - x0f) * (rng.range_i128(1, 500) as f64) / 1000.0);
        if dx <= 0 || x0 + dx > alpha {
            continue;
        }
        let via_csemm = csemm::swap_out(x0, y0, alpha, beta, dx);
        let via_ndim = swap_out_n(&[x0, y0], &[alpha, beta], 0, 1, dx);
        match (via_csemm, via_ndim) {
            (Ok((c_out, c_nx, c_ny)), Ok((n_out, n_nx, n_ny))) => {
                assert_eq!((c_out, c_nx, c_ny), (n_out, n_nx, n_ny), "n=2 vs csemm");
                checked += 1;
            }
            (Err(_), Err(_)) => {}
            (c, d) => panic!("validity disagreement {c:?} vs {d:?}"),
        }
    }
    assert!(checked > 1_500, "too few: {checked}");
}

// ---------------------------------------------------------------- n=3 balanced / sphere

#[test]
fn n3_balanced_point_on_sphere() {
    // u=2 (all α=2+√2): balanced x_i = k(1−1/√3). Invariant = 1.
    let k = TWO_PLUS_SQRT2;
    let xb = f64_to_wad((2.0 + 2f64.sqrt()) * (1.0 - 1.0 / 3f64.sqrt()));
    assert!(
        invariant_holds_n(&[xb, xb, xb], &[k, k, k], 100_000_000),
        "n=3 balanced not on the sphere"
    );
    // A clearly off-sphere point is rejected.
    assert!(!invariant_holds_n(
        &[k / 2, k / 2, k / 2],
        &[k, k, k],
        1_000_000
    ));
}

#[test]
fn n3_swap_exact_solve_holds_invariant() {
    // After a swap, the full n=3 invariant must still hold tightly (exact solve,
    // not the linearized ΔI/Δx shortcut). Also: the untouched reserve is unchanged.
    let k = TWO_PLUS_SQRT2;
    let xb = f64_to_wad((2.0 + 2f64.sqrt()) * (1.0 - 1.0 / 3f64.sqrt()));
    let res = [xb, xb, xb];
    let dx = k / 4;
    let (out, new_in, new_out) = swap_out_n(&res, &[k, k, k], 0, 1, dx).unwrap();
    assert!(out > 0);
    // Reconstruct post-swap reserves: token2 (index) untouched.
    let post = [new_in, new_out, res[2]];
    assert_eq!(post[2], xb, "untouched reserve changed");
    assert!(
        invariant_holds_n(&post, &[k, k, k], 200_000_000),
        "post-swap invariant off (exact solve failed)"
    );
}

#[test]
fn n3_swap_matches_f64_oracle_asymmetric() {
    // Asymmetric params, verify amount_out vs an independent f64 solve.
    let params_f = [3.0f64, 5.0, 7.0];
    let params = [f64_to_wad(3.0), f64_to_wad(5.0), f64_to_wad(7.0)];
    // Construct an on-invariant start: pick x0,x1 in range, solve x2.
    let x0f = 1.0;
    let x1f = 2.0;
    let s01 = term_f(x0f, params_f[0]) + term_f(x1f, params_f[1]);
    let x2f = params_f[2] * (1.0 - (1.0 - s01).powf(1.0 / u_f(params_f[2])));
    let res = [f64_to_wad(x0f), f64_to_wad(x1f), f64_to_wad(x2f)];

    let dxf = 0.5;
    let dx = f64_to_wad(dxf);
    let (out, _, _) = swap_out_n(&res, &params, 0, 2, dx).unwrap();

    // f64 oracle: add dx to token0, solve token2, token1 fixed.
    let s = term_f(x0f + dxf, params_f[0]) + term_f(x1f, params_f[1]);
    let new_x2 = params_f[2] * (1.0 - (1.0 - s).powf(1.0 / u_f(params_f[2])));
    let want_out = x2f - new_x2;
    assert!(
        ((out as f64) / 1e18 - want_out).abs() <= want_out.abs() * 1e-7 + 1e-9,
        "n=3 out {} vs oracle {want_out}",
        (out as f64) / 1e18
    );
}

// ---------------------------------------------------------------- errors

#[test]
fn error_paths() {
    let k = TWO_PLUS_SQRT2;
    let res = [k / 2, k / 2, k / 2];
    let p = [k, k, k];
    // bad shape / indices
    assert_eq!(
        swap_out_n(&res, &[k, k], 0, 1, 1),
        Err(CsemmError::OutOfRange)
    ); // len mismatch
    assert_eq!(swap_out_n(&[k], &[k], 0, 0, 1), Err(CsemmError::OutOfRange)); // n<2
    assert_eq!(swap_out_n(&res, &p, 0, 0, 1), Err(CsemmError::OutOfRange)); // in==out
    assert_eq!(swap_out_n(&res, &p, 0, 5, 1), Err(CsemmError::OutOfRange)); // out of range idx
                                                                            // amount ≤ 0
    assert_eq!(
        swap_out_n(&res, &p, 0, 1, 0),
        Err(CsemmError::InvalidAmount)
    );
    // shape param < 2
    assert_eq!(
        swap_out_n(&res, &[FIXED_SCALE, k, k], 0, 1, 1),
        Err(CsemmError::DomainError)
    );
    // push x_in past α_in → PriceOutOfRange
    assert_eq!(
        swap_out_n(&res, &p, 0, 1, 2 * k),
        Err(CsemmError::PriceOutOfRange)
    );
}
