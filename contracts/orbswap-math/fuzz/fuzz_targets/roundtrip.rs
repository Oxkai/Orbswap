//! Fuzz: swapping X→Y then Y→X never lets the trader end up with more than they
//! started (no value extraction), for the CCMM circle.
#![no_main]

use libfuzzer_sys::fuzz_target;
use orbswap_math::ccmm;

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
    let k = (rd(data, &mut i).rem_euclid(1_000_000_000_000)) + 1_000;
    let dx = rd(data, &mut i).rem_euclid(k / 2) + 1;

    // Start on-curve at (0, k). Swap dx of X in.
    if let Ok((out, nx, ny)) = ccmm::swap_out(0, k, k, dx) {
        if out > 0 {
            // Now swap that `out` of Y back in (roles swapped): reserves (ny, nx).
            if let Ok((back, _, _)) = ccmm::swap_out(ny, nx, k, out) {
                assert!(back <= dx, "trader profited: dx={dx} back={back} k={k}");
            }
        }
    }
});
