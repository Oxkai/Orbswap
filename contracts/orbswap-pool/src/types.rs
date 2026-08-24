//! Pool types. `PoolMode`, `Config`, and the shared constants live in
//! `orbswap-pool-interface` (so the factory/router can link against them without
//! the contract implementation); this module re-exports them plus the pool-local
//! `Paused` flags.
//!
//! Scaling model (Architecture §A–E): reserves are native units; the math runs in
//! normalized space `x̂ᵢ = internalᵢ / s ∈ [0, αᵢ]` (`internalᵢ` = native scaled to
//! 18 decimals); `s` is the dynamic liquidity scale (WAD); shape `α, β` is fixed.

pub use orbswap_pool_interface::{Config, PoolMode, MINIMUM_LIQUIDITY, TWO_PLUS_SQRT2, WAD};
use soroban_sdk::{contracttype, Address, Vec};

/// Per-operation pause flags (all default `false`). Withdrawals ideally stay open.
#[contracttype]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Paused {
    pub deposits: bool,
    pub swaps: bool,
    pub withdrawals: bool,
}

/// Tick angle resolution: `θc` and per-tick angles are integer degrees `0..=90`;
/// this is the sub-degree multiplier used for the continuous swap progress within a
/// segment (never for the `cos/sin` table, which is per integer degree).
pub const TICK_RES: i128 = 1_000_000;
/// Arc endpoints (integer degrees): 0° = all Y, 90° = all X, 45° = balanced.
pub const MIN_TICK: u32 = 0;
pub const MAX_TICK: u32 = 90;

/// A concentrated-liquidity position (Circular pools only): `liquidity` `L` spread
/// over the owner's `[lower, upper]` tick range, with the fee-growth snapshot taken
/// at the last interaction (used to settle owed fees on the next touch).
#[contracttype]
#[derive(Clone)]
pub struct Position {
    pub liquidity: i128,
    pub fee_growth_inside_last: Vec<i128>,
}

/// Oracle configuration for a **rate-aware** pool (todo.md §3). Absent from
/// storage ⇒ the pool is in parity mode and every rate is exactly `WAD`.
///
/// `rates[numeraire_index]` is pinned at `WAD`; only `quote_index` ever moves, so
/// the balanced point tracks the feed instead of sitting at 1:1.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateConfig {
    /// SEP-40 `PriceFeedTrait` contract.
    pub feed: Address,
    /// Index into `Config.tokens` of the leg the feed prices (the local currency).
    pub quote_index: u32,
    /// Index into `Config.tokens` of the leg pinned at `WAD` (the USDC side).
    pub numeraire_index: u32,
    /// `true` ⇒ the feed is denominated in something other than the numeraire
    /// (e.g. Lightecho quotes against XLM), so the rate is
    /// `lastprice(quote) / lastprice(numeraire)` and the staleness check uses the
    /// **older** of the two timestamps.
    pub cross: bool,
    /// Staleness threshold, seconds.
    pub max_age_secs: u64,
    /// Per-update move that trips the breaker instead of being accepted.
    pub max_deviation_bps: i128,
    /// Cached `feed.decimals()`, read once at `configure_rates`.
    pub feed_decimals: u32,
}
