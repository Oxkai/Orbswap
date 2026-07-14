//! Integration tests for `fixed_point.rs` — every edge case from todo.md §1.1.
//!
//! f64 appears only as a reference oracle (host-side); the library itself is
//! integer-only. Exact expectations are asserted exactly; transcendental
//! results are asserted within the documented error bounds.

mod common;

use common::*;
use orbswap_math::fixed_point::{
    exp_fixed, isqrt, ln_fixed, mul_div, pow_fixed, pow_int, MathError, Rounding, E_WAD,
    FIXED_SCALE, LN2,
};

// ---------------------------------------------------------------- isqrt

#[test]
fn isqrt_small_values() {
    let cases = [
        (0, 0),
        (1, 1),
        (2, 1),
        (3, 1),
        (4, 2),
        (99, 9),
        (100, 10),
        (101, 10),
    ];
    for (n, want) in cases {
        assert_eq!(isqrt(n), want, "isqrt({n})");
    }
}

#[test]
fn isqrt_perfect_squares() {
    for r in 0i128..=1_000 {
        assert_eq!(isqrt(r * r), r, "isqrt({r}²)");
        if r > 0 {
            assert_eq!(isqrt(r * r - 1), r - 1, "isqrt({r}²−1)");
            assert_eq!(isqrt(r * r + 1), r, "isqrt({r}²+1)");
        }
    }
}

#[test]
fn isqrt_wad_squared() {
    // (1e18)² = 1e36 → 1e18
    assert_eq!(isqrt(FIXED_SCALE * FIXED_SCALE), FIXED_SCALE);
}

#[test]
fn isqrt_negative_is_zero() {
    assert_eq!(isqrt(-1), 0);
    assert_eq!(isqrt(-1_000_000), 0);
    assert_eq!(isqrt(i128::MIN), 0);
}

#[test]
fn isqrt_i128_max_exact_floor() {
    let n = i128::MAX;
    let r = isqrt(n);
    let ru = r as u128;
    let nu = n as u128;
    assert!(ru * ru <= nu, "r² ≤ n");
    assert!((ru + 1) * (ru + 1) > nu, "(r+1)² > n");
}

#[test]
fn isqrt_floor_property_random() {
    let mut rng = Rng::new(0xDEAD_BEEF);
    for _ in 0..10_000 {
        let n = rng.range_i128(0, i128::MAX);
        let r = isqrt(n) as u128;
        let nu = n as u128;
        assert!(r * r <= nu, "r² ≤ n for n={n}");
        assert!((r + 1) * (r + 1) > nu, "(r+1)² > n for n={n}");
    }
}

#[test]
fn isqrt_monotonic() {
    let mut rng = Rng::new(42);
    for _ in 0..2_000 {
        let a = rng.range_i128(0, i128::MAX - 1);
        let b = rng.range_i128(a, i128::MAX);
        assert!(isqrt(a) <= isqrt(b), "monotonic at a={a} b={b}");
    }
}

// ---------------------------------------------------------------- mul_div

#[test]
fn mul_div_rounding_positive() {
    assert_eq!(mul_div(7, 1, 2, Rounding::Down), Ok(3)); // floor(3.5)
    assert_eq!(mul_div(7, 1, 2, Rounding::Up), Ok(4)); // ceil(3.5)
    assert_eq!(mul_div(6, 1, 2, Rounding::Down), Ok(3)); // exact: no bump
    assert_eq!(mul_div(6, 1, 2, Rounding::Up), Ok(3));
}

#[test]
fn mul_div_rounding_negative_is_directional() {
    // Down = toward −∞, Up = toward +∞ (not toward/away from zero).
    assert_eq!(mul_div(-7, 1, 2, Rounding::Down), Ok(-4)); // floor(−3.5)
    assert_eq!(mul_div(-7, 1, 2, Rounding::Up), Ok(-3)); // ceil(−3.5)
    assert_eq!(mul_div(7, -1, 2, Rounding::Down), Ok(-4));
    assert_eq!(mul_div(7, 1, -2, Rounding::Down), Ok(-4));
    // Double negative = positive.
    assert_eq!(mul_div(-7, -1, 2, Rounding::Down), Ok(3));
    assert_eq!(mul_div(-7, 1, -2, Rounding::Up), Ok(4));
}

#[test]
fn mul_div_wide_intermediate() {
    // a·b = 1e60 overflows i128 (max ~1.7e38) but the result fits.
    let e30: i128 = 1_000_000_000_000_000_000_000_000_000_000;
    assert_eq!(mul_div(e30, e30, e30, Rounding::Down), Ok(e30));

    // i128::MAX·2/4 = (2^128 − 2)/4 = 2^126 − 0.5 → floor / ceil around 2^126.
    let q = 1i128 << 126;
    assert_eq!(mul_div(i128::MAX, 2, 4, Rounding::Down), Ok(q - 1));
    assert_eq!(mul_div(i128::MAX, 2, 4, Rounding::Up), Ok(q));
}

#[test]
fn mul_div_identity_and_extremes() {
    assert_eq!(mul_div(i128::MAX, 1, 1, Rounding::Down), Ok(i128::MAX));
    assert_eq!(mul_div(-i128::MAX, 1, 1, Rounding::Down), Ok(-i128::MAX));
    assert_eq!(mul_div(0, i128::MAX, i128::MAX, Rounding::Up), Ok(0));
    // Documented range trade-off: a result of exactly i128::MIN is Overflow.
    assert_eq!(
        mul_div(i128::MIN, 1, 1, Rounding::Down),
        Err(MathError::Overflow)
    );
}

#[test]
fn mul_div_errors() {
    assert_eq!(mul_div(1, 1, 0, Rounding::Down), Err(MathError::DivByZero));
    assert_eq!(
        mul_div(i128::MAX, i128::MAX, 1, Rounding::Down),
        Err(MathError::Overflow)
    );
    // Quotient magnitude i128::MAX itself still fits:
    assert_eq!(mul_div(i128::MAX, 3, 3, Rounding::Down), Ok(i128::MAX));
}

#[test]
fn mul_div_matches_exact_rational_oracle() {
    // For operands small enough that a·b fits in i128, compare against exact
    // div_euclid floor/ceil semantics — including every sign combination.
    let mut rng = Rng::new(7);
    for _ in 0..20_000 {
        let a = rng.range_i128(-1_000_000_000, 1_000_000_000);
        let b = rng.range_i128(-1_000_000_000, 1_000_000_000);
        let mut d = rng.range_i128(-1_000_000_000, 1_000_000_000);
        if d == 0 {
            d = 1;
        }
        let p = a * b; // fits: |p| ≤ 1e18
                       // Exact integer oracle: ⌊p/d⌋ and ⌈p/d⌉ from truncating division + remainder sign.
        let exact_floor = {
            let q = p / d;
            let r = p % d;
            if r != 0 && ((r < 0) != (d < 0)) {
                q - 1
            } else {
                q
            }
        };
        let exact_ceil = {
            let q = p / d;
            let r = p % d;
            if r != 0 && ((r < 0) == (d < 0)) {
                q + 1
            } else {
                q
            }
        };
        assert_eq!(
            mul_div(a, b, d, Rounding::Down),
            Ok(exact_floor),
            "floor a={a} b={b} d={d}"
        );
        assert_eq!(
            mul_div(a, b, d, Rounding::Up),
            Ok(exact_ceil),
            "ceil a={a} b={b} d={d}"
        );
    }
}

#[test]
fn mul_div_down_le_up_sandwich() {
    let mut rng = Rng::new(99);
    for _ in 0..5_000 {
        let a = rng.range_i128(-i128::MAX / 2, i128::MAX / 2);
        let b = rng.range_i128(-1_000_000, 1_000_000);
        let mut d = rng.range_i128(-1_000_000_000_000, 1_000_000_000_000);
        if d == 0 {
            d = 1;
        }
        if let (Ok(down), Ok(up)) = (
            mul_div(a, b, d, Rounding::Down),
            mul_div(a, b, d, Rounding::Up),
        ) {
            assert!(down <= up, "sandwich a={a} b={b} d={d}");
            assert!(up - down <= 1, "differ ≤ 1 a={a} b={b} d={d}");
        }
    }
}

// ---------------------------------------------------------------- pow_int

#[test]
fn pow_int_basics() {
    assert_eq!(pow_int(0, 0), Ok(FIXED_SCALE)); // 0⁰ = 1 by AMM convention
    assert_eq!(pow_int(5 * FIXED_SCALE, 0), Ok(FIXED_SCALE));
    assert_eq!(pow_int(-3 * FIXED_SCALE, 0), Ok(FIXED_SCALE));
    assert_eq!(pow_int(7 * FIXED_SCALE, 1), Ok(7 * FIXED_SCALE));
    assert_eq!(pow_int(-7 * FIXED_SCALE, 1), Ok(-7 * FIXED_SCALE));
    // Powers of two are exact in WAD.
    assert_eq!(pow_int(2 * FIXED_SCALE, 10), Ok(1024 * FIXED_SCALE));
}

#[test]
fn pow_int_negative_base_parity() {
    assert_eq!(pow_int(-2 * FIXED_SCALE, 3), Ok(-8 * FIXED_SCALE)); // odd → negative
    assert_eq!(pow_int(-2 * FIXED_SCALE, 2), Ok(4 * FIXED_SCALE)); // even → positive
}

#[test]
fn pow_int_overflow() {
    // 10^40 ≈ 1e40 as a value — beyond i128 WAD range (~1.7e20).
    assert_eq!(pow_int(10 * FIXED_SCALE, 40), Err(MathError::Overflow));
}

#[test]
fn pow_int_vs_f64_oracle() {
    let mut rng = Rng::new(1234);
    for _ in 0..2_000 {
        let base = rng.range_i128(1, 20 * FIXED_SCALE);
        let exp = rng.range_i128(0, 8) as u32;
        match pow_int(base, exp) {
            Ok(got) => {
                let want = wad_to_f64(base).powi(exp as i32);
                if want < 1e20 {
                    assert_rel_close(
                        got,
                        f64_to_wad(want),
                        1_000_000_000, // 1e-9 relative
                        exp as i128 + 2,
                        "pow_int oracle",
                    );
                }
            }
            Err(MathError::Overflow) => {
                assert!(
                    wad_to_f64(base).powi(exp as i32) > 1e19,
                    "spurious overflow"
                );
            }
            Err(e) => panic!("unexpected error {e:?}"),
        }
    }
}

// ---------------------------------------------------------------- ln_fixed

#[test]
fn ln_exact_powers_of_two() {
    // Exact by construction of the range reduction.
    assert_eq!(ln_fixed(FIXED_SCALE), Ok(0)); // ln 1 = 0
    assert_eq!(ln_fixed(2 * FIXED_SCALE), Ok(LN2));
    assert_eq!(ln_fixed(4 * FIXED_SCALE), Ok(2 * LN2));
    assert_eq!(ln_fixed(FIXED_SCALE / 2), Ok(-LN2));
    assert_eq!(ln_fixed(FIXED_SCALE / 4), Ok(-2 * LN2));
}

#[test]
fn ln_known_values() {
    // ln e = 1, documented bound ≤ ~300 ULP; assert well within 1e4 ULP (1e-14).
    let got = ln_fixed(E_WAD).unwrap();
    assert_close(got, FIXED_SCALE, 10_000, "ln(e) = 1");

    // ln 3 = 1.0986122886681098…
    let got = ln_fixed(3 * FIXED_SCALE).unwrap();
    assert_close(got, 1_098_612_288_668_109_691, 10_000, "ln 3");

    // ln 10 = 2.302585092994046…
    let got = ln_fixed(10 * FIXED_SCALE).unwrap();
    assert_close(got, 2_302_585_092_994_045_684, 10_000, "ln 10");
}

#[test]
fn ln_domain_errors() {
    assert_eq!(ln_fixed(0), Err(MathError::DomainError));
    assert_eq!(ln_fixed(-1), Err(MathError::DomainError));
    assert_eq!(ln_fixed(i128::MIN), Err(MathError::DomainError));
}

#[test]
fn ln_extreme_inputs() {
    // Smallest positive value: 1 wei = 1e-18 → ln ≈ −41.4465…
    let got = ln_fixed(1).unwrap();
    assert_close(got, f64_to_wad((1e-18f64).ln()), 1_000_000, "ln(1 wei)");
    // Largest: i128::MAX ≈ 1.7014e20 as a value → ln ≈ 46.5827
    let got = ln_fixed(i128::MAX).unwrap();
    assert_close(
        got,
        f64_to_wad((i128::MAX as f64 / 1e18).ln()),
        1_000_000,
        "ln(MAX)",
    );
}

#[test]
fn ln_monotonic_and_vs_f64() {
    let mut rng = Rng::new(2024);
    let mut prev: Option<(i128, i128)> = None;
    let mut xs: Vec<i128> = (0..3_000).map(|_| rng.range_i128(1, i128::MAX)).collect();
    xs.sort_unstable();
    for x in xs {
        let got = ln_fixed(x).unwrap();
        // vs f64 oracle: 1e-10 relative with a small absolute floor
        let want = f64_to_wad(wad_to_f64(x).ln());
        assert_rel_close(got, want, 10_000_000_000, 1_000_000, "ln oracle");
        if let Some((px, pln)) = prev {
            if x > px {
                assert!(got >= pln, "monotonic: ln({x}) < ln({px})");
            }
        }
        prev = Some((x, got));
    }
}

// ---------------------------------------------------------------- exp_fixed

#[test]
fn exp_exact_and_known_values() {
    assert_eq!(exp_fixed(0), Ok(FIXED_SCALE)); // e⁰ = 1 exact
    assert_eq!(exp_fixed(LN2), Ok(2 * FIXED_SCALE)); // r = 0 branch → exact 2
    let got = exp_fixed(-LN2).unwrap();
    assert_close(got, FIXED_SCALE / 2, 2, "e^−ln2 = 0.5");
    let got = exp_fixed(FIXED_SCALE).unwrap();
    assert_close(got, E_WAD, 10_000, "e¹ = e");
}

#[test]
fn exp_overflow_and_underflow() {
    assert_eq!(exp_fixed(47 * FIXED_SCALE + 1), Err(MathError::Overflow));
    // 46.6 > ln(i128::MAX/1e18) ≈ 46.5827 → precise overflow via checked mul
    assert_eq!(
        exp_fixed(46 * FIXED_SCALE + 600_000_000_000_000_000),
        Err(MathError::Overflow)
    );
    // 46.0 still fits (e^46 ≈ 9.49e19 → 9.49e37 WAD)
    let got = exp_fixed(46 * FIXED_SCALE).unwrap();
    assert_rel_close(got, f64_to_wad(46f64.exp()), 1_000_000_000, 0, "e^46");

    // Deep negatives underflow to 0 at WAD resolution.
    assert_eq!(exp_fixed(-100 * FIXED_SCALE - 1), Ok(0));
    let tiny = exp_fixed(-45 * FIXED_SCALE).unwrap(); // e^−45 ≈ 2.9e-20 < 1e-18
    assert!(tiny <= 1, "e^−45 rounds to ≤ 1 wei, got {tiny}");
}

#[test]
fn exp_ln_roundtrip() {
    let mut rng = Rng::new(555);
    for _ in 0..3_000 {
        // Log-uniform spread across ~30 orders of magnitude.
        let magnitude = rng.range_i128(0, 30) as u32;
        let x = rng.range_i128(1, 10i128.pow(magnitude).clamp(1, i128::MAX / 2));
        let ln_x = ln_fixed(x).unwrap();
        let back = exp_fixed(ln_x).unwrap();
        // 1e-12 relative, 2 wei absolute floor for the tiniest inputs.
        assert_rel_close(back, x, 1_000_000_000_000, 2, "exp(ln x) = x");
    }
}

#[test]
fn exp_monotonic_and_vs_f64() {
    let mut rng = Rng::new(31337);
    let mut xs: Vec<i128> = (0..3_000)
        .map(|_| rng.range_i128(-50 * FIXED_SCALE, 46 * FIXED_SCALE))
        .collect();
    xs.sort_unstable();
    let mut prev: Option<(i128, i128)> = None;
    for x in xs {
        let got = exp_fixed(x).unwrap();
        let want = f64_to_wad(wad_to_f64(x).exp());
        assert_rel_close(got, want, 10_000_000_000, 2, "exp oracle");
        if let Some((px, pe)) = prev {
            if x > px {
                assert!(got >= pe, "monotonic: exp({x}) < exp({px})");
            }
        }
        prev = Some((x, got));
    }
}

// ---------------------------------------------------------------- pow_fixed

#[test]
fn pow_fixed_squares_match_mul() {
    let mut rng = Rng::new(808);
    for _ in 0..2_000 {
        let x = rng.range_i128(FIXED_SCALE / 1_000, 1_000 * FIXED_SCALE);
        let via_pow = pow_fixed(x, 2 * FIXED_SCALE).unwrap();
        let via_mul = mul_div(x, x, FIXED_SCALE, Rounding::Down).unwrap();
        assert_rel_close(via_pow, via_mul, 1_000_000_000_000, 4, "x^2 = x·x");
    }
}

#[test]
fn pow_fixed_roots_and_identities() {
    let half = FIXED_SCALE / 2;
    assert_rel_close(
        pow_fixed(4 * FIXED_SCALE, half).unwrap(),
        2 * FIXED_SCALE,
        1_000_000_000_000,
        2,
        "4^0.5 = 2",
    );
    assert_rel_close(
        pow_fixed(9 * FIXED_SCALE, half).unwrap(),
        3 * FIXED_SCALE,
        1_000_000_000_000,
        2,
        "9^0.5 = 3",
    );
    // x^1 = x, x^0 = 1
    let x = 123_456_789_000_000_000_000; // 123.456789
    assert_rel_close(
        pow_fixed(x, FIXED_SCALE).unwrap(),
        x,
        1_000_000_000_000,
        2,
        "x^1",
    );
    assert_eq!(pow_fixed(x, 0), Ok(FIXED_SCALE));
    // Negative exponent: 2^−1 = 0.5
    assert_rel_close(
        pow_fixed(2 * FIXED_SCALE, -FIXED_SCALE).unwrap(),
        half,
        1_000_000_000_000,
        2,
        "2^−1",
    );
}

#[test]
fn pow_fixed_domain() {
    // Negative base is a DomainError — never silently wrong (INVARIANT_MATH §4).
    assert_eq!(
        pow_fixed(-FIXED_SCALE, FIXED_SCALE / 3),
        Err(MathError::DomainError)
    );
    assert_eq!(pow_fixed(-1, 2 * FIXED_SCALE), Err(MathError::DomainError));
    // 0^positive = 0; 0^0 and 0^negative are undefined.
    assert_eq!(pow_fixed(0, FIXED_SCALE), Ok(0));
    assert_eq!(pow_fixed(0, 0), Err(MathError::DomainError));
    assert_eq!(pow_fixed(0, -FIXED_SCALE), Err(MathError::DomainError));
}

#[test]
fn pow_fixed_vs_f64_oracle() {
    let mut rng = Rng::new(90210);
    for _ in 0..3_000 {
        let base = rng.range_i128(FIXED_SCALE / 100, 100 * FIXED_SCALE); // 0.01 .. 100
        let exp = rng.range_i128(-3 * FIXED_SCALE, 3 * FIXED_SCALE); // −3 .. 3
        let got = pow_fixed(base, exp).unwrap();
        let want = f64_to_wad(wad_to_f64(base).powf(wad_to_f64(exp)));
        // Composition of ln+exp: 1e-9 relative is comfortably above the bound.
        assert_rel_close(got, want, 1_000_000_000, 4, "pow oracle");
    }
}

// ------------------------------------------------- csemm preview: u(α)

/// End-to-end sanity for the primitives feeding csemm's `u(x) = ln2 / ln(x/(x−1))`:
/// the golden identity u(2+√2) = 2 (docs/INVARIANT_MATH.md §3).
#[test]
fn u_of_2_plus_sqrt2_is_2() {
    let alpha: i128 = 3_414_213_562_373_095_049; // 2+√2 in WAD
    let ratio = mul_div(alpha, FIXED_SCALE, alpha - FIXED_SCALE, Rounding::Down).unwrap();
    let ln_ratio = ln_fixed(ratio).unwrap();
    let u = mul_div(LN2, FIXED_SCALE, ln_ratio, Rounding::Down).unwrap();
    assert_close(u, 2 * FIXED_SCALE, 10_000_000, "u(2+√2) = 2"); // ≤ 1e-11
}

/// And the boundary identity u(2) = 1 (exact ln2/ln2).
#[test]
fn u_of_2_is_1() {
    let two = 2 * FIXED_SCALE;
    let ratio = mul_div(two, FIXED_SCALE, two - FIXED_SCALE, Rounding::Down).unwrap();
    assert_eq!(ratio, 2 * FIXED_SCALE); // 2/(2−1) = 2 exactly
    let u = mul_div(LN2, FIXED_SCALE, ln_fixed(ratio).unwrap(), Rounding::Down).unwrap();
    assert_eq!(u, FIXED_SCALE); // ln2/ln2 = 1 exactly
}
