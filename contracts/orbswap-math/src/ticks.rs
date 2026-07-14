//! Ticks in polar coordinates (paper §2.2) — CCMM only.
//!
//! Ticks discretize the `0°–90°` swap arc (all-`$1.00` sits at 45°). Each tick
//! carries a `liquidity_net` that is applied to the pool's active liquidity `L`
//! when the swap angle crosses it (Uniswap-v3 mechanic): moving **up** in angle
//! adds `liquidity_net`, moving **down** subtracts it.
//!
//! # Scope (pure, storage-free)
//! This module owns only the storage-independent primitives:
//! liquidity bookkeeping ([`cross_tick`]), a [`u128`] tick **bitmap**
//! ([`next_initialized_tick`]), and swap **segmentation** ([`segment_swap`]).
//! Per-segment reserve math comes from [`crate::ccmm`]/[`crate::polar`], and the
//! multi-tick execution *loop* lives in the contract (see `docs/TICK_DESIGN.md`,
//! Architecture §D) — deliberately not hardcoded here.

/// Lowest valid tick angle (fully token-Y).
pub const MIN_TICK: i128 = 0;
/// Highest valid tick angle (fully token-X). The arc is `[0°, 90°]`.
pub const MAX_TICK: i128 = 90;

/// Errors from tick operations. No function here panics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickError {
    /// A tick angle was outside `[MIN_TICK, MAX_TICK]`, or an amount was negative.
    OutOfRange,
    /// Crossing would drive active liquidity below zero.
    InsufficientLiquidity,
    /// An intermediate exceeded `i128` range.
    Overflow,
}

/// Direction of travel along the arc during a swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Angle increasing (toward 90°, buying X).
    Up,
    /// Angle decreasing (toward 0°, buying Y).
    Down,
}

/// A single tick: its angle, the net liquidity applied when crossed upward, and
/// whether it holds any position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub angle: i128,
    pub liquidity_net: i128,
    pub initialized: bool,
}

/// Create an (empty) tick at `angle_degrees`, validating `0 ≤ angle ≤ 90`.
pub fn init_tick(angle_degrees: i128) -> Result<Tick, TickError> {
    if !(MIN_TICK..=MAX_TICK).contains(&angle_degrees) {
        return Err(TickError::OutOfRange);
    }
    Ok(Tick {
        angle: angle_degrees,
        liquidity_net: 0,
        initialized: true,
    })
}

/// Apply a tick's `liquidity_net` to the active liquidity when crossing it.
///
/// `Up` adds, `Down` subtracts (v3 convention). Result must stay `≥ 0`.
pub fn cross_tick(
    active_liquidity: i128,
    tick_net: i128,
    direction: Direction,
) -> Result<i128, TickError> {
    let new = match direction {
        Direction::Up => active_liquidity.checked_add(tick_net),
        Direction::Down => active_liquidity.checked_sub(tick_net),
    }
    .ok_or(TickError::Overflow)?;
    if new < 0 {
        return Err(TickError::InsufficientLiquidity);
    }
    Ok(new)
}

// ---------------------------------------------------------------- bitmap

/// Whether tick `angle` is initialized in `bitmap` (bit `angle` set).
pub fn is_initialized(bitmap: u128, angle: i128) -> bool {
    if !(0..128).contains(&angle) {
        return false;
    }
    bitmap & (1u128 << angle) != 0
}

/// Toggle the initialized bit for `angle`. Out-of-range angles are a no-op error.
pub fn flip_tick(bitmap: u128, angle: i128) -> Result<u128, TickError> {
    if !(MIN_TICK..=MAX_TICK).contains(&angle) {
        return Err(TickError::OutOfRange);
    }
    Ok(bitmap ^ (1u128 << angle))
}

/// The next initialized tick strictly beyond `from` in `direction`, skipping
/// empty ticks. `None` if there is none in range.
pub fn next_initialized_tick(bitmap: u128, from: i128, direction: Direction) -> Option<i128> {
    match direction {
        Direction::Up => {
            // Bits strictly above `from`.
            let shift = from + 1;
            if shift >= 128 {
                return None;
            }
            let mask = if shift <= 0 {
                bitmap
            } else {
                bitmap & (!0u128 << shift)
            };
            if mask == 0 {
                None
            } else {
                Some(mask.trailing_zeros() as i128)
            }
        }
        Direction::Down => {
            // Bits strictly below `from`.
            if from <= 0 {
                return None;
            }
            let mask = if from >= 128 {
                bitmap
            } else {
                bitmap & ((1u128 << from) - 1)
            };
            if mask == 0 {
                None
            } else {
                Some((127 - mask.leading_zeros()) as i128)
            }
        }
    }
}

// ---------------------------------------------------------------- segmentation

/// The outcome of fitting a swap against the current tick's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Input consumed within this tick.
    pub consumed: i128,
    /// Input left over to carry into the next tick (0 if the swap fit).
    pub carry: i128,
    /// Whether the swap reached the tick edge (⇒ a crossing must follow).
    pub reached_boundary: bool,
}

/// Split a swap at the current tick edge.
///
/// `available` is the remaining input; `to_boundary` is the input that exactly
/// reaches the next tick edge at the current liquidity (computed by the caller
/// via `ccmm`/`polar`). If `available ≥ to_boundary` the edge is reached and the
/// surplus is carried; otherwise the swap fills partially inside the tick.
pub fn segment_swap(available: i128, to_boundary: i128) -> Result<Segment, TickError> {
    if available < 0 || to_boundary < 0 {
        return Err(TickError::OutOfRange);
    }
    if available >= to_boundary {
        Ok(Segment {
            consumed: to_boundary,
            carry: available - to_boundary,
            reached_boundary: true,
        })
    } else {
        Ok(Segment {
            consumed: available,
            carry: 0,
            reached_boundary: false,
        })
    }
}

// ---------------------------------------------------------------- state

/// The pool's live tick position: current angle, active liquidity, and spacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickState {
    pub current_angle: i128,
    pub active_liquidity: i128,
    pub spacing: i128,
}

impl TickState {
    /// Cross the tick with `tick_net`, updating active liquidity and advancing
    /// `current_angle` by one `spacing` step in `direction` (clamped to the arc).
    pub fn cross(&mut self, tick_net: i128, direction: Direction) -> Result<(), TickError> {
        self.active_liquidity = cross_tick(self.active_liquidity, tick_net, direction)?;
        let step = match direction {
            Direction::Up => self.spacing,
            Direction::Down => -self.spacing,
        };
        let next = self
            .current_angle
            .checked_add(step)
            .ok_or(TickError::Overflow)?;
        self.current_angle = next.clamp(MIN_TICK, MAX_TICK);
        Ok(())
    }
}
