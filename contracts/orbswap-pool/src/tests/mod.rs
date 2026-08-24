//! Test fixture + module declarations.

use crate::types::{PoolMode, TWO_PLUS_SQRT2, WAD};
use crate::{OrbswapPool, OrbswapPoolClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Address, Env, Vec};

mod adversarial;
mod deposit;
mod exact_out;
mod features;
mod initialize;
pub mod mock_feed;
mod ndim;
mod operator;
mod oracle;
mod pause;
mod rates_mapping;
mod rates_oracle;
mod rates_reanchor;
mod rates_safety;
mod reentrancy;
mod swap;
mod tick_liquidity;
mod withdraw;

/// A registered pool + two mock SEP-41 tokens, with balances minted to an LP.
pub struct Fixture {
    pub env: Env,
    pub pool: OrbswapPoolClient<'static>,
    pub token_a: Address,
    pub token_b: Address,
    pub admin: Address,
    pub lp: Address,
}

impl Fixture {
    /// Create with both tokens at `decimals` and `mint` units minted to `lp`.
    pub fn new(decimals: u32, mint: i128) -> Fixture {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let lp = Address::generate(&env);

        let (token_a, admin_a) = create_token(&env, &admin, decimals);
        let (token_b, admin_b) = create_token(&env, &admin, decimals);
        admin_a.mint(&lp, &mint);
        admin_b.mint(&lp, &mint);

        let pool_id = env.register(OrbswapPool, ());
        let pool = OrbswapPoolClient::new(&env, &pool_id);

        Fixture {
            env,
            pool,
            token_a,
            token_b,
            admin,
            lp,
        }
    }

    pub fn tokens(&self) -> Vec<Address> {
        Vec::from_array(&self.env, [self.token_a.clone(), self.token_b.clone()])
    }

    /// Initialize as a `Circular` pool with the given fee.
    pub fn init_circular(&self, fee_bps: i128) {
        self.pool.initialize(
            &self.tokens(),
            &PoolMode::Circular,
            &TWO_PLUS_SQRT2,
            &TWO_PLUS_SQRT2,
            &fee_bps,
            &self.admin,
        );
    }

    /// Initialize as a `SuperElliptical` pool (α, β in WAD).
    pub fn init_superelliptical(&self, alpha: i128, beta: i128, fee_bps: i128) {
        self.pool.initialize(
            &self.tokens(),
            &PoolMode::SuperElliptical,
            &alpha,
            &beta,
            &fee_bps,
            &self.admin,
        );
    }

    /// Deposit `amount` of each token (balanced) from the LP.
    pub fn deposit_balanced(&self, amount: i128) -> i128 {
        let amounts = Vec::from_array(&self.env, [amount, amount]);
        self.pool.deposit(&self.lp, &amounts, &0, &u64::MAX)
    }

    pub fn balance(&self, token: &Address, who: &Address) -> i128 {
        token::Client::new(&self.env, token).balance(who)
    }
}

/// WAD re-export for tests.
pub const _WAD: i128 = WAD;

/// Register a mock SEP-41 token with the given admin and decimals.
fn create_token<'a>(
    env: &Env,
    admin: &Address,
    decimals: u32,
) -> (Address, token::StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    let addr = sac.address();
    let admin_client = token::StellarAssetClient::new(env, &addr);
    // StellarAsset tokens are 7-decimal; `decimals` is accepted for API symmetry
    // and asserted by tests that need it.
    let _ = decimals;
    (addr, admin_client)
}

// ─── Rate-aware fixtures (shared by the Phase 1–5 suites) ────────────────────

/// 14-decimal feed, matching Reflector's reported precision.
pub const FEED_DEC: u32 = 14;
/// `1.0` at [`FEED_DEC`].
pub const ONE_FEED: i128 = 100_000_000_000_000;
pub const RATE_FEE_BPS: i128 = 30;
pub const RATE_MINT: i128 = 1_000_000_000_000_000;

/// A seeded rate-aware `SuperElliptical` pool at `1 token_b = 0.001 token_a`,
/// plus a handle on its feed. `token_b` is the quote leg, `token_a` the numeraire.
pub fn rate_pool() -> (Fixture, mock_feed::MockFeedClient<'static>) {
    let f = Fixture::new(7, RATE_MINT);
    f.init_superelliptical(TWO_PLUS_SQRT2, TWO_PLUS_SQRT2, RATE_FEE_BPS);
    let feed_id = f.env.register(mock_feed::MockFeed, ());
    let feed = mock_feed::MockFeedClient::new(&f.env, &feed_id);
    feed.init(&FEED_DEC, &f.token_a);
    feed.set_price(&f.token_b, &(ONE_FEED / 1_000));
    feed.set_price(&f.token_a, &ONE_FEED);
    feed.set_timestamp(&f.env.ledger().timestamp());
    f.pool
        .configure_rates(&feed_id, &1, &0, &false, &3600, &500);
    let amounts = Vec::from_array(&f.env, [10_000_000_000i128, 10_000_000_000_000i128]);
    f.pool.deposit(&f.lp, &amounts, &0, &u64::MAX);
    (f, feed)
}

/// Move the quote price to `num/den` of its original value and accept it.
pub fn move_rate(f: &Fixture, feed: &mock_feed::MockFeedClient<'static>, num: i128, den: i128) {
    feed.set_price(&f.token_b, &(ONE_FEED / 1_000 * num / den));
    f.pool.poke_rate();
}
