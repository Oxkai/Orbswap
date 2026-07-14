//! Concentrated-liquidity position math for the **Circular (CCMM)** pool — the
//! contract-level counterpart to `ticks.rs`/`polar.rs`. Everything here is pure,
//! `no_std`, and float-free; angles are integer degrees on the arc `[0, 90]`
//! (0 = all Y, 90 = all X, 45 = balanced), and `cos/sin` come from the polar table.
//!
//! Geometry (see `docs/TICK_DESIGN.md`): reserves on the arc are
//! `x(θ) = L·(1 − cos θ)`, `y(θ) = L·(1 − sin θ)`, so a position of liquidity `L`
//! over `[lower, upper]` at the current angle `θc` holds, in internal (WAD) units:
//!
//! ```text
//! θ* = clamp(θc, lower, upper)
//! x  = L · (cos lower − cos θ*)      (token X, held above θ*)
//! y  = L · (sin upper − sin θ*)      (token Y, held below θ*)
//! ```
//!
//! `L` and the returned amounts are WAD-scaled (18-dec internal units).

use crate::fixed_point::{isqrt_wide, mul_div, MathError, Rounding, FIXED_SCALE};
use crate::polar::{cos_deg, sin_deg};

/// Errors from circular-liquidity position math. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircleLiqError {
    /// `lower`/`upper` outside `[0, 90]` or `lower >= upper`.
    InvalidRange,
    /// A provided amount was negative, or the derived liquidity was zero.
    InvalidAmount,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

impl From<MathError> for CircleLiqError {
    fn from(e: MathError) -> Self {
        match e {
            MathError::Overflow => CircleLiqError::Overflow,
            _ => CircleLiqError::InvalidAmount,
        }
    }
}

const MIN_TICK: i128 = 0;
const MAX_TICK: i128 = 90;

#[inline]
fn clamp(theta: i128, lo: i128, hi: i128) -> i128 {
    if theta < lo {
        lo
    } else if theta > hi {
        hi
    } else {
        theta
    }
}

#[inline]
fn check_range(lower: i128, upper: i128) -> Result<(), CircleLiqError> {
    if !(MIN_TICK..=MAX_TICK).contains(&lower)
        || !(MIN_TICK..=MAX_TICK).contains(&upper)
        || lower >= upper
    {
        return Err(CircleLiqError::InvalidRange);
    }
    Ok(())
}

/// The `(x, y)` internal (WAD) reserves a position of liquidity `l` over
/// `[lower, upper]` holds at the current angle `theta_c`. `rnd` picks the rounding
/// (Down for withdraw/quote — favors the pool; Up for the deposit pull — favors the
/// pool). Single-sided outside the range (one amount is 0).
pub fn position_amounts(
    l: i128,
    lower: i128,
    upper: i128,
    theta_c: i128,
    rnd: Rounding,
) -> Result<(i128, i128), CircleLiqError> {
    check_range(lower, upper)?;
    if l < 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    let tc = clamp(theta_c, lower, upper);
    // cos is decreasing, sin increasing on [0,90] ⇒ both deltas are ≥ 0.
    let dcos = cos_deg(lower) - cos_deg(tc);
    let dsin = sin_deg(upper) - sin_deg(tc);
    let x = mul_div(l, dcos, FIXED_SCALE, rnd)?;
    let y = mul_div(l, dsin, FIXED_SCALE, rnd)?;
    Ok((x, y))
}

/// The largest liquidity `l` whose [`position_amounts`] fits within the provided
/// `(x, y)` at `theta_c` — the deposit sizing. `l` rounds **down** (pool-favoring),
/// so re-pulling `position_amounts(l, …, Up)` never exceeds the input; the caller
/// refunds the dust. Returns `InvalidAmount` if the result is 0.
pub fn liquidity_for(
    x: i128,
    y: i128,
    lower: i128,
    upper: i128,
    theta_c: i128,
) -> Result<i128, CircleLiqError> {
    check_range(lower, upper)?;
    if x < 0 || y < 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    let tc = clamp(theta_c, lower, upper);
    let dcos = cos_deg(lower) - cos_deg(tc);
    let dsin = sin_deg(upper) - sin_deg(tc);

    // l ≤ x·WAD/dcos and l ≤ y·WAD/dsin. A zero delta means that side is
    // unconstrained (the position is single-sided there).
    let lx = if dcos > 0 {
        mul_div(x, FIXED_SCALE, dcos, Rounding::Down)?
    } else {
        i128::MAX
    };
    let ly = if dsin > 0 {
        mul_div(y, FIXED_SCALE, dsin, Rounding::Down)?
    } else {
        i128::MAX
    };
    let l = if lx < ly { lx } else { ly };
    if l <= 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    Ok(l)
}

/// Like [`position_amounts`], but the current angle is given by its `cos`/`sin`
/// (WAD, on the unit circle) instead of an integer degree — so it works at any
/// price a swap has moved to (the table is only used for the integer tick bounds).
pub fn position_amounts_cs(
    l: i128,
    lower: i128,
    upper: i128,
    cos_c: i128,
    sin_c: i128,
    rnd: Rounding,
) -> Result<(i128, i128), CircleLiqError> {
    check_range(lower, upper)?;
    if l < 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    let (cl, cu) = (cos_deg(lower), cos_deg(upper)); // cl ≥ cu (cos decreasing)
    let (sl, su) = (sin_deg(lower), sin_deg(upper)); // su ≥ sl (sin increasing)
                                                     // Clamp the price into [lower, upper] in cos-space:
                                                     //   cos_c ≥ cl ⇔ θc ≤ lower ;  cos_c ≤ cu ⇔ θc ≥ upper.
    let (cc, sc) = if cos_c >= cl {
        (cl, sl)
    } else if cos_c <= cu {
        (cu, su)
    } else {
        (cos_c, sin_c)
    };
    let x = mul_div(l, cl - cc, FIXED_SCALE, rnd)?; // cl − cc ≥ 0
    let y = mul_div(l, su - sc, FIXED_SCALE, rnd)?; // su − sc ≥ 0
    Ok((x, y))
}

/// Deposit sizing at a cos/sin price (the [`liquidity_for`] analog). Rounds `l`
/// down (pool-favoring).
pub fn liquidity_for_cs(
    x: i128,
    y: i128,
    lower: i128,
    upper: i128,
    cos_c: i128,
    sin_c: i128,
) -> Result<i128, CircleLiqError> {
    check_range(lower, upper)?;
    if x < 0 || y < 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    let (cl, cu) = (cos_deg(lower), cos_deg(upper));
    let (sl, su) = (sin_deg(lower), sin_deg(upper));
    let (cc, sc) = if cos_c >= cl {
        (cl, sl)
    } else if cos_c <= cu {
        (cu, su)
    } else {
        (cos_c, sin_c)
    };
    let dcos = cl - cc;
    let dsin = su - sc;
    let lx = if dcos > 0 {
        mul_div(x, FIXED_SCALE, dcos, Rounding::Down)?
    } else {
        i128::MAX
    };
    let ly = if dsin > 0 {
        mul_div(y, FIXED_SCALE, dsin, Rounding::Down)?
    } else {
        i128::MAX
    };
    let l = if lx < ly { lx } else { ly };
    if l <= 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    Ok(l)
}

/// One swap step within a segment of **constant** active liquidity `l`, on the
/// circle of radius `l` centered at `(l, l)`. `xv`, `yv` are the segment's virtual
/// reserves (`0 ≤ xv,yv ≤ l`, on the circle). `amount_in` is the internal input of
/// the token sold. `x_for_y = true` sells X (`xv` rises, out = Y); `false` sells Y.
///
/// Returns `(out, new_xv, new_yv)`. This is `ccmm::swap_out` generalized to any
/// `l` via a **256-bit radicand `isqrt`** ([`isqrt_wide`]) — the paired reserve is
/// an over-estimate (root floors), so the output rounds **down** and a round trip
/// can never profit. Errors if the input would push a reserve past the radius
/// (arc end); the caller splits at ticks so a step never overshoots.
pub fn swap_step(
    xv: i128,
    yv: i128,
    l: i128,
    amount_in: i128,
    x_for_y: bool,
) -> Result<(i128, i128, i128), CircleLiqError> {
    if l <= 0 || amount_in < 0 || xv < 0 || yv < 0 || xv > l || yv > l {
        return Err(CircleLiqError::InvalidAmount);
    }
    // The token being sold grows exactly by `amount_in`; solve the circle for its
    // paired reserve: on (a−l)²+(b−l)²=l², b = l − √(a·(2l − a)).
    let (a_new, in_res, out_res) = if x_for_y {
        (xv + amount_in, xv, yv)
    } else {
        (yv + amount_in, yv, xv)
    };
    let _ = in_res;
    if a_new > l {
        return Err(CircleLiqError::InvalidAmount); // past the arc end
    }
    let two_l_minus_a = 2 * l - a_new; // ≥ 0 since a_new ≤ l
    let root = isqrt_wide(a_new as u128, two_l_minus_a as u128); // floor ⇒ b over-estimate
    let b_new = l - root as i128; // paired reserve, ≥ true ⇒ out ≤ true (pool-favoring)
    let out = out_res - b_new;
    if out < 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    let (new_xv, new_yv) = if x_for_y {
        (a_new, b_new)
    } else {
        (b_new, a_new)
    };
    Ok((out, new_xv, new_yv))
}

/// Consistent on-circle virtual reserves at price `(cos, sin)` (WAD) and liquidity
/// `l`. The reserve of the token being **sold** is taken from the price; its pair is
/// derived as the exact paired reserve, so the resulting swap output is exact.
/// `x_for_y = true` seeds a sell-X swap (X from `cos`, Y = paired).
pub fn reserves_from_price(
    l: i128,
    cos: i128,
    sin: i128,
    x_for_y: bool,
) -> Result<(i128, i128), CircleLiqError> {
    if l < 0 {
        return Err(CircleLiqError::InvalidAmount);
    }
    let clampl = |v: i128| {
        if v < 0 {
            0
        } else if v > l {
            l
        } else {
            v
        }
    };
    if x_for_y {
        let xv = clampl(l - mul_div(l, cos, FIXED_SCALE, Rounding::Up)?);
        let root = isqrt_wide(xv as u128, (2 * l - xv) as u128);
        Ok((xv, l - root as i128))
    } else {
        let yv = clampl(l - mul_div(l, sin, FIXED_SCALE, Rounding::Up)?);
        let root = isqrt_wide(yv as u128, (2 * l - yv) as u128);
        Ok((l - root as i128, yv))
    }
}

/// Virtual reserves `(xv, yv)` of a segment of liquidity `l` at angle `θ` (integer
/// degree): `xv = l(1 − cos θ)`, `yv = l(1 − sin θ)`. Used to (re)seed the swap
/// state at a tick boundary after `l` changes.
pub fn reserves_at(l: i128, theta: i128) -> Result<(i128, i128), CircleLiqError> {
    if l < 0 || !(0..=90).contains(&theta) {
        return Err(CircleLiqError::InvalidAmount);
    }
    // Take xv from the angle (rounded to inside the arc), then derive yv as its
    // EXACT paired reserve on the circle — so the seed is a consistent on-circle
    // point (independent rounding of both would sit off-curve and be exploitable).
    let mut xv = l - mul_div(l, cos_deg(theta), FIXED_SCALE, Rounding::Up)?;
    if xv < 0 {
        xv = 0;
    }
    if xv > l {
        xv = l;
    }
    let root = isqrt_wide(xv as u128, (2 * l - xv) as u128);
    let yv = l - root as i128; // paired reserve ⇒ (xv,yv) on the circle
    Ok((xv, yv))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may unwrap; the deny applies to library paths
mod tests {
    use super::*;
    use crate::ccmm;

    // f64 oracle for x(θ),y(θ) contributions of a unit-L position.
    fn oracle(l: f64, lower: i128, upper: i128, tc: i128) -> (f64, f64) {
        let t = (tc.max(lower).min(upper)) as f64;
        let d2r = core::f64::consts::PI / 180.0;
        let x = l * ((lower as f64 * d2r).cos() - (t * d2r).cos());
        let y = l * ((upper as f64 * d2r).sin() - (t * d2r).sin());
        (x, y)
    }

    const WAD: i128 = FIXED_SCALE;

    #[test]
    fn balanced_full_range_is_symmetric() {
        // Full range [0,90] at balance (45°): x == y, and each = L·(1 − cos45).
        let l = 1000 * WAD;
        let (x, y) = position_amounts(l, 0, 90, 45, Rounding::Down).unwrap();
        // cos(45) and sin(45) table entries differ by ~1 ULP, so x≈y (not bit-equal).
        assert!(
            (x - y).abs() < WAD / 1000,
            "balanced full-range not symmetric: {x} vs {y}"
        );
        // 1 − cos45 ≈ 0.29289 → ~292.89 * WAD
        let (ox, _) = oracle(1000.0, 0, 90, 45);
        let got = x as f64 / WAD as f64;
        assert!((got - ox).abs() < 0.01, "x={got} oracle={ox}");
    }

    #[test]
    fn single_sided_outside_range() {
        let l = 500 * WAD;
        // θc below range → all Y (x == 0).
        let (x, y) = position_amounts(l, 40, 50, 30, Rounding::Down).unwrap();
        assert_eq!(x, 0);
        assert!(y > 0);
        // θc above range → all X (y == 0).
        let (x2, y2) = position_amounts(l, 40, 50, 60, Rounding::Down).unwrap();
        assert!(x2 > 0);
        assert_eq!(y2, 0);
    }

    #[test]
    fn matches_f64_oracle_across_ranges() {
        for &(lo, hi, tc) in &[(0, 90, 45), (30, 60, 45), (40, 50, 44), (10, 80, 70)] {
            let l = 12_345 * WAD;
            let (x, y) = position_amounts(l, lo, hi, tc, Rounding::Down).unwrap();
            let (ox, oy) = oracle(12_345.0, lo, hi, tc);
            let gx = x as f64 / WAD as f64;
            let gy = y as f64 / WAD as f64;
            assert!((gx - ox).abs() < 0.05, "x {gx} vs {ox} @ {lo},{hi},{tc}");
            assert!((gy - oy).abs() < 0.05, "y {gy} vs {oy} @ {lo},{hi},{tc}");
        }
    }

    #[test]
    fn liquidity_roundtrips_le_input() {
        // liquidity_for then position_amounts(Up) must not exceed the input.
        for &(lo, hi, tc) in &[(0, 90, 45), (30, 60, 45), (44, 46, 45)] {
            let (xin, yin) = (700 * WAD, 700 * WAD);
            let l = liquidity_for(xin, yin, lo, hi, tc).unwrap();
            let (x, y) = position_amounts(l, lo, hi, tc, Rounding::Up).unwrap();
            assert!(x <= xin && y <= yin, "pull {x},{y} > input {xin},{yin}");
            // and it should consume almost all of the binding side
            assert!(x > xin - WAD || y > yin - WAD, "left too much on the table");
        }
    }

    #[test]
    fn deposit_then_withdraw_conserves() {
        // Deposit L at θc, withdraw same L at same θc (Down) → get back ≤ deposited.
        let l = liquidity_for(1000 * WAD, 1000 * WAD, 30, 60, 45).unwrap();
        let (dx, dy) = position_amounts(l, 30, 60, 45, Rounding::Up).unwrap(); // pulled in
        let (wx, wy) = position_amounts(l, 30, 60, 45, Rounding::Down).unwrap(); // paid out
        assert!(
            wx <= dx && wy <= dy,
            "withdraw {wx},{wy} > deposit {dx},{dy}"
        );
    }

    #[test]
    fn error_paths() {
        assert_eq!(
            position_amounts(WAD, 50, 40, 45, Rounding::Down),
            Err(CircleLiqError::InvalidRange)
        );
        assert_eq!(
            position_amounts(WAD, 0, 91, 45, Rounding::Down),
            Err(CircleLiqError::InvalidRange)
        );
        assert_eq!(
            position_amounts(-1, 0, 90, 45, Rounding::Down),
            Err(CircleLiqError::InvalidAmount)
        );
        assert_eq!(
            liquidity_for(0, 0, 0, 90, 45),
            Err(CircleLiqError::InvalidAmount)
        );
    }

    // M4 golden regression: for L in ccmm's overflow-safe range, swap_step must
    // equal ccmm::swap_out exactly — the tick engine reduces to today's math.
    #[test]
    fn swap_step_equals_ccmm() {
        for &theta in &[30i128, 45, 60] {
            let l = 1_000_000_000i128; // safe for ccmm (l² < i128::MAX)
            let (xv, yv) = reserves_at(l, theta).unwrap();
            for &dx in &[1_000i128, 1_000_000, 100_000_000] {
                let (co, cx, cy) = ccmm::swap_out(xv, yv, l, dx).unwrap();
                let (so, sx, sy) = swap_step(xv, yv, l, dx, true).unwrap();
                assert_eq!(so, co, "out mismatch @θ{theta} dx{dx}");
                assert_eq!((sx, sy), (cx, cy), "reserve mismatch");
            }
        }
    }

    #[test]
    fn swap_step_large_l_no_overflow() {
        // L far beyond ccmm's i128 radicand range (l² ≈ 1e48) — the whole reason for
        // the wide radicand. Must produce a sane, pool-favoring output.
        let l = 1_000_000_000_000_000_000_000_000i128; // 1e24
        assert!(ccmm::swap_out(l / 4, l / 4, l, l / 100).is_err()); // ccmm overflows here
        let (xv, yv) = reserves_at(l, 45).unwrap();
        let dx = l / 1000;
        let (out, nx, ny) = swap_step(xv, yv, l, dx, true).unwrap();
        assert!(out > 0 && out < dx, "out {out} not in (0,dx)");
        assert_eq!(nx, xv + dx, "input reserve moves exactly");
        assert!(ny >= 0 && ny <= l, "paired reserve on arc");
    }

    #[test]
    fn swap_roundtrip_never_profits() {
        // X→Y then Y→X returns ≤ start, at both small and huge L.
        for &l in &[500_000_000i128, 1_000_000_000_000_000_000_000_000] {
            let (xv, yv) = reserves_at(l, 45).unwrap();
            let dx = l / 1000;
            let (out_y, x1, y1) = swap_step(xv, yv, l, dx, true).unwrap();
            let (back_x, _, _) = swap_step(x1, y1, l, out_y, false).unwrap();
            assert!(
                back_x <= dx,
                "round trip profited at l={l}: {back_x} > {dx}"
            );
        }
    }

    #[test]
    fn swap_roundtrip_property() {
        // Randomized X→Y→X across a wide L/θ/size range: never profits, always stays
        // on-or-inside the circle. Deterministic SplitMix64.
        let mut st: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            st = st.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = st;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };
        for _ in 0..5000 {
            // L from ~1e9 up past ccmm's overflow range (~1e26).
            let l =
                ((next() % 1_000_000) as i128 + 1) * 1_000_000_000_000_000_000_000 + 1_000_000_000;
            let theta = (next() % 89 + 1) as i128; // 1..=89
            let (xv, yv) = reserves_at(l, theta).unwrap();
            let room = l - xv;
            if room <= 1 {
                continue;
            }
            let dx = ((next() as i128).unsigned_abs() as i128 % room).max(1);
            let (out_y, x1, y1) = match swap_step(xv, yv, l, dx, true) {
                Ok(v) => v,
                Err(_) => continue,
            };
            // The swapped point stays on-or-inside the circle (paired reserve floors)
            // — the true value-conservation invariant.
            assert!(x1 >= 0 && x1 <= l && y1 >= 0 && y1 <= l, "off arc");
            // Reverse can recover at most a few ULP of dust (the two directions' floors
            // aren't symmetric when the state is seeded via `reserves_at`). The dust is
            // O(1) units — ≪ 1e-9 native token — so it is never economically drainable
            // (gas per swap ≫ the dust). M5b seeds swap state only from swap_step
            // outputs (consistent), so the live pool never even hits this seed path.
            if let Ok((back, _, _)) = swap_step(x1, y1, l, out_y, false) {
                assert!(
                    back <= dx + 64,
                    "profit l={l} θ={theta} dx={dx}: {back} > {dx}"
                );
            }
        }
    }

    #[test]
    fn cs_variants_match_integer_at_ticks() {
        // position_amounts_cs at (cos θ, sin θ) must equal position_amounts(θ).
        for &(lo, hi, tc) in &[(0i128, 90, 45), (30, 60, 45), (40, 50, 44), (10, 80, 70)] {
            let l = 12_345 * WAD;
            let (xi, yi) = position_amounts(l, lo, hi, tc, Rounding::Down).unwrap();
            let (xc, yc) =
                position_amounts_cs(l, lo, hi, cos_deg(tc), sin_deg(tc), Rounding::Down).unwrap();
            assert_eq!((xi, yi), (xc, yc), "cs mismatch @ {lo},{hi},{tc}");
        }
    }

    #[test]
    fn swap_step_errors_past_arc() {
        let l = 1_000_000_000i128;
        let (xv, yv) = reserves_at(l, 80).unwrap();
        // Selling far more X than the arc holds → past the radius.
        assert_eq!(
            swap_step(xv, yv, l, l, true),
            Err(CircleLiqError::InvalidAmount)
        );
    }
}
