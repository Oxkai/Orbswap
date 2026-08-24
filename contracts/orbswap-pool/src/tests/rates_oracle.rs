//! Phase 1 — SEP-40 client and rate guards (todo.md §Phase 1).
//!
//! Covers decimal normalization, cross-rate derivation, staleness, deviation, and
//! the no-fallback rule: an unavailable feed must error, never substitute a value.

use super::mock_feed::{MockFeed, MockFeedClient};
use crate::rates::{deviation_bps, to_wad};
use crate::types::WAD;
use crate::OrbswapError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

/// A mock feed with `decimals`, plus the two token addresses it prices.
struct FeedFixture {
    env: Env,
    feed: MockFeedClient<'static>,
    feed_id: Address,
    quote: Address,
    numeraire: Address,
}

fn feed_fixture(decimals: u32) -> FeedFixture {
    let env = Env::default();
    env.mock_all_auths();
    let quote = Address::generate(&env);
    let numeraire = Address::generate(&env);
    let feed_id = env.register(MockFeed, ());
    let feed = MockFeedClient::new(&env, &feed_id);
    feed.init(&decimals, &numeraire);
    FeedFixture {
        env,
        feed,
        feed_id,
        quote,
        numeraire,
    }
}

// ─── decimal normalization ───────────────────────────────────────────────────

#[test]
fn to_wad_lifts_14_decimal_feed() {
    // Reflector-style 14-decimal feed reporting 0.001.
    let raw = 100_000_000_000i128; // 0.001 * 1e14
    assert_eq!(to_wad(raw, 14).unwrap(), WAD / 1_000);
}

#[test]
fn to_wad_passes_through_18_decimals() {
    assert_eq!(to_wad(WAD, 18).unwrap(), WAD);
}

#[test]
fn to_wad_rejects_non_positive() {
    assert_eq!(to_wad(0, 14), Err(OrbswapError::OracleUnavailable));
    assert_eq!(to_wad(-1, 14), Err(OrbswapError::OracleUnavailable));
}

#[test]
fn to_wad_rejects_decimals_above_18() {
    assert_eq!(to_wad(1, 19), Err(OrbswapError::InvalidRateConfig));
}

// ─── deviation ───────────────────────────────────────────────────────────────

#[test]
fn deviation_zero_for_unchanged_rate() {
    assert_eq!(deviation_bps(WAD, WAD).unwrap(), 0);
}

#[test]
fn deviation_one_percent_is_100_bps() {
    let old = WAD;
    let new = WAD + WAD / 100;
    assert_eq!(deviation_bps(old, new).unwrap(), 100);
}

#[test]
fn deviation_is_absolute_not_signed() {
    let up = deviation_bps(WAD, WAD + WAD / 100).unwrap();
    let down = deviation_bps(WAD, WAD - WAD / 100).unwrap();
    assert_eq!(up, down, "a move down must count the same as a move up");
}

#[test]
fn deviation_rounds_up_so_boundary_trips() {
    // A move just over 100 bps must report >100, never exactly 100.
    let old = WAD;
    let new = WAD + WAD / 100 + 1;
    assert!(deviation_bps(old, new).unwrap() > 100);
}

#[test]
fn deviation_catches_the_yieldblox_shape() {
    // The Feb 2026 Blend V2 incident: ~100x price move inside one pricing window.
    let old = WAD;
    let new = WAD * 100;
    assert_eq!(deviation_bps(old, new).unwrap(), 990_000);
}

#[test]
fn deviation_rejects_non_positive_base() {
    assert_eq!(deviation_bps(0, WAD), Err(OrbswapError::OracleUnavailable));
}

#[test]
fn deviation_overflow_is_error_not_panic() {
    // Huge delta against a tiny base must surface Overflow, never unwind.
    assert!(deviation_bps(1, i128::MAX).is_err());
}

// ─── mock feed behaves like a SEP-40 feed ────────────────────────────────────

#[test]
fn mock_feed_reports_configured_decimals() {
    let f = feed_fixture(14);
    assert_eq!(f.feed.decimals(), 14);
}

#[test]
fn mock_feed_returns_set_price() {
    let f = feed_fixture(14);
    f.feed.set_price(&f.quote, &123);
    f.feed.set_timestamp(&999);
    let p = f
        .feed
        .lastprice(&crate::rates::Asset::Stellar(f.quote.clone()))
        .expect("price present");
    assert_eq!(p.price, 123);
    assert_eq!(p.timestamp, 999);
}

#[test]
fn mock_feed_unknown_asset_is_none() {
    let f = feed_fixture(14);
    let unknown = Address::generate(&f.env);
    assert!(f
        .feed
        .lastprice(&crate::rates::Asset::Stellar(unknown))
        .is_none());
}

#[test]
fn mock_feed_down_returns_none_for_known_asset() {
    let f = feed_fixture(14);
    f.feed.set_price(&f.quote, &123);
    f.feed.set_down(&true);
    assert!(
        f.feed
            .lastprice(&crate::rates::Asset::Stellar(f.quote.clone()))
            .is_none(),
        "a down feed must yield None so the pool closes rather than quoting stale"
    );
    let _ = (&f.feed_id, &f.numeraire);
}
