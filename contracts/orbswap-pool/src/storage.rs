//! Storage keys, typed accessors, and TTL bumps.
//!
//! Pool-global state lives in **instance** storage (bumped together with the
//! contract); per-LP share balances live in **persistent** storage. See the
//! `todo.md` §2.1 config/state split.

use crate::errors::OrbswapError;
use crate::types::{Config, Paused, Position, RateConfig, WAD};
use soroban_sdk::{contracttype, Address, Env, Vec};

// Instance TTL bump thresholds (ledgers). ~30 days at 5s/ledger ≈ 518k.
const INSTANCE_BUMP_AMOUNT: u32 = 518_400;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - 86_400;
const PERSISTENT_BUMP_AMOUNT: u32 = 518_400;
const PERSISTENT_LIFETIME_THRESHOLD: u32 = PERSISTENT_BUMP_AMOUNT - 86_400;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Immutable [`Config`] (instance).
    Config,
    /// Native reserves, parallel to `Config.tokens` (instance).
    Reserves,
    /// Liquidity scale `s`, WAD (instance).
    S,
    /// Total LP shares outstanding, incl. the locked minimum (instance).
    TotalShares,
    /// Per-operation pause flags (instance).
    Paused,
    /// Cumulative price accumulator (Σ price·Δt), token0-in-token1, WAD (instance).
    OracleCumulative,
    /// Ledger timestamp of the last oracle update (instance).
    OracleLastTime,
    /// Protocol's share of the swap fee, bps of the fee (instance; default 0).
    ProtocolFeeBps,
    /// Protocol fees collected so far, native units, parallel to tokens (instance).
    ProtocolOwed,
    /// LP fees accrued outside the curve, native units, parallel to tokens
    /// (instance). Distributed to LPs proportionally on withdraw. Kept OUT of the
    /// curve reserves so the invariant stays exact and swaps price per the paper.
    LpFeesOwed,
    /// Per-token allowed flags (depeg eject), parallel to tokens (instance).
    Allowed,
    /// Per-LP share balance (persistent).
    Shares(Address),

    // ── Concentrated-liquidity ticks (Circular pools only; see docs/TICK_DESIGN.md) ──
    /// Opt-in flag: concentrated-liquidity tick mode enabled (default false).
    TickMode,
    /// Current segment's reference tick (integer degree the polar math anchors on).
    TickRef,
    /// Continuous y-input consumed within the current segment (native units).
    TickYProg,
    /// Active liquidity `L` spanning the current angle (instance).
    ActiveLiq,
    /// Initialized-tick bitmap: bit `d` set ⇒ a position boundary at degree `d`.
    TickBitmap,
    /// Global fee growth per token, WAD per unit of `L` (Vec<i128>, len = tokens).
    FeeGrowthGlobal,
    /// Per-tick net liquidity applied on an upward cross (angle degree → i128).
    TickNet(u32),
    /// Per-tick fee-growth-outside per token (v3 bookkeeping), Vec<i128>.
    TickFeeOutside(u32),
    /// Per-LP concentrated position keyed by (owner, lower°, upper°) (persistent).
    Position(Address, u32, u32),

    // ── Rate-aware pools (todo.md §3) ─────────────────────────────────────────
    /// [`RateConfig`] for an oracle-priced pool; absent ⇒ parity mode (instance).
    RateCfg,
    /// Last-accepted rate per token, WAD, parallel to `Config.tokens` (instance).
    Rates,
    /// Ledger timestamp of the last accepted rate (instance).
    RateLastTime,
    /// Latched oracle circuit breaker (instance; default false).
    RateBreaker,
    /// Set when an accepted rate change moved the pool off its curve; cleared by
    /// `re_anchor`. Trading is refused while set (todo.md §0).
    NeedsReAnchor,
    /// Whether the LP allowlist is enforced (instance; default false).
    OperatorMode,
    /// Per-address LP permission (persistent).
    Operator(Address),
}

pub fn is_initialized(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::Config)
}

pub fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

pub fn get_config(env: &Env) -> Result<Config, OrbswapError> {
    env.storage()
        .instance()
        .get(&DataKey::Config)
        .ok_or(OrbswapError::NotInitialized)
}

pub fn set_config(env: &Env, config: &Config) {
    env.storage().instance().set(&DataKey::Config, config);
}

pub fn get_reserves(env: &Env) -> Vec<i128> {
    env.storage()
        .instance()
        .get(&DataKey::Reserves)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_reserves(env: &Env, reserves: &Vec<i128>) {
    env.storage().instance().set(&DataKey::Reserves, reserves);
}

pub fn get_s(env: &Env) -> i128 {
    env.storage().instance().get(&DataKey::S).unwrap_or(0)
}

pub fn set_s(env: &Env, s: i128) {
    env.storage().instance().set(&DataKey::S, &s);
}

pub fn get_total_shares(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TotalShares)
        .unwrap_or(0)
}

pub fn set_total_shares(env: &Env, shares: i128) {
    env.storage().instance().set(&DataKey::TotalShares, &shares);
}

pub fn get_oracle(env: &Env) -> (i128, u64) {
    let cum = env
        .storage()
        .instance()
        .get(&DataKey::OracleCumulative)
        .unwrap_or(0);
    let last = env
        .storage()
        .instance()
        .get(&DataKey::OracleLastTime)
        .unwrap_or(0);
    (cum, last)
}

pub fn set_oracle(env: &Env, cumulative: i128, last_time: u64) {
    env.storage()
        .instance()
        .set(&DataKey::OracleCumulative, &cumulative);
    env.storage()
        .instance()
        .set(&DataKey::OracleLastTime, &last_time);
}

pub fn get_protocol_fee_bps(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::ProtocolFeeBps)
        .unwrap_or(0)
}

pub fn set_protocol_fee_bps(env: &Env, bps: i128) {
    env.storage().instance().set(&DataKey::ProtocolFeeBps, &bps);
}

pub fn get_protocol_owed(env: &Env) -> Vec<i128> {
    env.storage()
        .instance()
        .get(&DataKey::ProtocolOwed)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_protocol_owed(env: &Env, owed: &Vec<i128>) {
    env.storage().instance().set(&DataKey::ProtocolOwed, owed);
}

pub fn get_lp_fees_owed(env: &Env) -> Vec<i128> {
    env.storage()
        .instance()
        .get(&DataKey::LpFeesOwed)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_lp_fees_owed(env: &Env, owed: &Vec<i128>) {
    env.storage().instance().set(&DataKey::LpFeesOwed, owed);
}

pub fn get_allowed(env: &Env) -> Vec<bool> {
    env.storage()
        .instance()
        .get(&DataKey::Allowed)
        .unwrap_or_else(|| Vec::new(env))
}

pub fn set_allowed(env: &Env, allowed: &Vec<bool>) {
    env.storage().instance().set(&DataKey::Allowed, allowed);
}

pub fn get_paused(env: &Env) -> Paused {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or_default()
}

pub fn set_paused(env: &Env, paused: &Paused) {
    env.storage().instance().set(&DataKey::Paused, paused);
}

pub fn get_shares(env: &Env, who: &Address) -> i128 {
    let key = DataKey::Shares(who.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

pub fn set_shares(env: &Env, who: &Address, amount: i128) {
    let key = DataKey::Shares(who.clone());
    env.storage().persistent().set(&key, &amount);
    env.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
}

// ── Concentrated-liquidity tick accessors (Circular pools only) ──────────────────
// Marked dead_code until the swap/deposit milestones wire them (docs/TICK_DESIGN.md).

pub fn get_tick_mode(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::TickMode)
        .unwrap_or(false)
}
pub fn set_tick_mode(env: &Env, on: bool) {
    env.storage().instance().set(&DataKey::TickMode, &on);
}

/// Current price angle as `(cos θc, sin θc)` in WAD (on the unit circle). The
/// swap moves it; the polar table is used only for integer tick boundaries.
/// `(0, 0)` means "unset" (before the first add).
pub fn get_price(env: &Env) -> (i128, i128) {
    let c = env.storage().instance().get(&DataKey::TickRef).unwrap_or(0);
    let s = env
        .storage()
        .instance()
        .get(&DataKey::TickYProg)
        .unwrap_or(0);
    (c, s)
}
pub fn set_price(env: &Env, cos: i128, sin: i128) {
    env.storage().instance().set(&DataKey::TickRef, &cos);
    env.storage().instance().set(&DataKey::TickYProg, &sin);
}

#[allow(dead_code)]
pub fn get_active_liq(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::ActiveLiq)
        .unwrap_or(0)
}
#[allow(dead_code)]
pub fn set_active_liq(env: &Env, l: i128) {
    env.storage().instance().set(&DataKey::ActiveLiq, &l);
}

#[allow(dead_code)]
pub fn get_tick_bitmap(env: &Env) -> u128 {
    env.storage()
        .instance()
        .get(&DataKey::TickBitmap)
        .unwrap_or(0)
}
#[allow(dead_code)]
pub fn set_tick_bitmap(env: &Env, bitmap: u128) {
    env.storage().instance().set(&DataKey::TickBitmap, &bitmap);
}

#[allow(dead_code)]
pub fn get_fee_growth_global(env: &Env) -> Vec<i128> {
    env.storage()
        .instance()
        .get(&DataKey::FeeGrowthGlobal)
        .unwrap_or_else(|| Vec::new(env))
}
#[allow(dead_code)]
pub fn set_fee_growth_global(env: &Env, fg: &Vec<i128>) {
    env.storage().instance().set(&DataKey::FeeGrowthGlobal, fg);
}

#[allow(dead_code)]
pub fn get_tick_net(env: &Env, angle: u32) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::TickNet(angle))
        .unwrap_or(0)
}
#[allow(dead_code)]
pub fn set_tick_net(env: &Env, angle: u32, net: i128) {
    env.storage().instance().set(&DataKey::TickNet(angle), &net);
}

#[allow(dead_code)]
pub fn get_tick_fee_outside(env: &Env, angle: u32) -> Vec<i128> {
    env.storage()
        .instance()
        .get(&DataKey::TickFeeOutside(angle))
        .unwrap_or_else(|| Vec::new(env))
}
#[allow(dead_code)]
pub fn set_tick_fee_outside(env: &Env, angle: u32, fo: &Vec<i128>) {
    env.storage()
        .instance()
        .set(&DataKey::TickFeeOutside(angle), fo);
}

#[allow(dead_code)]
pub fn get_position(env: &Env, owner: &Address, lower: u32, upper: u32) -> Option<Position> {
    env.storage()
        .persistent()
        .get(&DataKey::Position(owner.clone(), lower, upper))
}
#[allow(dead_code)]
pub fn set_position(env: &Env, owner: &Address, lower: u32, upper: u32, pos: &Position) {
    let key = DataKey::Position(owner.clone(), lower, upper);
    env.storage().persistent().set(&key, pos);
    env.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
}
#[allow(dead_code)]
pub fn remove_position(env: &Env, owner: &Address, lower: u32, upper: u32) {
    env.storage()
        .persistent()
        .remove(&DataKey::Position(owner.clone(), lower, upper));
}

// ─── Rate-aware pools ────────────────────────────────────────────────────────
// Accessors land with the storage keys (Phase 1); the entrypoints that call them
// arrive in Phases 2–5, which removes each `allow`. See todo.md §Phase 2.

pub fn get_rate_config(env: &Env) -> Option<RateConfig> {
    env.storage().instance().get(&DataKey::RateCfg)
}

pub fn set_rate_config(env: &Env, cfg: &RateConfig) {
    env.storage().instance().set(&DataKey::RateCfg, cfg);
}

pub fn has_rate_config(env: &Env) -> bool {
    env.storage().instance().has(&DataKey::RateCfg)
}

/// Last-accepted rates, WAD, parallel to `Config.tokens`. In parity mode (no
/// [`RateConfig`]) every entry is `WAD`, so callers need no special case.
pub fn get_rates(env: &Env, n: u32) -> Vec<i128> {
    env.storage()
        .instance()
        .get(&DataKey::Rates)
        .unwrap_or_else(|| {
            let mut v = Vec::new(env);
            for _ in 0..n {
                v.push_back(WAD);
            }
            v
        })
}

pub fn set_rates(env: &Env, rates: &Vec<i128>, last_time: u64) {
    env.storage().instance().set(&DataKey::Rates, rates);
    env.storage()
        .instance()
        .set(&DataKey::RateLastTime, &last_time);
}

pub fn get_rate_last_time(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::RateLastTime)
        .unwrap_or(0)
}

pub fn get_rate_breaker(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::RateBreaker)
        .unwrap_or(false)
}

pub fn set_rate_breaker(env: &Env, tripped: bool) {
    env.storage()
        .instance()
        .set(&DataKey::RateBreaker, &tripped);
}

pub fn get_operator_mode(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::OperatorMode)
        .unwrap_or(false)
}

pub fn set_operator_mode(env: &Env, enabled: bool) {
    env.storage()
        .instance()
        .set(&DataKey::OperatorMode, &enabled);
}

pub fn is_operator(env: &Env, who: &Address) -> bool {
    let key = DataKey::Operator(who.clone());
    let allowed: bool = env.storage().persistent().get(&key).unwrap_or(false);
    if allowed {
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_LIFETIME_THRESHOLD,
            PERSISTENT_BUMP_AMOUNT,
        );
    }
    allowed
}

pub fn set_operator(env: &Env, who: &Address, allowed: bool) {
    let key = DataKey::Operator(who.clone());
    env.storage().persistent().set(&key, &allowed);
    env.storage().persistent().extend_ttl(
        &key,
        PERSISTENT_LIFETIME_THRESHOLD,
        PERSISTENT_BUMP_AMOUNT,
    );
}

/// Whether an accepted rate change has left the pool off-curve.
///
/// A cheap boolean rather than an on-curve computation: verifying the invariant
/// costs several transcendental evaluations, and swaps cannot afford that per
/// call. `poke_rate` sets it, `re_anchor` clears it.
pub fn get_needs_reanchor(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::NeedsReAnchor)
        .unwrap_or(false)
}

pub fn set_needs_reanchor(env: &Env, needs: bool) {
    env.storage()
        .instance()
        .set(&DataKey::NeedsReAnchor, &needs);
}
