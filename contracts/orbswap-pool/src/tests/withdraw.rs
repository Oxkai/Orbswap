//! Tests for `withdraw` (todo.md §2.5).

use super::Fixture;
use soroban_sdk::Vec;

#[test]
fn withdraw_returns_proportional_reserves() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let minted = f.deposit_balanced(1_000_000_000);

    // Withdraw half the LP's shares.
    let half = minted / 2;
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let out = f.pool.withdraw(&f.lp, &half, &mins, &u64::MAX);

    // Roughly half of each reserve (minus the locked-minimum dilution).
    assert!(out.get_unchecked(0) > 490_000_000 && out.get_unchecked(0) <= 500_000_000);
    assert!(out.get_unchecked(1) > 490_000_000 && out.get_unchecked(1) <= 500_000_000);
    assert_eq!(f.pool.shares_of(&f.lp), minted - half);
    // Reserves + s decreased.
    assert!(f.pool.get_liquidity_scale() < 100_000_000_000_000_000_000);
}

#[test]
fn full_withdraw_leaves_locked_minimum() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let minted = f.deposit_balanced(1_000_000_000);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &minted, &mins, &u64::MAX);

    assert_eq!(f.pool.shares_of(&f.lp), 0);
    // total_shares never reaches 0 — the MINIMUM_LIQUIDITY stays locked.
    assert!(f.pool.total_shares() >= crate::types::MINIMUM_LIQUIDITY);
    assert!(f.pool.get_liquidity_scale() >= crate::types::MINIMUM_LIQUIDITY);
}

#[test]
fn deposit_then_withdraw_no_profit() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let a_before = f.balance(&f.token_a, &f.lp);
    let b_before = f.balance(&f.token_b, &f.lp);

    let minted = f.deposit_balanced(1_000_000_000);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &minted, &mins, &u64::MAX);

    // LP can never get back more than deposited (locked minimum stays behind).
    assert!(f.balance(&f.token_a, &f.lp) <= a_before);
    assert!(f.balance(&f.token_b, &f.lp) <= b_before);
}

#[test]
#[should_panic] // InsufficientLiquidity: more than owned
fn over_withdraw_rejected() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let minted = f.deposit_balanced(1_000_000_000);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &(minted + 1), &mins, &u64::MAX);
}

#[test]
#[should_panic] // SlippageExceeded: min_amounts too high
fn min_amounts_slippage() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let minted = f.deposit_balanced(1_000_000_000);
    let mins = Vec::from_array(&f.env, [i128::MAX, 0i128]);
    f.pool.withdraw(&f.lp, &(minted / 2), &mins, &u64::MAX);
}
