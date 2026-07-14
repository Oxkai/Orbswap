//! Integration tests for `csemm.rs` — every edge case from todo.md §1.3.
//!
//! The f64 reference oracle mirrors docs/INVARIANT_MATH.md and is independent of
//! the integer implementation. All reserves/params are WAD.

mod common;

use common::*;
use orbswap_math::ccmm;
use orbswap_math::csemm::{invariant_holds, spot_price, swap_in, swap_out, u, CsemmError};
use orbswap_math::fixed_point::FIXED_SCALE;

const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049; // 2+√2 in WAD

// ---- f64 oracle (independent reference) -------------------------------------

fn u_f(a: f64) -> f64 {
    2f64.ln() / (a / (a - 1.0)).ln()
}

/// Partner reserve `B·[1 − (1 − (1 − a/A)^{u_a})^{1/u_b}]` in real arithmetic.
fn partner_f(a: f64, cap_a: f64, scale_b: f64, u_a: f64, u_b: f64) -> f64 {
    let base = 1.0 - a / cap_a;
    let term = base.powf(u_a);
    let inner = 1.0 - term;
    scale_b * (1.0 - inner.powf(1.0 / u_b))
}

fn y_of_x_f(x: f64, alpha: f64, beta: f64) -> f64 {
    partner_f(x, alpha, beta, u_f(alpha), u_f(beta))
}

// ---------------------------------------------------------------- u(x)

#[test]
fn u_golden_values() {
    // u(2) = 1 exactly (ln2/ln2).
    assert_eq!(u(2 * FIXED_SCALE).unwrap(), FIXED_SCALE);
    // u(2+√2) = 2 within transcendental precision.
    assert_close(
        u(TWO_PLUS_SQRT2).unwrap(),
        2 * FIXED_SCALE,
        10_000_000,
        "u(2+√2)=2",
    );
    // u(10) ≈ 6.578813, u(100) ≈ 68.967564.
    assert_rel_close(
        u(10 * FIXED_SCALE).unwrap(),
        f64_to_wad(u_f(10.0)),
        100_000_000,
        4,
        "u(10)",
    );
    assert_rel_close(
        u(100 * FIXED_SCALE).unwrap(),
        f64_to_wad(u_f(100.0)),
        100_000_000,
        4,
        "u(100)",
    );
}

#[test]
fn u_domain_rejects_below_two() {
    assert_eq!(u(FIXED_SCALE), Err(CsemmError::DomainError)); // x = 1
    assert_eq!(u(FIXED_SCALE + 1), Err(CsemmError::DomainError)); // just above 1
    assert_eq!(u(3 * FIXED_SCALE / 2), Err(CsemmError::DomainError)); // x = 1.5 ∈ (1,2)
    assert_eq!(u(2 * FIXED_SCALE - 1), Err(CsemmError::DomainError)); // just below 2
    assert_eq!(u(0), Err(CsemmError::DomainError));
    assert_eq!(u(-FIXED_SCALE), Err(CsemmError::DomainError));
}

#[test]
fn u_monotonic_increasing() {
    // u grows with α (toward ∞).
    let mut prev = 0;
    for a in 2..=200 {
        let got = u(a * FIXED_SCALE).unwrap();
        assert!(got >= prev, "u not monotonic at α={a}");
        prev = got;
    }
}

#[test]
fn u_huge_alpha_no_panic() {
    // Very large α → large u; astronomically large → Overflow, never a panic.
    assert!(u(1_000_000 * FIXED_SCALE).is_ok());
    // α so large that ln(α/(α−1)) underflows toward 0 → Overflow.
    let r = u(i128::MAX);
    assert!(r == Err(CsemmError::Overflow) || r.is_ok());
}

// ---------------------------------------------------------------- endpoints

#[test]
fn arc_endpoints() {
    // x=0 → y=β; x=α → y=0 (asymmetric α=3, β=5).
    let (a, b) = (3 * FIXED_SCALE, 5 * FIXED_SCALE);
    // At x=0 a full-arc swap of α lands at (α, 0), out = β.
    let (out, nx, ny) = swap_out(0, b, a, b, a).unwrap();
    assert_eq!(nx, a);
    assert_close(ny, 0, 4, "y=0 at x=α");
    assert_close(out, b, 4, "out=β over full arc");
}

// ---------------------------------------------------------------- invariant

#[test]
fn invariant_holds_on_curve() {
    // Construct on-curve points via the f64 oracle; the integer invariant must
    // hold within transcendental epsilon (~1e8 WAD = 1e-10 relative).
    let eps = 100_000_000; // 1e8
    for &(a, b) in &[(3.0, 5.0), (2.5, 2.5), (10.0, 4.0), (3.4142135, 3.4142135)] {
        let (alpha, beta) = (f64_to_wad(a), f64_to_wad(b));
        for i in 0..=20 {
            let x = f64_to_wad(a * i as f64 / 20.0).clamp(0, alpha);
            let y = f64_to_wad(y_of_x_f(x as f64 / 1e18, a, b)).clamp(0, beta);
            assert!(
                invariant_holds(x, y, alpha, beta, eps),
                "off-curve α={a} β={b} x={x} y={y}"
            );
        }
    }
}

#[test]
fn invariant_rejects_off_curve() {
    let (a, b) = (3 * FIXED_SCALE, 5 * FIXED_SCALE);
    // Center-ish point that is not on the curve.
    assert!(!invariant_holds(a / 2, b / 2, a, b, 1_000_000));
    assert!(!invariant_holds(0, 0, a, b, 1_000_000));
}

// ---------------------------------------------------------------- swaps vs oracle

#[test]
fn swap_out_matches_oracle_asymmetric() {
    let mut rng = Rng::new(0x5E11);
    let mut checked = 0;
    for _ in 0..5_000 {
        let a = 2.0 + (rng.range_i128(0, 8_000) as f64) / 1000.0; // 2..10
        let b = 2.0 + (rng.range_i128(0, 8_000) as f64) / 1000.0;
        let (alpha, beta) = (f64_to_wad(a), f64_to_wad(b));
        // On-curve start at x0.
        let x0f = a * (rng.range_i128(0, 900) as f64) / 1000.0; // 0..0.9α
        let x0 = f64_to_wad(x0f);
        let y0 = f64_to_wad(y_of_x_f(x0f, a, b)).clamp(0, beta);
        let dxf = (a - x0f) * (rng.range_i128(1, 900) as f64) / 1000.0;
        let dx = f64_to_wad(dxf);
        if dx <= 0 || x0 + dx > alpha {
            continue;
        }
        let (out, nx, _) = swap_out(x0, y0, alpha, beta, dx).unwrap();
        let want_ny = y_of_x_f((nx as f64) / 1e18, a, b);
        let want_out = (y0 as f64) / 1e18 - want_ny;
        // 1e-7 relative accuracy vs the f64 oracle (transcendental + rounding).
        assert!(
            ((out as f64) / 1e18 - want_out).abs() <= want_out.abs() * 1e-7 + 1e-9,
            "swap_out oracle: got {} want {want_out} (α={a} β={b} x0={x0f} dx={dxf})",
            (out as f64) / 1e18
        );
        checked += 1;
    }
    assert!(checked > 3_000, "too few cases exercised: {checked}");
}

#[test]
fn swap_in_matches_oracle_reversed_u() {
    // Eq. 9: the reversed-u inverse must match the f64 oracle for asymmetric α≠β.
    let mut rng = Rng::new(0x1DEA);
    let mut checked = 0;
    for _ in 0..5_000 {
        let a = 2.0 + (rng.range_i128(0, 8_000) as f64) / 1000.0;
        let b = 2.0 + (rng.range_i128(0, 8_000) as f64) / 1000.0;
        let (alpha, beta) = (f64_to_wad(a), f64_to_wad(b));
        let x0f = a * (rng.range_i128(0, 800) as f64) / 1000.0;
        let x0 = f64_to_wad(x0f);
        let y0f = y_of_x_f(x0f, a, b);
        let y0 = f64_to_wad(y0f).clamp(0, beta);
        if y0 <= FIXED_SCALE / 1000 {
            continue;
        }
        let out = y0 / 3; // withdraw a third of Y
        let (in_req, _, ny) = swap_in(x0, y0, alpha, beta, out).unwrap();
        // Independent oracle: required x for the target new_y (reversed roles).
        let new_yf = (ny as f64) / 1e18;
        let want_x = partner_f(new_yf, b, a, u_f(b), u_f(a));
        let want_in = want_x - x0f;
        assert!(
            ((in_req as f64) / 1e18 - want_in).abs() <= want_in.abs() * 1e-7 + 1e-9,
            "swap_in oracle: got {} want {want_in} (α={a} β={b})",
            (in_req as f64) / 1e18
        );
        checked += 1;
    }
    assert!(checked > 3_000, "too few cases: {checked}");
}

#[test]
fn reversed_u_actually_matters() {
    // For α≠β, swapping the u-order (the naive bug) gives a materially different
    // answer — proving the reversal in swap_in is load-bearing, not cosmetic.
    let (a, b) = (3.0f64, 8.0f64);
    let x0f = 1.0;
    let y0f = y_of_x_f(x0f, a, b);
    let new_yf = y0f * 0.6;
    let correct = partner_f(new_yf, b, a, u_f(b), u_f(a)); // reversed (right)
    let naive = partner_f(new_yf, b, a, u_f(a), u_f(b)); // non-reversed (wrong)
    assert!(
        (correct - naive).abs() > 0.01,
        "reversed vs naive indistinguishable: {correct} vs {naive}"
    );
}

// ---------------------------------------------------------------- ladder

#[test]
fn ladder_equals_ccmm_at_2_plus_sqrt2() {
    // α=β=2+√2 ⇒ u=2 ⇒ the superellipse IS the circle of radius k=2+√2 (WAD).
    // Compare csemm::swap_out to ccmm::swap_out on identical WAD points.
    let k = TWO_PLUS_SQRT2;
    let mut rng = Rng::new(0x1ADDE4);
    let mut checked = 0;
    for _ in 0..3_000 {
        // On-circle start via ccmm from (0,k).
        let x0 = rng.range_i128(0, k - 1);
        let (_, _, y0) = match ccmm::swap_out(0, k, k, x0.max(1)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let y0 = if x0 == 0 { k } else { y0 };
        let dx = rng.range_i128(1, k - x0);
        let circle = ccmm::swap_out(x0, y0, k, dx);
        let ellipse = swap_out(x0, y0, k, k, dx);
        match (circle, ellipse) {
            (Ok((c_out, ..)), Ok((e_out, ..))) => {
                // Both ~exact; csemm carries transcendental error. Compare relative.
                let cf = c_out as f64;
                let ef = e_out as f64;
                assert!(
                    (cf - ef).abs() <= cf.abs() * 1e-8 + 1e6,
                    "ladder mismatch: ccmm {c_out} vs csemm {e_out} (x0={x0} dx={dx})"
                );
                checked += 1;
            }
            (Err(_), Err(_)) => {}
            (c, e) => panic!("ladder disagreement on validity: {c:?} vs {e:?}"),
        }
    }
    assert!(checked > 1_500, "too few ladder cases: {checked}");
}

// ---------------------------------------------------------------- degenerate / extreme

#[test]
fn degenerate_constant_sum_at_alpha_2() {
    // α=β=2, u=1 ⇒ constant sum x+y=2 ⇒ amount_out == amount_in (within precision).
    let two = 2 * FIXED_SCALE;
    // On-curve start: x0=0.5, y0 = 2 − 0.5 = 1.5.
    let x0 = FIXED_SCALE / 2;
    let y0 = 3 * FIXED_SCALE / 2;
    assert!(invariant_holds(x0, y0, two, two, 100_000_000));
    let dx = FIXED_SCALE / 4; // 0.25
    let (out, ..) = swap_out(x0, y0, two, two, dx).unwrap();
    assert!(
        ((out - dx).abs() as f64) <= (dx as f64) * 1e-7 + 1e6,
        "constant-sum: out={out} != dx={dx}"
    );
}

#[test]
fn extreme_asymmetry_no_panic() {
    // α=2 (linear side), β=100 (boxy side): must run without overflow/panic.
    let (a, b) = (2 * FIXED_SCALE, 100 * FIXED_SCALE);
    let y0 = b / 2;
    // find an on-curve x for y0 via a small swap_in-free construction: just swap.
    let r = swap_out(FIXED_SCALE / 2, y0, a, b, FIXED_SCALE / 4);
    assert!(
        r.is_ok() || matches!(r, Err(CsemmError::OutOfRange)),
        "unexpected {r:?}"
    );
}

// ---------------------------------------------------------------- negative base / errors

#[test]
fn never_pow_negative_base() {
    // x > α is off the arc; the impl must reject via OutOfRange, never feed pow a
    // negative base (which would be a silent NaN-equivalent).
    let (a, b) = (3 * FIXED_SCALE, 5 * FIXED_SCALE);
    assert_eq!(
        swap_out(4 * FIXED_SCALE, b / 2, a, b, 1),
        Err(CsemmError::OutOfRange)
    );
    assert_eq!(
        swap_out(a / 2, 6 * FIXED_SCALE, a, b, 1),
        Err(CsemmError::OutOfRange)
    );
}

#[test]
fn error_paths() {
    let (a, b) = (3 * FIXED_SCALE, 5 * FIXED_SCALE);
    // amount ≤ 0
    assert_eq!(
        swap_out(FIXED_SCALE, 2 * FIXED_SCALE, a, b, 0),
        Err(CsemmError::InvalidAmount)
    );
    assert_eq!(
        swap_in(FIXED_SCALE, 2 * FIXED_SCALE, a, b, -1),
        Err(CsemmError::InvalidAmount)
    );
    // shape < 2
    assert_eq!(
        swap_out(0, 0, FIXED_SCALE, b, 1),
        Err(CsemmError::DomainError)
    );
    // new_x > α (fold)
    assert_eq!(
        swap_out(2 * FIXED_SCALE, FIXED_SCALE, a, b, 2 * FIXED_SCALE),
        Err(CsemmError::PriceOutOfRange)
    );
    // over-withdraw (amount_out > y)
    assert_eq!(
        swap_in(FIXED_SCALE, 2 * FIXED_SCALE, a, b, 3 * FIXED_SCALE),
        Err(CsemmError::OutOfRange)
    );
}

// ---------------------------------------------------------------- spot_price

#[test]
fn spot_price_symmetric_balanced_is_one() {
    // Symmetric α=β, balanced x=y ⇒ price = 1.
    let k = TWO_PLUS_SQRT2;
    let x = k - k / 3; // some x<k; pick y=x (balanced) — price must be 1
    assert_close(
        spot_price(x, x, k, k),
        FIXED_SCALE,
        100_000_000,
        "balanced price=1",
    );
}

#[test]
fn spot_price_boundaries() {
    let (a, b) = (3 * FIXED_SCALE, 5 * FIXED_SCALE);
    assert_eq!(spot_price(a, 0, a, b), 0); // x=α → price 0
    assert_eq!(spot_price(0, b, a, b), i128::MAX); // y=β → +∞
}

#[test]
fn spot_price_matches_ccmm_on_ladder() {
    // At α=β=2+√2, csemm spot price = ccmm spot price on the same point.
    let k = TWO_PLUS_SQRT2;
    let x0 = k / 3;
    let (_, _, y0) = ccmm::swap_out(0, k, k, x0).unwrap();
    let ps_csemm = spot_price(x0, y0, k, k);
    let ps_ccmm = ccmm::spot_price(x0, y0, k);
    assert_rel_close(ps_csemm, ps_ccmm, 1_000_000, 1_000_000, "spot price ladder");
}
