//! Fuzz: after a `ccmm`/`csemm` swap the reserves still satisfy the invariant
//! within epsilon, and no swap ever panics.
#![no_main]

use libfuzzer_sys::fuzz_target;
use orbswap_math::fixed_point::FIXED_SCALE;
use orbswap_math::{ccmm, csemm};

fn rd(d: &[u8], i: &mut usize) -> i128 {
    let mut b = [0u8; 16];
    for byte in b.iter_mut() {
        *byte = d.get(*i).copied().unwrap_or(0);
        *i += 1;
    }
    i128::from_le_bytes(b)
}

fuzz_target!(|data: &[u8]| {
    let mut i = 0;

    // --- CCMM: build an on-curve start via a swap from (0, k), then swap again.
    let k = (rd(data, &mut i).rem_euclid(1_000_000_000_000)) + 1_000;
    let x0 = rd(data, &mut i).rem_euclid(k + 1);
    let y0 = if x0 == 0 {
        k
    } else {
        match ccmm::swap_out(0, k, k, x0) {
            Ok((_, _, ny)) => ny,
            Err(_) => k,
        }
    };
    let amt = rd(data, &mut i).rem_euclid(k) + 1;
    if let Ok((out, nx, ny)) = ccmm::swap_out(x0, y0, k, amt) {
        assert!(out >= 0 && nx >= 0 && ny >= 0);
        // Post-swap point is on the circle within the integer-√ residual (~2k).
        assert!(ccmm::invariant_holds(nx, ny, k, 2 * k + 2), "ccmm off-curve");
    }

    // --- CSEMM: random shape + on-curve-ish start; swap must keep the invariant.
    let alpha = (rd(data, &mut i).rem_euclid(8 * FIXED_SCALE)) + 2 * FIXED_SCALE;
    let beta = (rd(data, &mut i).rem_euclid(8 * FIXED_SCALE)) + 2 * FIXED_SCALE;
    let xin = rd(data, &mut i).rem_euclid(alpha + 1);
    // Solve y on the curve via a swap from (0, β).
    if let Ok((_, _, y_start)) = csemm::swap_out(0, beta, alpha, beta, xin.max(1)) {
        let ys = if xin == 0 { beta } else { y_start };
        let dx = rd(data, &mut i).rem_euclid(alpha - xin.min(alpha - 1)) + 1;
        if let Ok((out, nx, ny)) = csemm::swap_out(xin, ys, alpha, beta, dx) {
            assert!(out >= 0 && nx >= 0 && ny >= 0);
            // Transcendental residual budget.
            assert!(
                csemm::invariant_holds(nx, ny, alpha, beta, 1_000_000_000),
                "csemm off-curve"
            );
        }
    }
});
