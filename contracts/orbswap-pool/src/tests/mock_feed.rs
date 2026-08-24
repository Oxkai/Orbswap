//! A minimal SEP-40 `PriceFeedTrait` implementation for tests (todo.md §Phase 1).
//!
//! Supports everything the rate guards need to be exercised: per-asset prices, a
//! settable timestamp, configurable decimals, and a mode where `lastprice` returns
//! `None` to simulate an unavailable feed.

use crate::rates::{Asset, PriceData};
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Map, Symbol};

#[contracttype]
pub enum FeedKey {
    Decimals,
    Base,
    Prices,
    Timestamp,
    Down,
}

#[contract]
pub struct MockFeed;

#[contractimpl]
impl MockFeed {
    pub fn init(env: Env, decimals: u32, base: Address) {
        env.storage().instance().set(&FeedKey::Decimals, &decimals);
        env.storage().instance().set(&FeedKey::Base, &base);
        env.storage()
            .instance()
            .set(&FeedKey::Prices, &Map::<Address, i128>::new(&env));
        env.storage().instance().set(&FeedKey::Timestamp, &0u64);
        env.storage().instance().set(&FeedKey::Down, &false);
    }

    /// Set the price of `asset` in the feed's own decimals.
    pub fn set_price(env: Env, asset: Address, price: i128) {
        let mut m: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&FeedKey::Prices)
            .unwrap_or_else(|| Map::new(&env));
        m.set(asset, price);
        env.storage().instance().set(&FeedKey::Prices, &m);
    }

    /// Set the timestamp reported on every sample.
    pub fn set_timestamp(env: Env, ts: u64) {
        env.storage().instance().set(&FeedKey::Timestamp, &ts);
    }

    /// When `down`, `lastprice` returns `None` for every asset.
    pub fn set_down(env: Env, down: bool) {
        env.storage().instance().set(&FeedKey::Down, &down);
    }

    // ── SEP-40 surface ──────────────────────────────────────────────────────

    pub fn base(env: Env) -> Asset {
        Asset::Stellar(env.storage().instance().get(&FeedKey::Base).unwrap())
    }

    pub fn decimals(env: Env) -> u32 {
        env.storage().instance().get(&FeedKey::Decimals).unwrap()
    }

    pub fn resolution(_env: Env) -> u32 {
        300
    }

    pub fn lastprice(env: Env, asset: Asset) -> Option<PriceData> {
        let down: bool = env
            .storage()
            .instance()
            .get(&FeedKey::Down)
            .unwrap_or(false);
        if down {
            return None;
        }
        let addr = match asset {
            Asset::Stellar(a) => a,
            Asset::Other(_) => return None,
        };
        let m: Map<Address, i128> = env
            .storage()
            .instance()
            .get(&FeedKey::Prices)
            .unwrap_or_else(|| Map::new(&env));
        let price = m.get(addr)?;
        let timestamp: u64 = env
            .storage()
            .instance()
            .get(&FeedKey::Timestamp)
            .unwrap_or(0);
        Some(PriceData { price, timestamp })
    }

    /// Unused by the pool, present so the mock is a faithful SEP-40 feed.
    pub fn price(env: Env, asset: Asset, _timestamp: u64) -> Option<PriceData> {
        Self::lastprice(env, asset)
    }

    pub fn other(_env: Env, _s: Symbol) -> u32 {
        0
    }
}
