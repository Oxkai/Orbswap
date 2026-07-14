//! Fuzz: tick liquidity is conserved across an up↔down crossing, segmentation
//! carry is consistent, and nothing panics.
#![no_main]

use libfuzzer_sys::fuzz_target;
use orbswap_math::ticks::{cross_tick, segment_swap, Direction};

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

    // cross_tick conservation: up then down returns to the start (when both valid).
    let active = rd(data, &mut i).rem_euclid(1_000_000_000_000_000_000);
    let net = rd(data, &mut i);
    if let Ok(up) = cross_tick(active, net, Direction::Up) {
        if let Ok(back) = cross_tick(up, net, Direction::Down) {
            assert_eq!(back, active, "tick liquidity not conserved");
        }
    }

    // segment_swap: consumed + carry == available, and carry>0 ⇒ boundary reached.
    let available = rd(data, &mut i).rem_euclid(1_000_000_000_000);
    let to_boundary = rd(data, &mut i).rem_euclid(1_000_000_000_000);
    if let Ok(seg) = segment_swap(available, to_boundary) {
        assert_eq!(seg.consumed + seg.carry, available, "segment sum");
        assert!(seg.consumed >= 0 && seg.carry >= 0);
        if seg.carry > 0 {
            assert!(seg.reached_boundary, "carry without crossing");
        }
    }
});
