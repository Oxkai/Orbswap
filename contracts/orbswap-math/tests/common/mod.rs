//! Shared test harness for the orbswap-math integration tests.
//!
//! Runs on the host (std available), so f64 is used as a *reference oracle*
//! mirroring the verification log in `docs/INVARIANT_MATH.md`. The library
//! under test never touches floats; only these tests do.

#![allow(dead_code)] // each integration-test binary uses a subset of this

/// WAD scale, mirroring `orbswap_math::fixed_point::FIXED_SCALE`.
pub const S: i128 = 1_000_000_000_000_000_000;

/// Convert a WAD fixed-point value to f64 (test oracle only).
pub fn wad_to_f64(x: i128) -> f64 {
    x as f64 / 1e18
}

/// Convert an f64 to WAD (test construction only; rounds to nearest).
pub fn f64_to_wad(x: f64) -> i128 {
    (x * 1e18).round() as i128
}

/// Assert `|actual − expected| ≤ tol` (all WAD), with a readable failure.
#[track_caller]
pub fn assert_close(actual: i128, expected: i128, tol: i128, what: &str) {
    let diff = (actual - expected).abs();
    assert!(
        diff <= tol,
        "{what}: actual={actual} expected={expected} |diff|={diff} > tol={tol}"
    );
}

/// Assert relative closeness: `|actual − expected| ≤ max(abs_floor, |expected|/rel_den)`.
/// `rel_den = 1e12` means "within 1e-12 relative".
#[track_caller]
pub fn assert_rel_close(actual: i128, expected: i128, rel_den: i128, abs_floor: i128, what: &str) {
    let tol = core::cmp::max(abs_floor, expected.abs() / rel_den);
    assert_close(actual, expected, tol, what);
}

/// Deterministic pseudo-random generator (SplitMix64) — no external deps,
/// reproducible across runs/platforms.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[lo, hi]` (inclusive), `lo ≤ hi`.
    pub fn range_i128(&mut self, lo: i128, hi: i128) -> i128 {
        debug_assert!(lo <= hi);
        let span = (hi - lo) as u128 + 1;
        let r = ((self.next_u64() as u128) << 64) | self.next_u64() as u128;
        lo + (r % span) as i128
    }
}
