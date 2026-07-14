//! Fuzz: `fixed_point` primitives never panic, and their core invariants hold on
//! arbitrary input (isqrt floor, mul_div sandwich, ln/exp roundtrip bounds).
#![no_main]

use libfuzzer_sys::fuzz_target;
use orbswap_math::fixed_point::{
    exp_fixed, isqrt, ln_fixed, mul_div, pow_fixed, Rounding, FIXED_SCALE,
};

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

    // isqrt: exact floor for n>0, 0 otherwise. Never panics.
    let n = rd(data, &mut i);
    let r = isqrt(n);
    if n > 0 {
        let (ru, nu) = (r as u128, n as u128);
        assert!(ru * ru <= nu, "isqrt above");
        assert!((ru + 1).checked_mul(ru + 1).map_or(true, |v| v > nu), "isqrt not tight");
    } else {
        assert_eq!(r, 0);
    }

    // mul_div: Down ≤ Up and they differ by ≤ 1.
    let (a, b, dv) = (rd(data, &mut i), rd(data, &mut i), rd(data, &mut i));
    if let (Ok(dn), Ok(up)) = (
        mul_div(a, b, dv, Rounding::Down),
        mul_div(a, b, dv, Rounding::Up),
    ) {
        assert!(dn <= up && up - dn <= 1, "mul_div sandwich");
    }

    // ln/exp: roundtrip stays within a loose relative band (never panics).
    let x = rd(data, &mut i);
    if x > 0 {
        if let Ok(lx) = ln_fixed(x) {
            if let Ok(bk) = exp_fixed(lx) {
                let diff = (bk - x).abs();
                assert!(diff <= x / 10_000 + 1_000_000_000, "exp(ln x) drift");
            }
        }
    }

    // pow_fixed: never panics; a non-negative base with exp 0 is 1.
    let base = rd(data, &mut i);
    let exp = rd(data, &mut i);
    if base >= 0 {
        let _ = pow_fixed(base, exp);
        if base > 0 {
            assert_eq!(pow_fixed(base, 0), Ok(FIXED_SCALE));
        }
    }
});
