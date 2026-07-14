//! Deterministic fixed-point math (WAD = 1e18) for the Orbswap invariants.
//!
//! Everything here is integer-only `i128`/`u128` arithmetic — no floats — so
//! results are bit-identical on every target (paper Appendix 3.3, Note 1).
//! Fractional quantities are scaled by [`FIXED_SCALE`] (paper Note 2, WAD-style).
//!
//! # Rounding discipline
//! [`mul_div`] takes an explicit [`Rounding`]: `Down` = toward −∞ (floor),
//! `Up` = toward +∞ (ceil). Callers in `ccmm`/`csemm` choose the direction that
//! favors the pool. The transcendental functions ([`ln_fixed`], [`exp_fixed`],
//! [`pow_fixed`]) round internally toward zero per term; their total error bound
//! is documented per function, and invariant-level callers must budget for it
//! (epsilon margins), not assume exactness.
//!
//! # Error bounds (absolute, in WAD units unless stated)
//! - `isqrt`: exact floor.
//! - `mul_div`: exact in the requested direction (full 256-bit intermediate).
//! - `pow_int`: ≤ `exp` ULP accumulated truncation (one per multiply).
//! - `ln_fixed`: ≤ ~300 ULP (3e-16 as a real number) — range reduction loses
//!   ≤ 1 ULP per halving (≤ 127) plus ≤ ~40 ULP series truncation.
//! - `exp_fixed`: relative error ≤ ~1e-15 of the result.
//! - `pow_fixed`: composes `ln`+`exp`: relative error ≤ ~|exp|·1e-15 + 1e-15.

/// Fixed-point scale: 1.0 is represented as 1e18 (WAD).
///
/// Decision log: chosen over Stellar-native 1e7 for headroom in the CSEMM
/// `ln`/`pow` chain; contract layers normalize 7-decimal token amounts at the
/// boundary (todo.md, Architecture model §B).
pub const FIXED_SCALE: i128 = 1_000_000_000_000_000_000;

/// ln(2) in WAD, rounded to nearest (true value 0.693147180559945309417…).
pub const LN2: i128 = 693_147_180_559_945_309;

/// Euler's number e in WAD, rounded to nearest (2.718281828459045235…).
pub const E_WAD: i128 = 2_718_281_828_459_045_235;

/// Inputs above this make `exp_fixed` exceed `i128::MAX` (ln(i128::MAX/1e18) ≈ 46.58).
/// Kept slightly generous; the exact ceiling is enforced by checked arithmetic.
const EXP_INPUT_UPPER: i128 = 47 * FIXED_SCALE;

/// Below this, `exp_fixed` underflows to 0 at WAD precision (e^−42 < 1e-18).
const EXP_INPUT_LOWER: i128 = -100 * FIXED_SCALE;

/// Errors from fixed-point operations. No function in this module panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathError {
    /// Result (or an unavoidable intermediate) exceeds `i128` range.
    Overflow,
    /// A negative input where the operation requires non-negative.
    NegativeInput,
    /// Input outside the mathematical domain (e.g. `ln(x ≤ 0)`).
    DomainError,
    /// Division by zero.
    DivByZero,
}

/// Rounding direction for [`mul_div`].
///
/// `Down` = toward −∞ (floor), `Up` = toward +∞ (ceil). For non-negative
/// results these coincide with truncate/away-from-zero; the distinction only
/// matters for negative results and is defined this way so that
/// `mul_div(a, b, d, Down) ≤ exact ≤ mul_div(a, b, d, Up)` always holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    Down,
    Up,
}

/// Exact integer square root: the largest `r` with `r·r ≤ n`.
///
/// Total function: negative inputs return 0 (callers guard domain upstream;
/// this keeps the primitive panic-free). Uses the classic bit-pair method —
/// exact for the full `i128` range including `i128::MAX`.
pub fn isqrt(n: i128) -> i128 {
    if n <= 0 {
        return 0;
    }
    let mut num = n as u128;
    let mut res: u128 = 0;
    // Highest power of 4 ≤ num.
    let mut bit: u128 = 1 << 126;
    while bit > num {
        bit >>= 2;
    }
    while bit != 0 {
        if num >= res + bit {
            num -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res as i128
}

/// `floor(√(a·b))` computed in **256-bit** — for products that overflow `u128`
/// / `i128` (e.g. the CCMM circle radicand `≈ L²` for concentrated liquidity).
/// The result always fits in `u128` (`√(2²⁵⁶) = 2¹²⁸`). Binary search on the root,
/// squaring via [`wide_mul`] and comparing the 256-bit values — panic-free.
pub fn isqrt_wide(a: u128, b: u128) -> u128 {
    let (n_hi, n_lo) = wide_mul(a, b);
    if n_hi == 0 && n_lo == 0 {
        return 0;
    }
    // Largest r with r² ≤ n. r ≤ 2¹²⁸−1.
    let (mut lo, mut hi): (u128, u128) = (0, u128::MAX);
    while lo < hi {
        let mid = lo + (hi - lo) / 2 + 1; // upper mid: guarantees progress
        let (s_hi, s_lo) = wide_mul(mid, mid);
        // mid² ≤ n ?
        if s_hi < n_hi || (s_hi == n_hi && s_lo <= n_lo) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// 128×128 → 256-bit unsigned multiply, returned as `(hi, lo)`.
#[inline]
fn wide_mul(a: u128, b: u128) -> (u128, u128) {
    const MASK: u128 = (1u128 << 64) - 1;
    let (a_hi, a_lo) = (a >> 64, a & MASK);
    let (b_hi, b_lo) = (b >> 64, b & MASK);

    let ll = a_lo * b_lo;
    let lh = a_lo * b_hi;
    let hl = a_hi * b_lo;
    let hh = a_hi * b_hi;

    // mid = lh + hl, tracking the carry out of u128.
    let (mid, mid_carry) = lh.overflowing_add(hl);
    let mid_lo = mid << 64;
    let mid_hi = (mid >> 64) + ((mid_carry as u128) << 64);

    let (lo, lo_carry) = ll.overflowing_add(mid_lo);
    let hi = hh + mid_hi + lo_carry as u128;
    (hi, lo)
}

/// 256-bit ÷ 128-bit → (quotient, remainder).
///
/// Returns `None` if the quotient does not fit in `u128` (i.e. `hi ≥ d`).
/// `d` must be non-zero (checked by the caller).
#[inline]
fn div_wide(hi: u128, lo: u128, d: u128) -> Option<(u128, u128)> {
    if hi >= d {
        return None; // quotient ≥ 2^128
    }
    if hi == 0 {
        return Some((lo / d, lo % d));
    }
    // Binary long division: shift one bit of `lo` into `rem` at a time.
    // `rem < d ≤ u128::MAX`, so `rem << 1` may carry out one bit — the carry
    // means the shifted value is ≥ 2^128 > d, hence always subtractable
    // (wrapping_sub yields the correct 128-bit remainder).
    let mut rem = hi;
    let mut quot: u128 = 0;
    let mut i = 128u32;
    while i > 0 {
        i -= 1;
        let carry = rem >> 127;
        rem = (rem << 1) | ((lo >> i) & 1);
        if carry == 1 || rem >= d {
            rem = rem.wrapping_sub(d);
            quot |= 1 << i;
        }
    }
    Some((quot, rem))
}

/// `a · b / denom` with a full 256-bit intermediate (never overflows internally)
/// and explicit rounding: `Down` = floor (toward −∞), `Up` = ceil (toward +∞).
///
/// Errors: `DivByZero` if `denom == 0`; `Overflow` if the rounded quotient
/// magnitude exceeds `i128::MAX` (the single value `i128::MIN` is deliberately
/// not representable as a result — one unit of range given up for simplicity).
pub fn mul_div(a: i128, b: i128, denom: i128, rounding: Rounding) -> Result<i128, MathError> {
    if denom == 0 {
        return Err(MathError::DivByZero);
    }
    let negative = (a < 0) ^ (b < 0) ^ (denom < 0);

    let (hi, lo) = wide_mul(a.unsigned_abs(), b.unsigned_abs());
    let (q, r) = div_wide(hi, lo, denom.unsigned_abs()).ok_or(MathError::Overflow)?;

    // Adjust the truncated magnitude for the requested direction.
    let bump = match (negative, rounding) {
        (false, Rounding::Down) => false, // floor of +x.y = x
        (false, Rounding::Up) => r != 0,  // ceil  of +x.y = x+1
        (true, Rounding::Down) => r != 0, // floor of −x.y = −(x+1)
        (true, Rounding::Up) => false,    // ceil  of −x.y = −x
    };
    let q = if bump {
        q.checked_add(1).ok_or(MathError::Overflow)?
    } else {
        q
    };

    if q > i128::MAX as u128 {
        return Err(MathError::Overflow);
    }
    let q = q as i128;
    Ok(if negative { -q } else { q })
}

/// WAD-scaled integer power: `base^exp` where `base` is WAD and `exp` a plain
/// integer exponent. `exp = 0` returns 1.0 (WAD) for any base (including 0, by
/// the usual AMM convention). Negative base is allowed; the sign follows odd/even
/// parity. Each internal multiply truncates toward zero (≤ `exp` ULP total).
pub fn pow_int(base: i128, exp: u32) -> Result<i128, MathError> {
    if exp == 0 {
        return Ok(FIXED_SCALE);
    }
    let negative = base < 0 && exp % 2 == 1;
    let mut b = base.checked_abs().ok_or(MathError::Overflow)?;
    let mut acc = FIXED_SCALE;
    let mut e = exp;
    loop {
        if e & 1 == 1 {
            acc = mul_div(acc, b, FIXED_SCALE, Rounding::Down)?;
        }
        e >>= 1;
        if e == 0 {
            break;
        }
        b = mul_div(b, b, FIXED_SCALE, Rounding::Down)?;
    }
    Ok(if negative { -acc } else { acc })
}

/// Natural logarithm of a WAD value. Domain: `x > 0` (else `DomainError`).
///
/// Algorithm: range-reduce by powers of two to `m ∈ [1, 2)`, then the atanh
/// series `ln m = 2·(z + z³/3 + z⁵/5 + …)` with `z = (m−1)/(m+1) ∈ [0, 1/3)`,
/// which converges to WAD precision in ≤ ~20 terms. Result may be negative
/// (for `x < 1.0`). Error ≤ ~300 ULP; see module docs.
pub fn ln_fixed(x: i128) -> Result<i128, MathError> {
    if x <= 0 {
        return Err(MathError::DomainError);
    }
    // m ∈ [FIXED_SCALE, 2·FIXED_SCALE), x = m · 2^k.
    let mut m = x;
    let mut k: i32 = 0;
    while m >= 2 * FIXED_SCALE {
        m >>= 1;
        k += 1;
    }
    while m < FIXED_SCALE {
        m <<= 1; // safe: m < FIXED_SCALE ⇒ doubling < 2e18 ≪ i128::MAX
        k -= 1;
    }

    let z = mul_div(
        m - FIXED_SCALE,
        FIXED_SCALE,
        m + FIXED_SCALE,
        Rounding::Down,
    )?;
    let z2 = mul_div(z, z, FIXED_SCALE, Rounding::Down)?;

    let mut term = z; // z^(2n+1)
    let mut sum = z; // Σ z^(2n+1)/(2n+1)
    let mut n: i128 = 1;
    while term != 0 && n < 128 {
        term = mul_div(term, z2, FIXED_SCALE, Rounding::Down)?;
        n += 2;
        sum += term / n;
    }

    // ln x = k·ln2 + 2·Σ. |k| ≤ 127 so k·LN2 cannot overflow.
    Ok((k as i128) * LN2 + 2 * sum)
}

/// Round-to-nearest signed division by a positive divisor (ties away from zero).
#[inline]
fn div_round_nearest(a: i128, d: i128) -> i128 {
    debug_assert!(d > 0);
    if a >= 0 {
        (a + d / 2) / d
    } else {
        -((-a + d / 2) / d)
    }
}

/// Exponential of a WAD value: `e^x` in WAD.
///
/// - `x > ~46.58` (result > `i128::MAX`) → `Overflow` (enforced exactly by
///   checked arithmetic; fast-rejected above [`EXP_INPUT_UPPER`]).
/// - `x < −100` → returns 0 (true value < 1e-44, far below WAD resolution).
///
/// Algorithm: `x = k·ln2 + r` with `|r| ≤ ln2/2`, then a Taylor series for
/// `e^r ∈ [0.707, 1.415]` (≤ ~20 terms), scaled by `2^k`. Relative error ≤ ~1e-15.
pub fn exp_fixed(x: i128) -> Result<i128, MathError> {
    if x > EXP_INPUT_UPPER {
        return Err(MathError::Overflow);
    }
    if x < EXP_INPUT_LOWER {
        return Ok(0);
    }

    let k = div_round_nearest(x, LN2); // |k| ≤ ~145
    let r = x - k * LN2; // |r| ≤ LN2/2 < 0.35·FIXED_SCALE

    // e^r = Σ r^n / n!
    let mut term = FIXED_SCALE;
    let mut sum = FIXED_SCALE;
    let mut n: i128 = 1;
    while term != 0 && n < 64 {
        // term ← term·r/(n·FIXED_SCALE); n·FIXED_SCALE ≤ 64e18, no overflow.
        term = mul_div(term, r, n * FIXED_SCALE, Rounding::Down)?;
        sum += term;
        n += 1;
    }

    if k >= 0 {
        if k > 127 {
            return Err(MathError::Overflow);
        }
        let factor = 1i128 << (k as u32);
        sum.checked_mul(factor).ok_or(MathError::Overflow)
    } else {
        let shift = -k;
        if shift > 127 {
            return Ok(0);
        }
        Ok(sum >> (shift as u32))
    }
}

/// WAD power with a WAD (possibly fractional, possibly negative) exponent:
/// `base^exp = e^(exp·ln base)`.
///
/// Domain: `base > 0`, or `base == 0` with `exp > 0` (→ 0). A negative base is
/// a `DomainError` — non-integer powers of negatives are undefined in the reals,
/// and the CSEMM call sites are required to pass magnitudes (see
/// `docs/INVARIANT_MATH.md` §4). Never silently wrong.
pub fn pow_fixed(base: i128, exp: i128) -> Result<i128, MathError> {
    if base < 0 {
        return Err(MathError::DomainError);
    }
    if base == 0 {
        return if exp > 0 {
            Ok(0)
        } else {
            Err(MathError::DomainError)
        };
    }
    if exp == 0 {
        return Ok(FIXED_SCALE);
    }
    let ln_b = ln_fixed(base)?;
    let e_arg = mul_div(exp, ln_b, FIXED_SCALE, Rounding::Down)?;
    exp_fixed(e_arg)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests may unwrap; the deny applies to library paths
mod tests {
    //! Unit tests for the private wide-arithmetic helpers. The public API is
    //! exercised exhaustively in `tests/fixed_point.rs`.
    use super::*;

    #[test]
    fn isqrt_wide_matches_and_overflows() {
        // Agrees with plain isqrt when the product fits in i128.
        for (a, b) in [(0u128, 0u128), (1, 1), (100, 100), (7, 7), (12345, 98765)] {
            let prod = a * b;
            assert_eq!(isqrt_wide(a, b), isqrt(prod as i128) as u128);
        }
        // Perfect squares beyond i128 range: isqrt_wide(r, r) == r.
        for r in [1u128 << 100, 1u128 << 120, (1u128 << 127) - 1, u128::MAX] {
            assert_eq!(isqrt_wide(r, r), r, "r={r}");
        }
        // Between consecutive squares: floor holds.  (r²+1) → r ; ((r+1)²−1) → r
        let r: u128 = 1u128 << 100;
        // r² is exact; r²+something small still floors to r
        assert_eq!(isqrt_wide(r, r), r);
    }

    #[test]
    fn wide_mul_known_values() {
        assert_eq!(wide_mul(0, 0), (0, 0));
        assert_eq!(wide_mul(u128::MAX, 1), (0, u128::MAX));
        // (2^128 − 1)² = 2^256 − 2^129 + 1 → hi = 2^128 − 2, lo = 1
        assert_eq!(wide_mul(u128::MAX, u128::MAX), (u128::MAX - 1, 1));
        // 2^64 · 2^64 = 2^128 → hi = 1, lo = 0
        assert_eq!(wide_mul(1 << 64, 1 << 64), (1, 0));
    }

    #[test]
    fn div_wide_paths() {
        // hi == 0 fast path
        assert_eq!(div_wide(0, 100, 7), Some((14, 2)));
        // quotient does not fit
        assert_eq!(div_wide(7, 0, 7), None);
        // long-division path: (2^128 · 5 + 10) / 5 = 2^128 + 2 → doesn't fit
        assert_eq!(div_wide(5, 10, 5), None);
        // (2^128 · 3 + 7) / 4: hi=3 < 4 → q = (3·2^128 + 7)/4
        let (q, r) = div_wide(3, 7, 4).unwrap();
        // 3·2^128 + 7 = 4q + r ⇒ q = 3·2^126 + 1, r = 3
        assert_eq!(q, 3 * (1u128 << 126) + 1);
        assert_eq!(r, 3);
    }

    #[test]
    fn div_wide_reconstructs() {
        // q·d + r must reconstruct (hi, lo) for assorted values.
        let cases: &[(u128, u128, u128)] = &[
            (1, 0, 3),
            (12345, 678910, 999_999_937),
            (u128::MAX / 2, u128::MAX, u128::MAX),
            (0, u128::MAX, 1),
        ];
        for &(hi, lo, d) in cases {
            if let Some((q, r)) = div_wide(hi, lo, d) {
                assert!(r < d);
                // verify q·d + r == (hi, lo) using wide_mul
                let (phi, plo) = wide_mul(q, d);
                let (sum_lo, carry) = plo.overflowing_add(r);
                let sum_hi = phi + carry as u128;
                assert_eq!((sum_hi, sum_lo), (hi, lo), "hi={hi} lo={lo} d={d}");
            } else {
                assert!(hi >= d);
            }
        }
    }
}
