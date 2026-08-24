//! SEP-40 oracle client and rate cache for **rate-aware** pools (todo.md §Phase 1).
//!
//! A rate-aware pool maps native reserves into the math library's normalized space
//! as `internal = native · scale · rate`, so its balanced point sits at the oracle
//! FX rate instead of at 1:1. This module owns everything that talks to the feed;
//! the curve math never sees a rate.
//!
//! # Why swaps never call the feed
//! Cross-contract calls cost CPU against Soroban's per-tx budget, and a swap that
//! depends on a live oracle read fails whenever the feed is congested. Swaps
//! therefore read the **cached** rate ([`current`]); only [`fetch_rate`], driven by
//! `poke_rate`, touches the feed. That also gives staleness and deviation checks a
//! single choke point.
//!
//! # Safety posture
//! Per todo.md §0, an open pool sitting off the invariant is a free pot for the
//! first trader. So a rate that is stale, deviant, or unavailable must **close the
//! pool**, never be papered over with a fallback value.

use crate::errors::OrbswapError;
use crate::storage;
use crate::types::{RateConfig, WAD};
use orbswap_math::fixed_point::{mul_div, Rounding};
use soroban_sdk::{contractclient, contracttype, Address, Env, Symbol, Vec};

/// Basis-point denominator.
pub const BPS: i128 = 10_000;

// ─── SEP-40 (Oracle Consumer Interface) ──────────────────────────────────────
// Verified against stellar-protocol/ecosystem/sep-0040.md on 2026-08-21. We
// declare only the subset we call; `#[contractclient]` needs no implementation.
//
// NOTE: `twap` is NOT part of SEP-40 (it is a Reflector extension). Depending on
// it would break feed-agnosticism. Build smoothing from `prices` if ever needed.

/// A price sample from a SEP-40 feed, in the feed's own decimals.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

/// SEP-40 asset identifier.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

// The trait exists to generate `PriceFeedClient`; nothing calls it directly.
#[allow(dead_code)]
#[contractclient(name = "PriceFeedClient")]
pub trait PriceFeedTrait {
    fn base(env: Env) -> Asset;
    fn decimals(env: Env) -> u32;
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
}

// ─── Decimal normalization ───────────────────────────────────────────────────

/// `10^n` for `n ≤ 18`.
fn pow10(n: u32) -> Result<i128, OrbswapError> {
    if n > 18 {
        return Err(OrbswapError::InvalidRateConfig);
    }
    let mut v: i128 = 1;
    for _ in 0..n {
        v *= 10;
    }
    Ok(v)
}

/// Lift a feed price from `feed_decimals` to WAD (18 decimals).
///
/// Feeds coarser than WAD are scaled up exactly; feeds at 18 decimals pass
/// through. `feed_decimals > 18` is rejected at `configure_rates` time.
pub fn to_wad(price: i128, feed_decimals: u32) -> Result<i128, OrbswapError> {
    if price <= 0 {
        return Err(OrbswapError::OracleUnavailable);
    }
    let factor = pow10(
        18u32
            .checked_sub(feed_decimals)
            .ok_or(OrbswapError::InvalidRateConfig)?,
    )?;
    price.checked_mul(factor).ok_or(OrbswapError::Overflow)
}

// ─── Feed reads ──────────────────────────────────────────────────────────────

/// Read the current rate for the quote leg, in WAD, plus its timestamp.
///
/// In `cross` mode the feed is denominated in something other than the pool's
/// numeraire (Lightecho quotes against XLM), so the rate is
/// `lastprice(quote) / lastprice(numeraire)` — the feed's decimals cancel in the
/// division — and the timestamp is the **older** of the two samples, which is the
/// conservative choice for staleness.
///
/// Errors with [`OrbswapError::OracleUnavailable`] if the feed returns `None`, a
/// non-positive price, or a zero divisor. There is deliberately **no fallback**:
/// per todo.md §0, quoting from a bad rate is worse than not quoting.
pub fn fetch_rate(
    env: &Env,
    cfg: &RateConfig,
    tokens: &Vec<Address>,
) -> Result<(i128, u64), OrbswapError> {
    let client = PriceFeedClient::new(env, &cfg.feed);

    let quote_addr = tokens
        .try_get(cfg.quote_index)
        .map_err(|_| OrbswapError::InvalidRateConfig)?
        .ok_or(OrbswapError::InvalidRateConfig)?;
    let pq = client
        .lastprice(&Asset::Stellar(quote_addr))
        .ok_or(OrbswapError::OracleUnavailable)?;
    if pq.price <= 0 {
        return Err(OrbswapError::OracleUnavailable);
    }

    let (rate, ts) = if cfg.cross {
        let num_addr = tokens
            .try_get(cfg.numeraire_index)
            .map_err(|_| OrbswapError::InvalidRateConfig)?
            .ok_or(OrbswapError::InvalidRateConfig)?;
        let pn = client
            .lastprice(&Asset::Stellar(num_addr))
            .ok_or(OrbswapError::OracleUnavailable)?;
        if pn.price <= 0 {
            return Err(OrbswapError::OracleUnavailable);
        }
        // Decimals cancel: (q · 10^d) / (n · 10^d) · WAD.
        let r =
            mul_div(pq.price, WAD, pn.price, Rounding::Down).map_err(|_| OrbswapError::Overflow)?;
        (r, core::cmp::min(pq.timestamp, pn.timestamp))
    } else {
        (to_wad(pq.price, cfg.feed_decimals)?, pq.timestamp)
    };

    if rate <= 0 {
        return Err(OrbswapError::OracleUnavailable);
    }
    Ok((rate, ts))
}

// ─── Guards ──────────────────────────────────────────────────────────────────

/// Absolute relative move between two rates, in bps, **rounded up** so a move
/// sitting exactly on the bound trips rather than squeaks through.
pub fn deviation_bps(old: i128, new: i128) -> Result<i128, OrbswapError> {
    if old <= 0 {
        return Err(OrbswapError::OracleUnavailable);
    }
    let delta = (new - old).abs();
    mul_div(delta, BPS, old, Rounding::Up).map_err(|_| OrbswapError::Overflow)
}

/// Whether the cached rate is within `max_age_secs` of `now`.
pub fn is_fresh(env: &Env, cfg: &RateConfig, now: u64) -> bool {
    let last = storage::get_rate_last_time(env);
    // A clock behind the last write is treated as fresh (no negative age).
    now.saturating_sub(last) <= cfg.max_age_secs
}

/// Cached rates, WAD, parallel to `Config.tokens`. All `WAD` in parity mode, so
/// callers never branch on whether the pool is rate-aware.
pub fn current(env: &Env, n: u32) -> Vec<i128> {
    storage::get_rates(env, n)
}

/// Gate for every value-moving path **except withdrawals**. All three conditions
/// close the pool, because each one means a trade would price off a state the
/// contract cannot vouch for:
///
/// 1. **Breaker latched** — the feed moved implausibly; halt rather than reprice.
/// 2. **Rate stale** — the cached rate may no longer reflect the market.
/// 3. **Re-anchor pending** — an accepted rate change moved the pool off its
///    curve, and per todo.md §0 an open off-curve pool hands its full
///    revaluation to the first trader, at any trade size.
///
/// Withdrawals deliberately do not call this: an LP must always be able to exit,
/// whatever the oracle is doing.
pub fn require_tradeable(env: &Env) -> Result<(), OrbswapError> {
    let cfg = match storage::get_rate_config(env) {
        Some(c) => c,
        None => return Ok(()), // parity pool: nothing to check
    };
    if storage::get_rate_breaker(env) {
        return Err(OrbswapError::RateBreakerTripped);
    }
    if !is_fresh(env, &cfg, env.ledger().timestamp()) {
        return Err(OrbswapError::RateStale);
    }
    if storage::get_needs_reanchor(env) {
        return Err(OrbswapError::OffCurve);
    }
    Ok(())
}
