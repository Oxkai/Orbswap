#![no_std]
//! Orbswap pool — concentrated N-dimensional AMM (Soroban). MVP: 2-token pools in
//! `Circular` (CCMM) or `SuperElliptical` (CSEMM) mode.
//!
//! # Scaling model (Architecture §A–B)
//! Reserves are held in **native** token units. Internally they are scaled to 18
//! decimals (`internal = native · scale`), then normalized by the dynamic
//! **liquidity scale `s`** (WAD) to `x̂ = internal / s ∈ [0, α]` — the space the
//! math library operates in. Shares are denominated in `s` (`total_shares == s`),
//! with `MINIMUM_LIQUIDITY` permanently locked on the first deposit. Swaps move
//! reserves along the curve and never change `s`; only deposit/withdraw do.

mod errors;
mod events;
mod storage;
pub mod types;

#[cfg(test)]
mod tests;

// Re-exports for the factory/router crates.
pub use errors::OrbswapError;
pub use types::{Config, Paused, PoolMode};

use orbswap_math::ccmm;
use orbswap_math::circle_liq;
use orbswap_math::csemm;
use orbswap_math::fees;
use orbswap_math::fixed_point::{mul_div, Rounding};
use orbswap_math::ndim;
use orbswap_math::oracle;
use orbswap_math::polar;
use orbswap_math::ticks;
use soroban_sdk::{contract, contractimpl, symbol_short, token, Address, Env, Vec};
use types::{Position, MINIMUM_LIQUIDITY, TWO_PLUS_SQRT2, WAD};

/// Tolerance (WAD) for the post-swap invariant check in SuperElliptical mode, and
/// the proportional-deposit ratio check.
const INVARIANT_EPSILON: i128 = 1_000_000_000; // 1e-9 relative
/// Maximum tokens in a pool (bounds the fixed-size math buffers).
const MAX_TOKENS: usize = 8;
/// Minimum **normalized** (x̂-space) trade size for SuperElliptical pools — the
/// fuzz guard (§1.12) applies to the units the csemm math actually sees. In
/// normalized space `dx̂ = internal·WAD/s`, so this scales with pool size: bigger
/// pools reject proportionally-dustier trades (where x/α quantization degrades).
const MIN_TRADE_NORMALIZED: i128 = 1_000_000; // 1e-12 of the curve extent

fn md(a: i128, b: i128, d: i128, r: Rounding) -> Result<i128, OrbswapError> {
    mul_div(a, b, d, r).map_err(|_| OrbswapError::Overflow)
}

/// Whether the current price (given by `cos θc`, WAD) sits in `[lower, upper)` — i.e.
/// the position is active. `cos` is decreasing on the arc, so `θc ≥ lower ⇔ cos ≤
/// cos(lower)` and `θc < upper ⇔ cos > cos(upper)`.
fn angle_in_range(cos_c: i128, lower: u32, upper: u32) -> bool {
    cos_c <= polar::cos_deg(lower as i128) && cos_c > polar::cos_deg(upper as i128)
}

/// Next initialized tick boundary in the swap direction. `x_for_y` (sell X) moves
/// θ up (cos ↓) → closest initialized tick above the current angle (largest
/// `cos < cos_c`); else closest below. Ticks 0/90 are always initialized (first add
/// is full-range), so a boundary always exists.
fn next_boundary(bitmap: u128, cos_c: i128, x_for_y: bool) -> Option<u32> {
    let mut best: Option<(u32, i128)> = None;
    let mut t: u32 = 0;
    while t <= 90 {
        if bitmap & (1u128 << t) != 0 {
            let ct = polar::cos_deg(t as i128);
            let take = if x_for_y {
                ct < cos_c && best.map(|(_, bc)| ct > bc).unwrap_or(true)
            } else {
                ct > cos_c && best.map(|(_, bc)| ct < bc).unwrap_or(true)
            };
            if take {
                best = Some((t, ct));
            }
        }
        t += 1;
    }
    best.map(|(tk, _)| tk)
}

/// Tick-walking swap for a Circular tick pool: move the price with active `L`,
/// crossing initialized ticks (adjusting `L`), until the input is consumed. Updates
/// reserves (`in += net`, `out -= out`), active `L`, and the stored price; fees are
/// held outside the curve. Returns `(out_native, lp_fee, protocol_fee)`.
fn tick_swap(
    env: &Env,
    config: &Config,
    x_for_y: bool,
    amount_in_native: i128,
    protocol_bps: i128,
) -> Result<(i128, i128, i128), OrbswapError> {
    let (net_native, fee_native) =
        fees::apply_fee(amount_in_native, config.fee_bps).map_err(|_| OrbswapError::Overflow)?;
    if net_native <= 0 {
        return Err(OrbswapError::BelowMinTrade);
    }
    let (lp_fee, protocol_fee) =
        fees::split_protocol_fee(fee_native, protocol_bps).map_err(|_| OrbswapError::Overflow)?;

    let (in_idx, out_idx) = if x_for_y { (0u32, 1u32) } else { (1u32, 0u32) };
    let scale_in = config.scales.get_unchecked(in_idx);
    let scale_out = config.scales.get_unchecked(out_idx);
    let mut remaining = internal(net_native, scale_in)?;
    let mut out_int: i128 = 0;

    let (mut cos_c, mut sin_c) = storage::get_price(env);
    let mut l = storage::get_active_liq(env);
    let l_start = l;
    let bitmap = storage::get_tick_bitmap(env);

    let mut guard = 0u32;
    while remaining > 0 {
        guard += 1;
        if guard > 300 {
            return Err(OrbswapError::InvariantViolation);
        }
        if l <= 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let (xv, yv) = circle_liq::reserves_from_price(l, cos_c, sin_c, x_for_y)
            .map_err(OrbswapError::from)?;
        let bt =
            next_boundary(bitmap, cos_c, x_for_y).ok_or(OrbswapError::InsufficientLiquidity)?;
        let (bcos, bsin) = (polar::cos_deg(bt as i128), polar::sin_deg(bt as i128));
        let to_boundary = if x_for_y {
            (l - md(l, bcos, WAD, Rounding::Up)? - xv).max(0)
        } else {
            (l - md(l, bsin, WAD, Rounding::Up)? - yv).max(0)
        };

        if remaining < to_boundary {
            let (out, nxv, nyv) =
                circle_liq::swap_step(xv, yv, l, remaining, x_for_y).map_err(OrbswapError::from)?;
            out_int += out;
            cos_c = md(l - nxv, WAD, l, Rounding::Down)?;
            sin_c = md(l - nyv, WAD, l, Rounding::Down)?;
            remaining = 0;
        } else {
            if to_boundary > 0 {
                let (out, _, _) = circle_liq::swap_step(xv, yv, l, to_boundary, x_for_y)
                    .map_err(OrbswapError::from)?;
                out_int += out;
                remaining -= to_boundary;
            }
            cos_c = bcos;
            sin_c = bsin;
            let dir = if x_for_y {
                ticks::Direction::Up
            } else {
                ticks::Direction::Down
            };
            l = ticks::cross_tick(l, storage::get_tick_net(env, bt), dir)
                .map_err(|_| OrbswapError::InsufficientLiquidity)?;
            events::tick_crossed(env, bt, x_for_y, l);
        }
    }

    let out_native = out_int / scale_out;
    if out_native <= 0 {
        return Err(OrbswapError::InsufficientLiquidity);
    }
    // Accrue the LP fee as growth-per-unit-liquidity on the input token, attributed
    // to the liquidity active at swap start (exact for single-segment swaps).
    if l_start > 0 && lp_fee > 0 {
        let lp_fee_int = internal(lp_fee, scale_in)?;
        let mut fg = storage::get_fee_growth_global(env);
        if fg.len() < 2 {
            fg = Vec::from_array(env, [0i128, 0i128]);
        }
        let add = md(lp_fee_int, WAD, l_start, Rounding::Down)?;
        fg.set(in_idx, fg.get_unchecked(in_idx) + add);
        storage::set_fee_growth_global(env, &fg);
    }

    let mut reserves = storage::get_reserves(env);
    reserves.set(in_idx, reserves.get_unchecked(in_idx) + net_native);
    reserves.set(out_idx, reserves.get_unchecked(out_idx) - out_native);
    storage::set_reserves(env, &reserves);
    storage::set_active_liq(env, l);
    storage::set_price(env, cos_c, sin_c);
    Ok((out_native, lp_fee, protocol_fee))
}

/// v3 fee-growth-inside `[lower, upper]` per token (WAD per unit L), given the
/// current price `cos_c`. Full-range positions get the global growth.
fn fee_growth_inside(env: &Env, lower: u32, upper: u32, cos_c: i128) -> (i128, i128) {
    let fg = storage::get_fee_growth_global(env);
    let (g0, g1) = if fg.len() >= 2 {
        (fg.get_unchecked(0), fg.get_unchecked(1))
    } else {
        (0, 0)
    };
    let get_out = |t: u32| -> (i128, i128) {
        let v = storage::get_tick_fee_outside(env, t);
        if v.len() >= 2 {
            (v.get_unchecked(0), v.get_unchecked(1))
        } else {
            (0, 0)
        }
    };
    let (l0, l1) = get_out(lower);
    let (u0, u1) = get_out(upper);
    // current tick ≥ lower ⇔ θc ≥ lower ⇔ cos_c ≤ cos(lower).
    let ge_lower = cos_c <= polar::cos_deg(lower as i128);
    let ge_upper = cos_c <= polar::cos_deg(upper as i128);
    let below0 = if ge_lower { l0 } else { g0 - l0 };
    let below1 = if ge_lower { l1 } else { g1 - l1 };
    let above0 = if ge_upper { g0 - u0 } else { u0 };
    let above1 = if ge_upper { g1 - u1 } else { u1 };
    (g0 - below0 - above0, g1 - below1 - above1)
}

#[contract]
pub struct OrbswapPool;

#[contractimpl]
impl OrbswapPool {
    /// One-time configuration. `alpha`/`beta` are WAD shape params; for `Circular`
    /// both must equal `2+√2`. Token decimals are read from each token contract.
    pub fn initialize(
        env: Env,
        tokens: Vec<Address>,
        mode: PoolMode,
        alpha: i128,
        beta: i128,
        fee_bps: i128,
        admin: Address,
    ) -> Result<(), OrbswapError> {
        if storage::is_initialized(&env) {
            return Err(OrbswapError::AlreadyInitialized);
        }
        let ntok = tokens.len() as usize;
        if !(2..=MAX_TOKENS).contains(&ntok) {
            return Err(OrbswapError::InvalidConfig);
        }
        // No duplicate tokens (O(n²), n ≤ 8).
        for i in 0..tokens.len() {
            for j in (i + 1)..tokens.len() {
                if tokens.get_unchecked(i) == tokens.get_unchecked(j) {
                    return Err(OrbswapError::InvalidConfig);
                }
            }
        }
        if !(0..=10_000).contains(&fee_bps) {
            return Err(OrbswapError::InvalidConfig);
        }
        match mode {
            PoolMode::Circular => {
                // The circle is strictly 2-token.
                if ntok != 2 || alpha != TWO_PLUS_SQRT2 || beta != TWO_PLUS_SQRT2 {
                    return Err(OrbswapError::InvalidConfig);
                }
            }
            PoolMode::SuperElliptical => {
                // N-token pools are symmetric: `alpha` is the shared shape param.
                if alpha < 2 * WAD || beta < 2 * WAD {
                    return Err(OrbswapError::InvalidConfig);
                }
            }
        }

        // Per-token scale to 18 decimals.
        let mut scales = Vec::new(&env);
        for i in 0..tokens.len() {
            let t = tokens.get_unchecked(i);
            let dec = token::Client::new(&env, &t).decimals();
            if dec > 18 {
                return Err(OrbswapError::InvalidConfig);
            }
            scales.push_back(pow10(18 - dec));
        }

        let config = Config {
            tokens,
            mode,
            alpha,
            beta,
            scales,
            fee_bps,
            admin,
        };
        let n = config.tokens.len();
        let mut zeroes = Vec::new(&env);
        let mut allowed = Vec::new(&env);
        for _ in 0..n {
            zeroes.push_back(0i128);
            allowed.push_back(true);
        }
        storage::set_config(&env, &config);
        storage::set_reserves(&env, &zeroes);
        storage::set_protocol_owed(&env, &zeroes);
        storage::set_lp_fees_owed(&env, &zeroes);
        storage::set_allowed(&env, &allowed);
        storage::set_s(&env, 0);
        storage::set_total_shares(&env, 0);
        storage::set_oracle(&env, 0, env.ledger().timestamp());
        storage::bump_instance(&env);
        Ok(())
    }

    /// Deposit `amounts` (parallel to `tokens`), minting LP shares (`∝ Δs`).
    /// The first deposit must be balanced; later deposits must be proportional.
    pub fn deposit(
        env: Env,
        from: Address,
        amounts: Vec<i128>,
        min_shares: i128,
        deadline: u64,
    ) -> Result<i128, OrbswapError> {
        from.require_auth();
        check_deadline(&env, deadline)?;
        if storage::get_paused(&env).deposits {
            return Err(OrbswapError::Paused);
        }
        // Tick pools use `add_liquidity`/`remove_liquidity` (concentrated positions),
        // not the fungible-share path.
        if storage::get_tick_mode(&env) {
            return Err(OrbswapError::TickModeActive);
        }
        let config = storage::get_config(&env)?;
        let n = config.tokens.len();
        if amounts.len() != n {
            return Err(OrbswapError::InvalidAmount);
        }
        for i in 0..n {
            if amounts.get_unchecked(i) <= 0 {
                return Err(OrbswapError::InvalidAmount);
            }
        }
        // Depeg eject: deposits are proportional (would include the bad token) —
        // freeze them while any token is disallowed. Withdrawals stay open.
        let allowed = storage::get_allowed(&env);
        for i in 0..allowed.len() {
            if !allowed.get_unchecked(i) {
                return Err(OrbswapError::TokenNotAllowed);
            }
        }

        let mut reserves = storage::get_reserves(&env);
        let s = storage::get_s(&env);
        let total_shares = storage::get_total_shares(&env);

        let minted;
        if total_shares == 0 {
            // First deposit: all internal amounts must be equal (balanced start).
            let v = internal(amounts.get_unchecked(0), config.scales.get_unchecked(0))?;
            for i in 1..n {
                let vi = internal(amounts.get_unchecked(i), config.scales.get_unchecked(i))?;
                if vi != v {
                    return Err(OrbswapError::ImbalancedDeposit);
                }
            }
            // Set the liquidity scale so equal reserves land exactly on the curve:
            // x̂ = v/s = x̂_balanced ⇒ s = v / x̂_balanced. For the 2-token circle
            // x̂_balanced = 1.0 (s = v); for n-token it is α(1 − n^{−1/u}).
            let x_bal = balanced_xhat(&config, n as usize)?;
            let s0 = md(v, WAD, x_bal, Rounding::Down)?;
            if s0 <= MINIMUM_LIQUIDITY {
                return Err(OrbswapError::MinimumLiquidity);
            }
            storage::set_s(&env, s0);
            storage::set_total_shares(&env, s0);
            minted = s0 - MINIMUM_LIQUIDITY; // MINIMUM_LIQUIDITY locked (unowned)
            reserves = amounts.clone();
        } else {
            // Proportional deposit: amountsᵢ/reservesᵢ must be equal across tokens.
            let d0 = internal(amounts.get_unchecked(0), config.scales.get_unchecked(0))?;
            let r0 = internal(reserves.get_unchecked(0), config.scales.get_unchecked(0))?;
            if r0 == 0 {
                return Err(OrbswapError::MathDomain);
            }
            // Δs = s · (d0 / r0)
            let delta_s = md(s, d0, r0, Rounding::Down)?;
            for i in 1..n {
                let di = internal(amounts.get_unchecked(i), config.scales.get_unchecked(i))?;
                let ri = internal(reserves.get_unchecked(i), config.scales.get_unchecked(i))?;
                let expected = md(ri, d0, r0, Rounding::Down)?;
                // Within a small tolerance of proportional.
                if (di - expected).abs() > expected / 1_000_000 + 2 {
                    return Err(OrbswapError::ImbalancedDeposit);
                }
            }
            storage::set_s(&env, s + delta_s);
            storage::set_total_shares(&env, total_shares + delta_s);
            minted = delta_s;
            for i in 0..n {
                let r = reserves.get_unchecked(i) + amounts.get_unchecked(i);
                reserves.set(i, r);
            }
        }

        if minted < min_shares {
            return Err(OrbswapError::SlippageExceeded);
        }
        storage::set_reserves(&env, &reserves);
        let bal = storage::get_shares(&env, &from);
        storage::set_shares(&env, &from, bal + minted);
        storage::bump_instance(&env);

        // Pull tokens in (state already written).
        for i in 0..n {
            transfer_in(
                &env,
                &config.tokens.get_unchecked(i),
                &from,
                amounts.get_unchecked(i),
            );
        }
        events::deposit(&env, &from, minted, &amounts);
        Ok(minted)
    }

    // ── Concentrated-liquidity tick entrypoints (Circular + tick mode) ───────────
    // See docs/TICK_DESIGN.md. Positions are keyed by (owner, lower°, upper°); the
    // pool tracks a global angle `θc` + active liquidity `L`. `deposit`/`withdraw`
    // are disabled once tick mode is on (they'd conflate fungible shares with
    // concentrated positions).

    /// Enable concentrated-liquidity tick mode. Circular pools only, before any
    /// liquidity is added; admin-gated and irreversible.
    pub fn enable_ticks(env: Env) -> Result<(), OrbswapError> {
        let config = storage::get_config(&env)?;
        config.admin.require_auth();
        if config.mode != PoolMode::Circular {
            return Err(OrbswapError::TickModeOnly);
        }
        if storage::get_total_shares(&env) != 0 || storage::get_active_liq(&env) != 0 {
            return Err(OrbswapError::AlreadyInitialized);
        }
        storage::set_tick_mode(&env, true);
        storage::set_fee_growth_global(&env, &Vec::from_array(&env, [0i128, 0i128]));
        storage::bump_instance(&env);
        Ok(())
    }

    /// Add concentrated liquidity over `[lower, upper]` (integer degrees, arc
    /// `[0,90]`), pulling **at most** `amounts = [x_max, y_max]`. The first add must
    /// be full-range `[0,90]` balanced (sets `θc = 45°`, locks `MINIMUM_LIQUIDITY`).
    /// Returns the liquidity `L` credited to the caller's position.
    #[allow(clippy::too_many_arguments)]
    pub fn add_liquidity(
        env: Env,
        from: Address,
        amounts: Vec<i128>,
        lower: u32,
        upper: u32,
        min_liquidity: i128,
        deadline: u64,
    ) -> Result<i128, OrbswapError> {
        from.require_auth();
        check_deadline(&env, deadline)?;
        if storage::get_paused(&env).deposits {
            return Err(OrbswapError::Paused);
        }
        if !storage::get_tick_mode(&env) {
            return Err(OrbswapError::TickModeOnly);
        }
        let config = storage::get_config(&env)?;
        if amounts.len() != 2 {
            return Err(OrbswapError::InvalidAmount);
        }
        if lower >= upper || upper > 90 {
            return Err(OrbswapError::InvalidTickRange);
        }
        let x_max = amounts.get_unchecked(0);
        let y_max = amounts.get_unchecked(1);
        if x_max < 0 || y_max < 0 {
            return Err(OrbswapError::InvalidAmount);
        }

        let scale_x = config.scales.get_unchecked(0);
        let scale_y = config.scales.get_unchecked(1);
        let x_int = internal(x_max, scale_x)?;
        let y_int = internal(y_max, scale_y)?;

        // First add establishes the price: require full-range balanced → θc = 45°.
        let first = storage::get_tick_bitmap(&env) == 0;
        let (cos_c, sin_c) = if first {
            if lower != 0 || upper != 90 {
                return Err(OrbswapError::InvalidTickRange);
            }
            (polar::cos_deg(45), polar::sin_deg(45))
        } else {
            storage::get_price(&env)
        };

        let l =
            circle_liq::liquidity_for_cs(x_int, y_int, lower as i128, upper as i128, cos_c, sin_c)?;
        if first && l <= MINIMUM_LIQUIDITY {
            return Err(OrbswapError::MinimumLiquidity);
        }
        // Liquidity the caller keeps: the first add permanently locks MINIMUM_LIQUIDITY.
        let credited = if first { l - MINIMUM_LIQUIDITY } else { l };
        if credited < min_liquidity {
            return Err(OrbswapError::SlippageExceeded);
        }

        // Actual token pull (round up → pool-favoring), guaranteed ≤ the maxes.
        let (ax_int, ay_int) = circle_liq::position_amounts_cs(
            l,
            lower as i128,
            upper as i128,
            cos_c,
            sin_c,
            Rounding::Up,
        )?;
        let pull_x = ceil_div(ax_int, scale_x)?;
        let pull_y = ceil_div(ay_int, scale_y)?;
        if pull_x > x_max || pull_y > y_max {
            return Err(OrbswapError::SlippageExceeded);
        }

        // ---- state writes (before transfers) ----
        let mut reserves = storage::get_reserves(&env);
        if reserves.is_empty() {
            reserves = Vec::from_array(&env, [0i128, 0i128]);
        }
        reserves.set(0, reserves.get_unchecked(0) + pull_x);
        reserves.set(1, reserves.get_unchecked(1) + pull_y);
        storage::set_reserves(&env, &reserves);

        if first {
            storage::set_price(&env, polar::cos_deg(45), polar::sin_deg(45));
        }
        // Tick net liquidity: +L entering `lower`, −L leaving `upper` (v3).
        storage::set_tick_net(&env, lower, storage::get_tick_net(&env, lower) + l);
        storage::set_tick_net(&env, upper, storage::get_tick_net(&env, upper) - l);
        let mut bm = storage::get_tick_bitmap(&env);
        bm |= 1u128 << lower;
        bm |= 1u128 << upper;
        storage::set_tick_bitmap(&env, bm);
        // Active liquidity if the current angle sits inside the range.
        if angle_in_range(cos_c, lower, upper) {
            storage::set_active_liq(&env, storage::get_active_liq(&env) + l);
        }
        // Position: accrue to any existing (owner, lower, upper). Snapshot the current
        // fee-growth-inside so the position earns from now on. (Re-adding to a
        // fee-bearing position resets the snapshot; withdraw fully to claim first.)
        let prev = storage::get_position(&env, &from, lower, upper);
        let prev_liq = prev.map(|p| p.liquidity).unwrap_or(0);
        let (fgi0, fgi1) = fee_growth_inside(&env, lower, upper, cos_c);
        storage::set_position(
            &env,
            &from,
            lower,
            upper,
            &Position {
                liquidity: prev_liq + credited,
                fee_growth_inside_last: Vec::from_array(&env, [fgi0, fgi1]),
            },
        );
        storage::bump_instance(&env);

        transfer_in(&env, &config.tokens.get_unchecked(0), &from, pull_x);
        transfer_in(&env, &config.tokens.get_unchecked(1), &from, pull_y);
        Ok(credited)
    }

    /// Remove `liquidity` from the caller's `[lower, upper]` position, returning the
    /// tokens released at the current angle. Returns `[x_out, y_out]`.
    #[allow(clippy::too_many_arguments)]
    pub fn remove_liquidity(
        env: Env,
        from: Address,
        lower: u32,
        upper: u32,
        liquidity: i128,
        min_amounts: Vec<i128>,
        deadline: u64,
    ) -> Result<Vec<i128>, OrbswapError> {
        from.require_auth();
        check_deadline(&env, deadline)?;
        if storage::get_paused(&env).withdrawals {
            return Err(OrbswapError::Paused);
        }
        if !storage::get_tick_mode(&env) {
            return Err(OrbswapError::TickModeOnly);
        }
        if liquidity <= 0 || min_amounts.len() != 2 {
            return Err(OrbswapError::InvalidAmount);
        }
        let config = storage::get_config(&env)?;
        let pos = storage::get_position(&env, &from, lower, upper)
            .ok_or(OrbswapError::PositionNotFound)?;
        if liquidity > pos.liquidity {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let (cos_c, sin_c) = storage::get_price(&env);
        let scale_x = config.scales.get_unchecked(0);
        let scale_y = config.scales.get_unchecked(1);

        // Tokens released (round down → pool-favoring).
        let (x_int, y_int) = circle_liq::position_amounts_cs(
            liquidity,
            lower as i128,
            upper as i128,
            cos_c,
            sin_c,
            Rounding::Down,
        )?;
        let reserve_out_x = x_int / scale_x;
        let reserve_out_y = y_int / scale_y;

        // Earned fees: L · (feeGrowthInside_now − snapshot), paid from the LP pot.
        let (inside0, inside1) = fee_growth_inside(&env, lower, upper, cos_c);
        let last = &pos.fee_growth_inside_last;
        let (last0, last1) = if last.len() >= 2 {
            (last.get_unchecked(0), last.get_unchecked(1))
        } else {
            (0, 0)
        };
        let mut lp_owed = storage::get_lp_fees_owed(&env);
        let owed_x = (md(pos.liquidity, (inside0 - last0).max(0), WAD, Rounding::Down)? / scale_x)
            .min(lp_owed.get_unchecked(0));
        let owed_y = (md(pos.liquidity, (inside1 - last1).max(0), WAD, Rounding::Down)? / scale_y)
            .min(lp_owed.get_unchecked(1));

        let out_x = reserve_out_x + owed_x;
        let out_y = reserve_out_y + owed_y;
        if out_x < min_amounts.get_unchecked(0) || out_y < min_amounts.get_unchecked(1) {
            return Err(OrbswapError::SlippageExceeded);
        }

        // ---- state writes ----
        let mut reserves = storage::get_reserves(&env);
        reserves.set(0, reserves.get_unchecked(0) - reserve_out_x);
        reserves.set(1, reserves.get_unchecked(1) - reserve_out_y);
        storage::set_reserves(&env, &reserves);
        lp_owed.set(0, lp_owed.get_unchecked(0) - owed_x);
        lp_owed.set(1, lp_owed.get_unchecked(1) - owed_y);
        storage::set_lp_fees_owed(&env, &lp_owed);

        storage::set_tick_net(&env, lower, storage::get_tick_net(&env, lower) - liquidity);
        storage::set_tick_net(&env, upper, storage::get_tick_net(&env, upper) + liquidity);
        if angle_in_range(cos_c, lower, upper) {
            storage::set_active_liq(&env, storage::get_active_liq(&env) - liquidity);
        }
        let remaining = pos.liquidity - liquidity;
        if remaining == 0 {
            storage::remove_position(&env, &from, lower, upper);
        } else {
            storage::set_position(
                &env,
                &from,
                lower,
                upper,
                &Position {
                    liquidity: remaining,
                    fee_growth_inside_last: Vec::from_array(&env, [inside0, inside1]),
                },
            );
        }
        storage::bump_instance(&env);

        transfer_out(&env, &config.tokens.get_unchecked(0), &from, out_x);
        transfer_out(&env, &config.tokens.get_unchecked(1), &from, out_y);
        Ok(Vec::from_array(&env, [out_x, out_y]))
    }

    /// Burn `shares`, returning proportional reserves.
    pub fn withdraw(
        env: Env,
        from: Address,
        shares: i128,
        min_amounts: Vec<i128>,
        deadline: u64,
    ) -> Result<Vec<i128>, OrbswapError> {
        from.require_auth();
        check_deadline(&env, deadline)?;
        if storage::get_paused(&env).withdrawals {
            return Err(OrbswapError::Paused);
        }
        if storage::get_tick_mode(&env) {
            return Err(OrbswapError::TickModeActive);
        }
        let config = storage::get_config(&env)?;
        let n = config.tokens.len();
        if shares <= 0 || min_amounts.len() != n {
            return Err(OrbswapError::InvalidAmount);
        }
        let bal = storage::get_shares(&env, &from);
        if shares > bal {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let s = storage::get_s(&env);
        let total_shares = storage::get_total_shares(&env);
        let mut reserves = storage::get_reserves(&env);
        let mut lp_fees = storage::get_lp_fees_owed(&env);

        // amount_iᵢ = (reservesᵢ + lp_fees_owedᵢ) · shares / total_shares (round
        // down, favors pool). Accrued LP fees are paid out pro-rata alongside the
        // curve reserves — they live outside the curve but belong to LPs.
        let mut out = Vec::new(&env);
        for i in 0..n {
            let res_amt = md(
                reserves.get_unchecked(i),
                shares,
                total_shares,
                Rounding::Down,
            )?;
            let fee_amt = md(
                lp_fees.get_unchecked(i),
                shares,
                total_shares,
                Rounding::Down,
            )?;
            let amt = res_amt + fee_amt;
            if amt < min_amounts.get_unchecked(i) {
                return Err(OrbswapError::SlippageExceeded);
            }
            reserves.set(i, reserves.get_unchecked(i) - res_amt);
            lp_fees.set(i, lp_fees.get_unchecked(i) - fee_amt);
            out.push_back(amt);
        }
        storage::set_s(&env, s - shares);
        storage::set_total_shares(&env, total_shares - shares);
        storage::set_shares(&env, &from, bal - shares);
        storage::set_reserves(&env, &reserves);
        storage::set_lp_fees_owed(&env, &lp_fees);
        storage::bump_instance(&env);

        for i in 0..n {
            transfer_out(
                &env,
                &config.tokens.get_unchecked(i),
                &from,
                out.get_unchecked(i),
            );
        }
        events::withdraw(&env, &from, shares, &out);
        Ok(out)
    }

    /// Swap `amount_in` of `token_in` for `token_out`, returning the output.
    pub fn swap(
        env: Env,
        from: Address,
        token_in: Address,
        amount_in: i128,
        token_out: Address,
        min_out: i128,
        deadline: u64,
    ) -> Result<i128, OrbswapError> {
        from.require_auth();
        check_deadline(&env, deadline)?;
        if storage::get_paused(&env).swaps {
            return Err(OrbswapError::Paused);
        }
        if amount_in <= 0 {
            return Err(OrbswapError::InvalidAmount);
        }
        if token_in == token_out {
            return Err(OrbswapError::InvalidAmount);
        }
        let config = storage::get_config(&env)?;
        let i_in = token_index(&config, &token_in)?;
        let i_out = token_index(&config, &token_out)?;
        if !storage::get_allowed(&env).get_unchecked(i_in as u32) {
            return Err(OrbswapError::TokenNotAllowed);
        }

        // Concentrated tick pool: walk the ticks (Circular + tick mode).
        if storage::get_tick_mode(&env) {
            let protocol_bps = storage::get_protocol_fee_bps(&env);
            let x_for_y = i_in == 0;
            let (out_native, lp_fee, protocol_fee) =
                tick_swap(&env, &config, x_for_y, amount_in, protocol_bps)?;
            if out_native < min_out {
                return Err(OrbswapError::SlippageExceeded);
            }
            accrue_fees(&env, i_in, lp_fee, protocol_fee);
            storage::bump_instance(&env);
            transfer_in(&env, &token_in, &from, amount_in);
            transfer_out(&env, &token_out, &from, out_native);
            events::swap(&env, &from, &token_in, amount_in, &token_out, out_native);
            return Ok(out_native);
        }
        let s = storage::get_s(&env);
        if s == 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let mut reserves = storage::get_reserves(&env);

        // Oracle: accumulate the price that held over the elapsed time BEFORE the
        // swap moves reserves (v2-style, using pre-swap reserves).
        update_oracle(&env, &config, &reserves, s);

        let protocol_bps = storage::get_protocol_fee_bps(&env);
        let (out_native, new_in, new_out, lp_fee, protocol_fee) =
            compute_swap(&config, &reserves, s, i_in, i_out, amount_in, protocol_bps)?;
        if out_native <= 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        if out_native < min_out {
            return Err(OrbswapError::SlippageExceeded);
        }

        reserves.set(i_in as u32, new_in);
        reserves.set(i_out as u32, new_out);
        storage::set_reserves(&env, &reserves);
        accrue_fees(&env, i_in, lp_fee, protocol_fee);
        storage::bump_instance(&env);

        // State written; move tokens.
        transfer_in(&env, &token_in, &from, amount_in);
        transfer_out(&env, &token_out, &from, out_native);
        events::swap(&env, &from, &token_in, amount_in, &token_out, out_native);
        Ok(out_native)
    }

    /// Swap for an **exact** `amount_out` of `token_out`, pulling at most `max_in`
    /// of `token_in`. Returns the input actually charged.
    pub fn swap_exact_out(
        env: Env,
        from: Address,
        token_in: Address,
        token_out: Address,
        amount_out: i128,
        max_in: i128,
        deadline: u64,
    ) -> Result<i128, OrbswapError> {
        from.require_auth();
        check_deadline(&env, deadline)?;
        if storage::get_tick_mode(&env) {
            return Err(OrbswapError::TickModeActive);
        }
        if storage::get_paused(&env).swaps {
            return Err(OrbswapError::Paused);
        }
        if amount_out <= 0 || token_in == token_out {
            return Err(OrbswapError::InvalidAmount);
        }
        let config = storage::get_config(&env)?;
        let i_in = token_index(&config, &token_in)?;
        let i_out = token_index(&config, &token_out)?;
        if !storage::get_allowed(&env).get_unchecked(i_in as u32) {
            return Err(OrbswapError::TokenNotAllowed);
        }
        let s = storage::get_s(&env);
        if s == 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let mut reserves = storage::get_reserves(&env);
        update_oracle(&env, &config, &reserves, s);

        let protocol_bps = storage::get_protocol_fee_bps(&env);
        let (amount_in, new_in, new_out, lp_fee, protocol_fee) =
            compute_swap_exact_out(&config, &reserves, s, i_in, i_out, amount_out, protocol_bps)?;
        if amount_in <= 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        if amount_in > max_in {
            return Err(OrbswapError::SlippageExceeded);
        }

        reserves.set(i_in as u32, new_in);
        reserves.set(i_out as u32, new_out);
        storage::set_reserves(&env, &reserves);
        accrue_fees(&env, i_in, lp_fee, protocol_fee);
        storage::bump_instance(&env);

        transfer_in(&env, &token_in, &from, amount_in);
        transfer_out(&env, &token_out, &from, amount_out);
        events::swap(&env, &from, &token_in, amount_in, &token_out, amount_out);
        Ok(amount_in)
    }

    /// Quote the output of an exact-in swap without executing (view).
    pub fn quote(
        env: Env,
        token_in: Address,
        amount_in: i128,
        token_out: Address,
    ) -> Result<i128, OrbswapError> {
        let config = storage::get_config(&env)?;
        let i_in = token_index(&config, &token_in)?;
        let i_out = token_index(&config, &token_out)?;
        let s = storage::get_s(&env);
        if s == 0 || amount_in <= 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let reserves = storage::get_reserves(&env);
        let protocol_bps = storage::get_protocol_fee_bps(&env);
        let (out, ..) = compute_swap(&config, &reserves, s, i_in, i_out, amount_in, protocol_bps)?;
        Ok(out)
    }

    /// Quote the input required for an exact-out swap (view).
    pub fn quote_exact_out(
        env: Env,
        token_in: Address,
        token_out: Address,
        amount_out: i128,
    ) -> Result<i128, OrbswapError> {
        let config = storage::get_config(&env)?;
        let i_in = token_index(&config, &token_in)?;
        let i_out = token_index(&config, &token_out)?;
        let s = storage::get_s(&env);
        if s == 0 || amount_out <= 0 {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        let reserves = storage::get_reserves(&env);
        let protocol_bps = storage::get_protocol_fee_bps(&env);
        let (amount_in, ..) =
            compute_swap_exact_out(&config, &reserves, s, i_in, i_out, amount_out, protocol_bps)?;
        Ok(amount_in)
    }

    // -------- views --------
    pub fn get_reserves(env: Env) -> Vec<i128> {
        storage::get_reserves(&env)
    }
    pub fn get_config(env: Env) -> Result<Config, OrbswapError> {
        storage::get_config(&env)
    }
    pub fn get_liquidity_scale(env: Env) -> i128 {
        storage::get_s(&env)
    }
    pub fn total_shares(env: Env) -> i128 {
        storage::get_total_shares(&env)
    }
    pub fn shares_of(env: Env, who: Address) -> i128 {
        storage::get_shares(&env, &who)
    }
    pub fn paused(env: Env) -> Paused {
        storage::get_paused(&env)
    }

    /// Marginal price of token0 in token1 (decimal-normalized, WAD; 1.0 at balance).
    /// `i128::MAX` if at a price boundary or uninitialized.
    pub fn get_spot_price(env: Env) -> i128 {
        let config = match storage::get_config(&env) {
            Ok(c) => c,
            Err(_) => return i128::MAX,
        };
        let reserves = storage::get_reserves(&env);
        let s = storage::get_s(&env);
        current_spot(&config, &reserves, s).unwrap_or(i128::MAX)
    }

    /// Oracle accumulator `(Σ price·Δt, last_update_time)`. TWAP over `[t0,t1]` =
    /// `(cum1 − cum0) / (t1 − t0)` computed off-chain (v2-style).
    pub fn price_cumulative(env: Env) -> (i128, u64) {
        storage::get_oracle(&env)
    }

    // -------- admin: pausability --------
    pub fn pause_deposits(env: Env, pause: bool) -> Result<(), OrbswapError> {
        set_pause_flag(&env, symbol_short!("deposits"), pause, |p| &mut p.deposits)
    }
    pub fn pause_swaps(env: Env, pause: bool) -> Result<(), OrbswapError> {
        set_pause_flag(&env, symbol_short!("swaps"), pause, |p| &mut p.swaps)
    }
    pub fn pause_withdrawals(env: Env, pause: bool) -> Result<(), OrbswapError> {
        set_pause_flag(&env, symbol_short!("withdraw"), pause, |p| {
            &mut p.withdrawals
        })
    }
    /// Emergency: pause every mutating entrypoint at once.
    pub fn pause_all(env: Env, pause: bool) -> Result<(), OrbswapError> {
        require_admin(&env)?;
        storage::set_paused(
            &env,
            &Paused {
                deposits: pause,
                swaps: pause,
                withdrawals: pause,
            },
        );
        events::paused(&env, symbol_short!("all"), pause);
        Ok(())
    }

    // -------- admin: protocol fee --------
    /// Set the protocol's cut of the swap fee (bps of the fee, 0–10000). Admin only.
    pub fn set_protocol_fee_bps(env: Env, bps: i128) -> Result<(), OrbswapError> {
        require_admin(&env)?;
        if !(0..=10_000).contains(&bps) {
            return Err(OrbswapError::InvalidConfig);
        }
        storage::set_protocol_fee_bps(&env, bps);
        storage::bump_instance(&env);
        Ok(())
    }
    pub fn protocol_fee_bps(env: Env) -> i128 {
        storage::get_protocol_fee_bps(&env)
    }
    pub fn protocol_owed(env: Env) -> Vec<i128> {
        storage::get_protocol_owed(&env)
    }
    /// LP fees accrued outside the curve, parallel to tokens. Paid out pro-rata to
    /// LPs on withdraw; not part of the swap curve.
    pub fn lp_fees_owed(env: Env) -> Vec<i128> {
        storage::get_lp_fees_owed(&env)
    }

    // -------- tick views (Circular tick pools) --------
    /// Whether concentrated-liquidity tick mode is enabled.
    pub fn tick_mode(env: Env) -> bool {
        storage::get_tick_mode(&env)
    }
    /// Active liquidity `L` spanning the current price.
    pub fn active_liquidity(env: Env) -> i128 {
        storage::get_active_liq(&env)
    }
    /// Current price as `[cos θc, sin θc]` (WAD).
    pub fn tick_price(env: Env) -> Vec<i128> {
        let (c, s) = storage::get_price(&env);
        Vec::from_array(&env, [c, s])
    }
    /// Current integer tick (floor of the price angle, `0..=90`).
    pub fn current_tick(env: Env) -> u32 {
        let (c, _) = storage::get_price(&env);
        let mut d: u32 = 0;
        while d < 90 && polar::cos_deg((d + 1) as i128) >= c {
            d += 1;
        }
        d
    }
    /// Liquidity of a caller's `[lower, upper]` position (0 if none).
    pub fn position_liquidity(env: Env, owner: Address, lower: u32, upper: u32) -> i128 {
        storage::get_position(&env, &owner, lower, upper)
            .map(|p| p.liquidity)
            .unwrap_or(0)
    }
    /// Transfer all accrued protocol fees to `to`, zeroing the accrual. Admin only.
    pub fn collect_protocol_fees(env: Env, to: Address) -> Result<Vec<i128>, OrbswapError> {
        require_admin(&env)?;
        let config = storage::get_config(&env)?;
        let n = config.tokens.len();
        let owed = storage::get_protocol_owed(&env);
        let mut zeroes = Vec::new(&env);
        for _ in 0..n {
            zeroes.push_back(0i128);
        }
        // State before transfers (reentrancy-safe).
        storage::set_protocol_owed(&env, &zeroes);
        storage::bump_instance(&env);
        for i in 0..n {
            let amt = owed.get_unchecked(i);
            if amt > 0 {
                transfer_out(&env, &config.tokens.get_unchecked(i), &to, amt);
            }
        }
        events::protocol_collected(&env, &to, &owed);
        Ok(owed)
    }

    // -------- keeper: depeg auto-eject (paper §2.4) --------
    /// Allow/disallow a token. A disallowed token cannot be swapped **in** and
    /// deposits freeze; swapping it **out** and withdrawals stay open, so LPs can
    /// exit and arbitrage can drain the depegged coin. Admin-gated.
    pub fn set_allowed(env: Env, token: Address, allowed: bool) -> Result<(), OrbswapError> {
        require_admin(&env)?;
        let config = storage::get_config(&env)?;
        let i = token_index(&config, &token)?;
        let mut flags = storage::get_allowed(&env);
        flags.set(i as u32, allowed);
        storage::set_allowed(&env, &flags);
        storage::bump_instance(&env);
        events::token_allowed(&env, &token, allowed);
        Ok(())
    }
    pub fn is_allowed(env: Env, token: Address) -> Result<bool, OrbswapError> {
        let config = storage::get_config(&env)?;
        let i = token_index(&config, &token)?;
        Ok(storage::get_allowed(&env).get_unchecked(i as u32))
    }

    // -------- LP share transfer --------
    /// Move `amount` LP shares from `from` to `to`.
    pub fn transfer_shares(
        env: Env,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), OrbswapError> {
        from.require_auth();
        if amount <= 0 {
            return Err(OrbswapError::InvalidAmount);
        }
        let from_bal = storage::get_shares(&env, &from);
        if amount > from_bal {
            return Err(OrbswapError::InsufficientLiquidity);
        }
        storage::set_shares(&env, &from, from_bal - amount);
        let to_bal = storage::get_shares(&env, &to);
        storage::set_shares(&env, &to, to_bal + amount);
        storage::bump_instance(&env);
        events::shares_transferred(&env, &from, &to, amount);
        Ok(())
    }
}

fn require_admin(env: &Env) -> Result<(), OrbswapError> {
    let admin = storage::get_config(env)?.admin;
    admin.require_auth();
    Ok(())
}

fn set_pause_flag(
    env: &Env,
    what: soroban_sdk::Symbol,
    pause: bool,
    field: impl Fn(&mut Paused) -> &mut bool,
) -> Result<(), OrbswapError> {
    require_admin(env)?;
    let mut p = storage::get_paused(env);
    *field(&mut p) = pause;
    storage::set_paused(env, &p);
    events::paused(env, what, pause);
    Ok(())
}

// ---------------------------------------------------------------- helpers

fn check_deadline(env: &Env, deadline: u64) -> Result<(), OrbswapError> {
    if env.ledger().timestamp() > deadline {
        return Err(OrbswapError::Expired);
    }
    Ok(())
}

/// `native · scale` — native units to internal 18-decimal WAD.
fn internal(native: i128, scale: i128) -> Result<i128, OrbswapError> {
    native.checked_mul(scale).ok_or(OrbswapError::Overflow)
}

fn pow10(n: u32) -> i128 {
    let mut v = 1i128;
    for _ in 0..n {
        v *= 10;
    }
    v
}

/// Marginal price of token0 in token1 at the current reserves (WAD), or `None` at
/// a boundary / empty pool. Decimal-normalized (uses internal reserves).
fn current_spot(config: &Config, reserves: &Vec<i128>, s: i128) -> Option<i128> {
    // Oracle tracks the token0/token1 pair; only meaningful for 2-token pools.
    if s == 0 || config.tokens.len() != 2 {
        return None;
    }
    let i0 = internal(reserves.get_unchecked(0), config.scales.get_unchecked(0)).ok()?;
    let i1 = internal(reserves.get_unchecked(1), config.scales.get_unchecked(1)).ok()?;
    let x0 = mul_div(i0, WAD, s, Rounding::Down).ok()?;
    let x1 = mul_div(i1, WAD, s, Rounding::Down).ok()?;
    let p = match config.mode {
        PoolMode::Circular => ccmm::spot_price(x0, x1, TWO_PLUS_SQRT2),
        PoolMode::SuperElliptical => csemm::spot_price(x0, x1, config.alpha, config.beta),
    };
    if p == i128::MAX || p <= 0 {
        None
    } else {
        Some(p)
    }
}

/// Accumulate `price · elapsed` since the last update using the (pre-swap) reserves.
fn update_oracle(env: &Env, config: &Config, reserves: &Vec<i128>, s: i128) {
    let now = env.ledger().timestamp();
    let (cum, last) = storage::get_oracle(env);
    if now <= last {
        return;
    }
    let elapsed = (now - last) as i128;
    let new_cum = match current_spot(config, reserves, s) {
        Some(spot) => oracle::accumulate(cum, spot, elapsed).unwrap_or(cum),
        None => cum,
    };
    storage::set_oracle(env, new_cum, now);
}

fn token_index(config: &Config, token: &Address) -> Result<usize, OrbswapError> {
    for i in 0..config.tokens.len() {
        if &config.tokens.get_unchecked(i) == token {
            return Ok(i as usize);
        }
    }
    Err(OrbswapError::UnknownToken)
}

/// Accrue a swap's fees into their off-curve pots (token `i_in`): the LP cut into
/// `LpFeesOwed` (paid out proportionally on withdraw) and the protocol cut into
/// `ProtocolOwed` (swept by `collect_protocol_fees`). Both sit in the contract
/// balance but outside the curve reserves, keeping the invariant exact.
fn accrue_fees(env: &Env, i_in: usize, lp_fee: i128, protocol_fee: i128) {
    if lp_fee > 0 {
        let mut owed = storage::get_lp_fees_owed(env);
        owed.set(i_in as u32, owed.get_unchecked(i_in as u32) + lp_fee);
        storage::set_lp_fees_owed(env, &owed);
    }
    if protocol_fee > 0 {
        let mut owed = storage::get_protocol_owed(env);
        owed.set(i_in as u32, owed.get_unchecked(i_in as u32) + protocol_fee);
        storage::set_protocol_owed(env, &owed);
    }
}

/// Core swap math: native reserves + scaling → normalized math-lib swap → native
/// output. Returns `(out_native, new_reserve_in_native, new_reserve_out_native,
/// lp_fee_native, protocol_fee_native)`.
///
/// **Fees are held OUTSIDE the curve.** The curve reserve moves by `net` only
/// (`reserve_in += net`, `reserve_out -= out`), so the pool stays exactly on the
/// invariant and every swap prices per the paper. The LP and protocol fee cuts are
/// returned to the caller to accrue in `LpFeesOwed` / `ProtocolOwed`.
fn compute_swap(
    config: &Config,
    reserves: &Vec<i128>,
    s: i128,
    i_in: usize,
    i_out: usize,
    amount_in_native: i128,
    protocol_bps: i128,
) -> Result<(i128, i128, i128, i128, i128), OrbswapError> {
    let scale_in = config.scales.get_unchecked(i_in as u32);
    let scale_out = config.scales.get_unchecked(i_out as u32);
    let res_in_n = reserves.get_unchecked(i_in as u32);
    let res_out_n = reserves.get_unchecked(i_out as u32);

    // Take the swap fee from the input (rounds up → pool). Only NET is swapped; the
    // whole fee is held OUTSIDE the curve, split LP vs protocol.
    let (net_native, fee_native) =
        fees::apply_fee(amount_in_native, config.fee_bps).map_err(|_| OrbswapError::Overflow)?;
    if net_native <= 0 {
        return Err(OrbswapError::BelowMinTrade);
    }
    let (lp_fee, protocol_fee) =
        fees::split_protocol_fee(fee_native, protocol_bps).map_err(|_| OrbswapError::Overflow)?;

    // native → internal (18-dec). Curve moves by NET only (stays on-invariant).
    let internal_in_amt = internal(net_native, scale_in)?;
    let res_in_int = internal(res_in_n, scale_in)?;
    let res_out_int = internal(res_out_n, scale_out)?;

    // internal → normalized (÷s).
    let xin = md(res_in_int, WAD, s, Rounding::Down)?;
    let xout = md(res_out_int, WAD, s, Rounding::Down)?;
    let dxhat = md(internal_in_amt, WAD, s, Rounding::Down)?;
    if dxhat <= 0 {
        return Err(OrbswapError::BelowMinTrade);
    }

    // Math-lib swap in normalized space. 2-token uses ccmm/csemm; n>2 uses ndim
    // (the n-dimensional superellipse; `initialize` forces SuperElliptical for n>2).
    let n = config.tokens.len() as usize;
    let (out_norm, nx_in, nx_out) = if n == 2 {
        match config.mode {
            PoolMode::Circular => ccmm::swap_out(xin, xout, TWO_PLUS_SQRT2, dxhat)?,
            PoolMode::SuperElliptical => {
                if dxhat < MIN_TRADE_NORMALIZED {
                    return Err(OrbswapError::BelowMinTrade);
                }
                let r = csemm::swap_out(xin, xout, config.alpha, config.beta, dxhat)?;
                // Fuzz-discovered guard: reject if the post-state drifts off-curve.
                if !csemm::invariant_holds(r.1, r.2, config.alpha, config.beta, INVARIANT_EPSILON) {
                    return Err(OrbswapError::InvariantViolation);
                }
                r
            }
        }
    } else {
        if dxhat < MIN_TRADE_NORMALIZED {
            return Err(OrbswapError::BelowMinTrade);
        }
        let (xhat, params) = normalized_all(config, reserves, s)?;
        let r = ndim::swap_out_n(&xhat[..n], &params[..n], i_in, i_out, dxhat)?;
        // Post-swap invariant over all n dimensions.
        let mut post = xhat;
        post[i_in] = r.1;
        post[i_out] = r.2;
        if !ndim::invariant_holds_n(&post[..n], &params[..n], INVARIANT_EPSILON) {
            return Err(OrbswapError::InvariantViolation);
        }
        r
    };

    // normalized → internal → native (round down, favors pool).
    let _ = (nx_in, nx_out); // used above only for the invariant guard
    let out_int = md(out_norm, s, WAD, Rounding::Down)?;
    let out_native = out_int / scale_out;
    if out_native > res_out_n {
        return Err(OrbswapError::InsufficientLiquidity);
    }

    // Curve reserves move by NET only (fees held outside → pool stays on-curve).
    let new_in_native = res_in_n + net_native;
    let new_out_native = res_out_n - out_native;
    Ok((
        out_native,
        new_in_native,
        new_out_native,
        lp_fee,
        protocol_fee,
    ))
}

/// The normalized reserve `x̂` at which **equal** reserves lie on the curve:
/// `α·(1 − n^{−1/u(α)})` for the superellipse, `1.0` for the circle. Verified:
/// at this value the n-dimensional invariant equals exactly 1.
fn balanced_xhat(config: &Config, n: usize) -> Result<i128, OrbswapError> {
    match config.mode {
        PoolMode::Circular => Ok(WAD),
        PoolMode::SuperElliptical => {
            let u = csemm::u(config.alpha)?;
            let inv_u = md(WAD, WAD, u, Rounding::Down)?; // 1/u (WAD)
            let n_wad = (n as i128).checked_mul(WAD).ok_or(OrbswapError::Overflow)?;
            // n^{−1/u}
            let pow = orbswap_math::fixed_point::pow_fixed(n_wad, -inv_u)
                .map_err(|_| OrbswapError::Overflow)?;
            let one_minus = WAD - pow;
            md(config.alpha, one_minus, WAD, Rounding::Down)
        }
    }
}

/// Build fixed-size arrays of every reserve normalized to x̂-space and the shared
/// shape param, for the `ndim` math (n ≤ MAX_TOKENS).
#[allow(clippy::type_complexity)]
fn normalized_all(
    config: &Config,
    reserves: &Vec<i128>,
    s: i128,
) -> Result<([i128; MAX_TOKENS], [i128; MAX_TOKENS]), OrbswapError> {
    let n = config.tokens.len() as usize;
    let mut xhat = [0i128; MAX_TOKENS];
    let mut params = [0i128; MAX_TOKENS];
    for i in 0..n {
        let ii = internal(
            reserves.get_unchecked(i as u32),
            config.scales.get_unchecked(i as u32),
        )?;
        xhat[i] = md(ii, WAD, s, Rounding::Down)?;
        params[i] = config.alpha; // symmetric n-token pool
    }
    Ok((xhat, params))
}

/// Exact-output swap math. Given the desired `amount_out_native`, compute the
/// gross input required (fee-inclusive), rounding **up** at every step so the
/// pool is favored (the user pays at least enough). Returns
/// `(amount_in_native, new_reserve_in_native, new_reserve_out_native, lp_fee,
/// protocol_fee)`. Fees are held OUTSIDE the curve (reserve moves by NET only).
fn compute_swap_exact_out(
    config: &Config,
    reserves: &Vec<i128>,
    s: i128,
    i_in: usize,
    i_out: usize,
    amount_out_native: i128,
    protocol_bps: i128,
) -> Result<(i128, i128, i128, i128, i128), OrbswapError> {
    let scale_in = config.scales.get_unchecked(i_in as u32);
    let scale_out = config.scales.get_unchecked(i_out as u32);
    let res_in_n = reserves.get_unchecked(i_in as u32);
    let res_out_n = reserves.get_unchecked(i_out as u32);
    if amount_out_native <= 0 || amount_out_native > res_out_n {
        return Err(OrbswapError::InsufficientLiquidity);
    }
    // Exact-out needs the curve inverse; the math lib has `swap_in` only for 2
    // tokens (no `swap_in_n` yet). N-token pools support exact-in only.
    if config.tokens.len() != 2 {
        return Err(OrbswapError::MathDomain);
    }

    // out native → internal → normalized (round UP → require more input, pool-favoring).
    let out_internal = internal(amount_out_native, scale_out)?;
    let res_in_int = internal(res_in_n, scale_in)?;
    let res_out_int = internal(res_out_n, scale_out)?;
    let xin = md(res_in_int, WAD, s, Rounding::Down)?;
    let xout = md(res_out_int, WAD, s, Rounding::Down)?;
    let out_norm = md(out_internal, WAD, s, Rounding::Up)?;
    if out_norm <= 0 {
        return Err(OrbswapError::BelowMinTrade);
    }

    // Required NET input in normalized space (curve inverse).
    let (in_norm, nx_in, nx_out) = match config.mode {
        PoolMode::Circular => ccmm::swap_in(xin, xout, TWO_PLUS_SQRT2, out_norm)?,
        PoolMode::SuperElliptical => {
            if out_norm < MIN_TRADE_NORMALIZED {
                return Err(OrbswapError::BelowMinTrade);
            }
            let r = csemm::swap_in(xin, xout, config.alpha, config.beta, out_norm)?;
            if !csemm::invariant_holds(r.1, r.2, config.alpha, config.beta, INVARIANT_EPSILON) {
                return Err(OrbswapError::InvariantViolation);
            }
            r
        }
    };
    let _ = (nx_in, nx_out);

    // net_norm → internal → native (round UP).
    let net_internal = md(in_norm, s, WAD, Rounding::Up)?;
    let net_native = ceil_div(net_internal, scale_in)?;
    // Invert the fee: smallest gross whose post-fee net ≥ net_native.
    let gross = gross_from_net(net_native, config.fee_bps)?;
    let fee_native = gross - net_native.min(gross);
    let (lp_fee, protocol_fee) =
        fees::split_protocol_fee(fee_native, protocol_bps).map_err(|_| OrbswapError::Overflow)?;

    // Curve moves by NET only; the whole fee (lp + protocol) is held outside.
    let new_in_native = res_in_n + net_native;
    let new_out_native = res_out_n - amount_out_native;
    Ok((gross, new_in_native, new_out_native, lp_fee, protocol_fee))
}

#[inline]
fn ceil_div(a: i128, b: i128) -> Result<i128, OrbswapError> {
    if b <= 0 {
        return Err(OrbswapError::Overflow);
    }
    a.checked_add(b - 1)
        .map(|x| x / b)
        .ok_or(OrbswapError::Overflow)
}

/// Net input after the fee is taken (`apply_fee(gross).0`).
#[inline]
fn net_after_fee(gross: i128, fee_bps: i128) -> Result<i128, OrbswapError> {
    let (net, _) = fees::apply_fee(gross, fee_bps).map_err(|_| OrbswapError::Overflow)?;
    Ok(net)
}

/// Smallest `gross` whose post-fee net is `≥ required_net` (fee rounds up). The
/// closed form can be off by ≤1 due to the ceil in `apply_fee`, so we correct.
fn gross_from_net(required_net: i128, fee_bps: i128) -> Result<i128, OrbswapError> {
    if fee_bps == 0 {
        return Ok(required_net);
    }
    let denom = 10_000 - fee_bps;
    // gross ≈ ceil(required_net · 10000 / (10000 − fee_bps)).
    let mut gross = md(required_net, 10_000, denom, Rounding::Up)?;
    // Correct upward until the net actually covers the requirement (≤2 steps).
    let mut guard = 0;
    while net_after_fee(gross, fee_bps)? < required_net {
        gross = gross.checked_add(1).ok_or(OrbswapError::Overflow)?;
        guard += 1;
        if guard > 4 {
            return Err(OrbswapError::Overflow);
        }
    }
    Ok(gross)
}

fn transfer_in(env: &Env, token: &Address, from: &Address, amount: i128) {
    let client = token::Client::new(env, token);
    client.transfer(from, env.current_contract_address(), &amount);
}

fn transfer_out(env: &Env, token: &Address, to: &Address, amount: i128) {
    let client = token::Client::new(env, token);
    client.transfer(&env.current_contract_address(), to, &amount);
}
