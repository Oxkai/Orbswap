//! Integration tests for `ticks.rs` — every edge case from todo.md §1.5.

mod common;

use orbswap_math::ticks::{
    cross_tick, flip_tick, init_tick, is_initialized, next_initialized_tick, segment_swap,
    Direction, TickError, TickState, MAX_TICK, MIN_TICK,
};

// ---------------------------------------------------------------- init_tick

#[test]
fn init_tick_valid_range() {
    let t = init_tick(45).unwrap();
    assert_eq!(t.angle, 45);
    assert_eq!(t.liquidity_net, 0);
    assert!(t.initialized);
    assert!(init_tick(MIN_TICK).is_ok()); // 0
    assert!(init_tick(MAX_TICK).is_ok()); // 90
}

#[test]
fn init_tick_out_of_range() {
    assert_eq!(init_tick(-1), Err(TickError::OutOfRange));
    assert_eq!(init_tick(91), Err(TickError::OutOfRange));
    assert_eq!(init_tick(1000), Err(TickError::OutOfRange));
}

// ---------------------------------------------------------------- cross_tick

#[test]
fn cross_tick_both_directions() {
    // Up adds net, Down subtracts.
    assert_eq!(cross_tick(1000, 300, Direction::Up), Ok(1300));
    assert_eq!(cross_tick(1000, 300, Direction::Down), Ok(700));
    // Negative net (an upper position boundary).
    assert_eq!(cross_tick(1000, -300, Direction::Up), Ok(700));
    assert_eq!(cross_tick(1000, -300, Direction::Down), Ok(1300));
}

#[test]
fn cross_tick_conserves_liquidity_roundtrip() {
    // Crossing a tick up then back down returns to the same liquidity.
    let mut rng = common::Rng::new(0x71C);
    for _ in 0..10_000 {
        let active = rng.range_i128(0, 1_000_000_000_000_000_000);
        let net = rng.range_i128(-(active), 1_000_000_000_000_000_000);
        let up = cross_tick(active, net, Direction::Up).unwrap();
        let back = cross_tick(up, net, Direction::Down).unwrap();
        assert_eq!(back, active, "roundtrip active={active} net={net}");
    }
}

#[test]
fn cross_tick_guards_negative_and_overflow() {
    // Down past zero → InsufficientLiquidity.
    assert_eq!(
        cross_tick(100, 200, Direction::Down),
        Err(TickError::InsufficientLiquidity)
    );
    // Up with net that overflows.
    assert_eq!(
        cross_tick(i128::MAX, 1, Direction::Up),
        Err(TickError::Overflow)
    );
}

// ---------------------------------------------------------------- bitmap

#[test]
fn bitmap_flip_and_query() {
    let b = flip_tick(0, 45).unwrap();
    assert!(is_initialized(b, 45));
    assert!(!is_initialized(b, 44));
    let b = flip_tick(b, 45).unwrap(); // toggle back off
    assert!(!is_initialized(b, 45));
    assert_eq!(flip_tick(0, 91), Err(TickError::OutOfRange));
    assert_eq!(flip_tick(0, -1), Err(TickError::OutOfRange));
}

#[test]
fn next_initialized_skips_empty() {
    // Initialize ticks 10, 45, 80.
    let mut b = 0u128;
    for a in [10, 45, 80] {
        b = flip_tick(b, a).unwrap();
    }
    // Upward from various points.
    assert_eq!(next_initialized_tick(b, 0, Direction::Up), Some(10));
    assert_eq!(next_initialized_tick(b, 10, Direction::Up), Some(45)); // strictly beyond
    assert_eq!(next_initialized_tick(b, 44, Direction::Up), Some(45));
    assert_eq!(next_initialized_tick(b, 45, Direction::Up), Some(80));
    assert_eq!(next_initialized_tick(b, 80, Direction::Up), None); // nothing above
                                                                   // Downward.
    assert_eq!(next_initialized_tick(b, 90, Direction::Down), Some(80));
    assert_eq!(next_initialized_tick(b, 80, Direction::Down), Some(45)); // strictly below
    assert_eq!(next_initialized_tick(b, 45, Direction::Down), Some(10));
    assert_eq!(next_initialized_tick(b, 10, Direction::Down), None);
    assert_eq!(next_initialized_tick(b, 0, Direction::Down), None);
}

#[test]
fn next_initialized_empty_bitmap() {
    assert_eq!(next_initialized_tick(0, 45, Direction::Up), None);
    assert_eq!(next_initialized_tick(0, 45, Direction::Down), None);
}

// ---------------------------------------------------------------- segmentation

#[test]
fn segment_swap_fits_and_overflows_tick() {
    // Fits inside the tick: consume all, no carry, no crossing.
    let s = segment_swap(50, 100).unwrap();
    assert_eq!((s.consumed, s.carry, s.reached_boundary), (50, 0, false));
    // Exactly reaches the edge.
    let s = segment_swap(100, 100).unwrap();
    assert_eq!((s.consumed, s.carry, s.reached_boundary), (100, 0, true));
    // Overflows the tick: consume to boundary, carry the rest, crossing follows.
    let s = segment_swap(250, 100).unwrap();
    assert_eq!((s.consumed, s.carry, s.reached_boundary), (100, 150, true));
    // Negative inputs rejected.
    assert_eq!(segment_swap(-1, 100), Err(TickError::OutOfRange));
    assert_eq!(segment_swap(100, -1), Err(TickError::OutOfRange));
}

#[test]
fn multi_tick_walk_carries_and_conserves() {
    // Simulate a swap crossing several ticks. Each tick i (from a small model
    // "contract storage") has a capacity `cap[i]` (input to its far edge at the
    // tick's liquidity) and a `net[i]` applied on crossing. We walk upward,
    // segmenting the input, carrying the remainder, and crossing — then assert
    // total consumed == input and liquidity is conserved on the return walk.
    let caps = [100i128, 200, 150, 300];
    let nets = [500i128, 300, -200, 100];
    let start_liquidity = 1_000i128;

    let mut input = 100 + 200 + 150 + 120; // fills 3 full ticks + partial of the 4th
    let mut liquidity = start_liquidity;
    let mut consumed_total = 0;
    let mut crossings = 0usize;
    let mut liq_trace = vec![liquidity];

    for i in 0..caps.len() {
        if input == 0 {
            break;
        }
        let seg = segment_swap(input, caps[i]).unwrap();
        consumed_total += seg.consumed;
        input = seg.carry;
        if seg.reached_boundary {
            liquidity = cross_tick(liquidity, nets[i], Direction::Up).unwrap();
            crossings += 1;
            liq_trace.push(liquidity);
        }
    }

    assert_eq!(consumed_total, 570, "all input consumed across ticks");
    assert_eq!(input, 0, "no leftover (last tick only partially filled)");
    assert_eq!(crossings, 3, "crossed exactly 3 full ticks");

    // Liquidity conservation: unwinding the same crossings downward returns to start.
    for i in (0..crossings).rev() {
        liquidity = cross_tick(liquidity, nets[i], Direction::Down).unwrap();
    }
    assert_eq!(
        liquidity, start_liquidity,
        "liquidity conserved over the walk"
    );
}

// ---------------------------------------------------------------- TickState

#[test]
fn tick_state_cross_updates_liquidity_and_angle() {
    let mut st = TickState {
        current_angle: 45,
        active_liquidity: 1_000,
        spacing: 5,
    };
    st.cross(200, Direction::Up).unwrap();
    assert_eq!(st.active_liquidity, 1_200);
    assert_eq!(st.current_angle, 50);

    st.cross(200, Direction::Down).unwrap();
    assert_eq!(st.active_liquidity, 1_000); // back
    assert_eq!(st.current_angle, 45);
}

#[test]
fn tick_state_angle_clamps_to_arc() {
    let mut st = TickState {
        current_angle: 88,
        active_liquidity: 1_000,
        spacing: 5,
    };
    st.cross(0, Direction::Up).unwrap();
    assert_eq!(st.current_angle, MAX_TICK); // clamped at 90, not 93

    let mut st = TickState {
        current_angle: 2,
        active_liquidity: 1_000,
        spacing: 5,
    };
    st.cross(0, Direction::Down).unwrap();
    assert_eq!(st.current_angle, MIN_TICK); // clamped at 0
}
