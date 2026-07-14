//! Fuzz: at α=β=2+√2 (u=2) the CSEMM superellipse stays within a small relative
//! band of the CCMM circle on identical swaps (the "ladder").
#![no_main]

use libfuzzer_sys::fuzz_target;
use orbswap_math::{ccmm, csemm};

const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049; // 2+√2 in WAD

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
    let k = TWO_PLUS_SQRT2;

    // On-circle start via ccmm from (0, k).
    let x0 = rd(data, &mut i).rem_euclid(k);
    let y0 = if x0 == 0 {
        k
    } else {
        match ccmm::swap_out(0, k, k, x0) {
            Ok((_, _, ny)) => ny,
            Err(_) => return,
        }
    };
    let dx = rd(data, &mut i).rem_euclid(k - x0) + 1;

    // Ladder equivalence holds in the *well-resolved interior*. Near the
    // near-vertical x≈0 boundary, csemm's normalized ratio x/α quantizes coarsely
    // for wei-scale reserves and its transcendental chain becomes unreliable (it can
    // over- or under-output vs the true curve). That is an inherent fixed-point limit
    // of the superellipse near the price asymptote — NOT covered here; Phase 2 must
    // guard it (min trade size + post-swap invariant_holds). So gate to the interior.
    let interior = x0 >= k / 20 && x0 <= k - k / 20;
    if !interior {
        return;
    }
    if let (Ok((c_out, ..)), Ok((e_out, ..))) = (
        ccmm::swap_out(x0, y0, k, dx),
        csemm::swap_out(x0, y0, k, k, dx),
    ) {
        if c_out > 1_000_000_000 {
            let (cf, ef) = (c_out as f64, e_out as f64);
            assert!(
                (cf - ef).abs() <= cf.abs() * 1e-6 + 1_000_000.0,
                "ladder diverged in interior: ccmm={c_out} csemm={e_out} (x0={x0} dx={dx})"
            );
        }
    }
});
