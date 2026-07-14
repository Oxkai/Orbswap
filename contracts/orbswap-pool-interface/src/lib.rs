#![no_std]
//! Shared pool types and a `#[contractclient]` so the factory/router can call a
//! pool **without** depending on the pool contract crate (which would pull its
//! exported wasm symbols into their binaries and collide at link time).
//!
//! The types here are the single source of truth: `orbswap-pool` re-exports them,
//! so the client and the contract always agree on the wire format.

use soroban_sdk::{contractclient, contracttype, Address, Env, Vec};

/// Internal fixed-point scale (WAD, 1e18).
pub const WAD: i128 = 1_000_000_000_000_000_000;
/// `2 + √2` in WAD — the circle's shape parameter (`u = 2`).
pub const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049;
/// Shares permanently locked on the first deposit (inflation-attack guard).
pub const MINIMUM_LIQUIDITY: i128 = 1_000;

/// Which concentration mechanism the pool uses.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolMode {
    Circular,
    SuperElliptical,
}

/// Immutable pool configuration.
#[contracttype]
#[derive(Clone)]
pub struct Config {
    pub tokens: Vec<Address>,
    pub mode: PoolMode,
    pub alpha: i128,
    pub beta: i128,
    pub scales: Vec<i128>,
    pub fee_bps: i128,
    pub admin: Address,
}

/// Generates `PoolClient` for cross-contract calls (no implementation, no exports).
#[contractclient(name = "PoolClient")]
pub trait PoolInterface {
    #[allow(clippy::too_many_arguments)]
    fn initialize(
        env: Env,
        tokens: Vec<Address>,
        mode: PoolMode,
        alpha: i128,
        beta: i128,
        fee_bps: i128,
        admin: Address,
    );
    fn deposit(
        env: Env,
        from: Address,
        amounts: Vec<i128>,
        min_shares: i128,
        deadline: u64,
    ) -> i128;
    fn withdraw(
        env: Env,
        from: Address,
        shares: i128,
        min_amounts: Vec<i128>,
        deadline: u64,
    ) -> Vec<i128>;
    #[allow(clippy::too_many_arguments)]
    fn swap(
        env: Env,
        from: Address,
        token_in: Address,
        amount_in: i128,
        token_out: Address,
        min_out: i128,
        deadline: u64,
    ) -> i128;
    #[allow(clippy::too_many_arguments)]
    fn swap_exact_out(
        env: Env,
        from: Address,
        token_in: Address,
        token_out: Address,
        amount_out: i128,
        max_in: i128,
        deadline: u64,
    ) -> i128;
    fn lp_fees_owed(env: Env) -> Vec<i128>;
    fn quote(env: Env, token_in: Address, amount_in: i128, token_out: Address) -> i128;
    fn quote_exact_out(env: Env, token_in: Address, token_out: Address, amount_out: i128) -> i128;
    fn get_config(env: Env) -> Config;
}
