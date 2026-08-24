#![no_std]
//! A minimal, operator-controlled **SEP-40 price feed** for testnet.
//!
//! # Why this exists
//! Orbswap's rate-aware pools consume any SEP-40 `PriceFeedTrait` contract, so in
//! production they point straight at **Reflector** (major pairs) or **Lightecho**
//! (NGN, ARS, BRL, KES, INR) by address — no code change, because the pool is
//! feed-agnostic by construction.
//!
//! But a testnet demo runs on SAC test tokens that no real oracle quotes. This
//! contract fills that gap: a faithful SEP-40 surface whose prices the deployer
//! sets, so a rate-aware pool can be exercised end-to-end before a real feed is
//! wired up.
//!
//! **Never deploy this to mainnet.** It is a stand-in with a trusted writer, and
//! a pool pointed at it inherits exactly one trust assumption: the writer.

use soroban_sdk::{
    contract, contractimpl, contracttype, panic_with_error, Address, Env, Map, Symbol, Vec,
};

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
enum Key {
    Admin,
    Decimals,
    Base,
    Prices,
}

#[soroban_sdk::contracterror]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum FeedError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    InvalidPrice = 3,
}

#[contract]
pub struct FeedStub;

#[contractimpl]
impl FeedStub {
    /// `base` is the asset every price is denominated in. Set it to the pool's
    /// numeraire (e.g. USDC) to quote directly; set it to XLM to exercise the
    /// pool's cross-rate path, the way Lightecho behaves.
    pub fn initialize(env: Env, admin: Address, decimals: u32, base: Address) {
        if env.storage().instance().has(&Key::Admin) {
            panic_with_error!(&env, FeedError::AlreadyInitialized);
        }
        env.storage().instance().set(&Key::Admin, &admin);
        env.storage().instance().set(&Key::Decimals, &decimals);
        env.storage().instance().set(&Key::Base, &base);
        env.storage()
            .instance()
            .set(&Key::Prices, &Map::<Address, PriceData>::new(&env));
    }

    /// Publish a price for `asset`, in this feed's decimals, stamped with the
    /// current ledger time.
    pub fn set_price(env: Env, asset: Address, price: i128) {
        Self::admin(&env).require_auth();
        if price <= 0 {
            panic_with_error!(&env, FeedError::InvalidPrice);
        }
        let mut m = Self::prices(&env);
        m.set(
            asset,
            PriceData {
                price,
                timestamp: env.ledger().timestamp(),
            },
        );
        env.storage().instance().set(&Key::Prices, &m);
    }

    // ── SEP-40 surface ──────────────────────────────────────────────────────

    pub fn base(env: Env) -> Asset {
        Asset::Stellar(
            env.storage()
                .instance()
                .get(&Key::Base)
                .unwrap_or_else(|| panic_with_error!(&env, FeedError::NotInitialized)),
        )
    }

    pub fn decimals(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&Key::Decimals)
            .unwrap_or_else(|| panic_with_error!(&env, FeedError::NotInitialized))
    }

    pub fn resolution(_env: Env) -> u32 {
        300
    }

    pub fn assets(env: Env) -> Vec<Asset> {
        let mut out = Vec::new(&env);
        for (addr, _) in Self::prices(&env).iter() {
            out.push_back(Asset::Stellar(addr));
        }
        out
    }

    pub fn lastprice(env: Env, asset: Asset) -> Option<PriceData> {
        match asset {
            Asset::Stellar(a) => Self::prices(&env).get(a),
            Asset::Other(_) => None,
        }
    }

    pub fn price(env: Env, asset: Asset, _timestamp: u64) -> Option<PriceData> {
        Self::lastprice(env, asset)
    }

    // ── internals ───────────────────────────────────────────────────────────

    fn admin(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&Key::Admin)
            .unwrap_or_else(|| panic_with_error!(env, FeedError::NotInitialized))
    }

    fn prices(env: &Env) -> Map<Address, PriceData> {
        env.storage()
            .instance()
            .get(&Key::Prices)
            .unwrap_or_else(|| Map::new(env))
    }
}

#[cfg(test)]
mod test;
