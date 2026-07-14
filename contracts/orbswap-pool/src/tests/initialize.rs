//! Tests for `initialize` (todo.md §2.3).

use super::Fixture;
use crate::types::{PoolMode, TWO_PLUS_SQRT2, WAD};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Vec};

#[test]
fn circular_init_succeeds() {
    let f = Fixture::new(7, 1_000_000_000);
    f.pool.initialize(
        &f.tokens(),
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &f.admin,
    );
    let cfg = f.pool.get_config();
    assert_eq!(cfg.tokens.len(), 2);
    assert_eq!(cfg.fee_bps, 30);
    // 7-decimal tokens → scale 10^11.
    assert_eq!(cfg.scales.get_unchecked(0), 100_000_000_000);
    assert_eq!(f.pool.get_liquidity_scale(), 0);
    assert_eq!(f.pool.total_shares(), 0);
}

#[test]
fn superelliptical_init_succeeds() {
    let f = Fixture::new(7, 1_000_000_000);
    f.pool.initialize(
        &f.tokens(),
        &PoolMode::SuperElliptical,
        &(3 * WAD),
        &(5 * WAD),
        &30,
        &f.admin,
    );
    let cfg = f.pool.get_config();
    assert_eq!(cfg.alpha, 3 * WAD);
    assert_eq!(cfg.beta, 5 * WAD);
}

#[test]
#[should_panic] // AlreadyInitialized
fn double_init_panics() {
    let f = Fixture::new(7, 1_000_000_000);
    let init = || {
        f.pool.initialize(
            &f.tokens(),
            &PoolMode::Circular,
            &TWO_PLUS_SQRT2,
            &TWO_PLUS_SQRT2,
            &30,
            &f.admin,
        )
    };
    init();
    init();
}

#[test]
#[should_panic] // InvalidConfig: Circular requires α=2+√2
fn circular_wrong_alpha_panics() {
    let f = Fixture::new(7, 1_000_000_000);
    f.pool.initialize(
        &f.tokens(),
        &PoolMode::Circular,
        &(3 * WAD),
        &(3 * WAD),
        &30,
        &f.admin,
    );
}

#[test]
#[should_panic] // InvalidConfig: fee > 100%
fn fee_too_high_panics() {
    let f = Fixture::new(7, 1_000_000_000);
    f.pool.initialize(
        &f.tokens(),
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &10_001,
        &f.admin,
    );
}

#[test]
#[should_panic] // InvalidConfig: duplicate token
fn duplicate_token_panics() {
    let f = Fixture::new(7, 1_000_000_000);
    let dup = Vec::from_array(&f.env, [f.token_a.clone(), f.token_a.clone()]);
    f.pool.initialize(
        &dup,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &f.admin,
    );
}

#[test]
#[should_panic] // InvalidConfig: needs exactly 2 tokens
fn wrong_token_count_panics() {
    let f = Fixture::new(7, 1_000_000_000);
    let one = Vec::from_array(&f.env, [f.token_a.clone()]);
    f.pool.initialize(
        &one,
        &PoolMode::Circular,
        &TWO_PLUS_SQRT2,
        &TWO_PLUS_SQRT2,
        &30,
        &f.admin,
    );
    let _ = Address::generate(&f.env);
}
