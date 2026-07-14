//! Orbswap AMM math — pure Rust, no `soroban_sdk`.
//!
//! Implements the invariants and swap functions from Tolstikov, Wentz, Schiarizzi,
//! *Concentrated N-dimensional AMM with Polar Coordinates in Rust* (Sept 19, 2025).
//! Every equation and its edge cases are documented in `docs/INVARIANT_MATH.md`.
//!
//! Design rules (see `todo.md`, "Architecture model"):
//! - `#![no_std]`, zero dependencies, **no floats** — deterministic on any target,
//!   embeddable in Soroban (wasm) contracts.
//! - All fractional quantities are WAD fixed-point (`fixed_point::FIXED_SCALE` = 1e18).
//! - Every rounding decision favors the pool; callers pick the direction explicitly.
//! - No `panic!`/`unwrap` in library code paths: fallible ops return `Result`.

#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

pub mod ccmm;
pub mod circle_liq;
pub mod csemm;
pub mod fees;
pub mod fingerprint;
pub mod fixed_point;
pub mod multimodal;
pub mod ndim;
pub mod oracle;
pub mod polar;
pub mod skew;
pub mod ticks;
