//! Tests for `deposit` (todo.md §2.4).

use super::Fixture;
use crate::types::MINIMUM_LIQUIDITY;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::Vec;

#[test]
fn first_deposit_mints_shares_and_locks_minimum() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(30);
    // 100 tokens each (7-dec → 1e9 native) → internal 1e20 = s.
    let amount = 1_000_000_000i128;
    let minted = f.deposit_balanced(amount);

    let internal = amount * 100_000_000_000; // ·10^11
    assert_eq!(
        f.pool.get_liquidity_scale(),
        internal,
        "s = balanced internal"
    );
    assert_eq!(f.pool.total_shares(), internal);
    // User gets everything except the locked minimum.
    assert_eq!(minted, internal - MINIMUM_LIQUIDITY);
    assert_eq!(f.pool.shares_of(&f.lp), minted);
    // Tokens moved into the pool.
    assert_eq!(f.balance(&f.token_a, &f.pool.address), amount);
    assert_eq!(f.balance(&f.token_b, &f.pool.address), amount);
}

#[test]
#[should_panic] // ImbalancedDeposit: first deposit must be balanced
fn first_deposit_imbalanced_rejected() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(30);
    let amounts = Vec::from_array(&f.env, [1_000_000_000i128, 500_000_000i128]);
    f.pool.deposit(&f.lp, &amounts, &0, &u64::MAX);
}

#[test]
fn proportional_second_deposit() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(30);
    f.deposit_balanced(1_000_000_000);
    let s0 = f.pool.get_liquidity_scale();

    // Add another balanced (= proportional) deposit of half the size.
    let minted = f.deposit_balanced(500_000_000);
    // Δs = s0 · 0.5.
    assert_eq!(minted, s0 / 2, "proportional shares ∝ Δs");
    assert_eq!(f.pool.get_liquidity_scale(), s0 + s0 / 2);
    assert_eq!(f.balance(&f.token_a, &f.pool.address), 1_500_000_000);
}

#[test]
#[should_panic] // ImbalancedDeposit
fn imbalanced_second_deposit_rejected() {
    let f = Fixture::new(7, 3_000_000_000);
    f.init_circular(30);
    f.deposit_balanced(1_000_000_000);
    let amounts = Vec::from_array(&f.env, [1_000_000_000i128, 300_000_000i128]);
    f.pool.deposit(&f.lp, &amounts, &0, &u64::MAX);
}

#[test]
#[should_panic] // SlippageExceeded: min_shares too high
fn min_shares_slippage() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(30);
    let amounts = Vec::from_array(&f.env, [1_000_000_000i128, 1_000_000_000i128]);
    f.pool.deposit(&f.lp, &amounts, &i128::MAX, &u64::MAX);
}

#[test]
#[should_panic] // Expired
fn deadline_expired() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(30);
    let amounts = Vec::from_array(&f.env, [1_000_000_000i128, 1_000_000_000i128]);
    // ledger timestamp defaults to 0; advance it past the deadline.
    f.env.ledger().with_mut(|l| l.timestamp = 100);
    f.pool.deposit(&f.lp, &amounts, &0, &50);
}
