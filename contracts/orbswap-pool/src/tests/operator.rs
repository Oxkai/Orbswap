//! Phase 5 — operator mode (todo.md §Phase 5).
//!
//! Operator mode is what makes the pool an anchor's settlement rail rather than a
//! public venue: liquidity comes from an allowlist, but **anyone may trade**, and
//! **anyone may exit**.

use super::{rate_pool, Fixture, RATE_FEE_BPS, RATE_MINT};
use crate::types::TWO_PLUS_SQRT2;
use crate::OrbswapError;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Vec};

fn parity_pool() -> Fixture {
    let f = Fixture::new(7, RATE_MINT);
    f.init_superelliptical(TWO_PLUS_SQRT2, TWO_PLUS_SQRT2, RATE_FEE_BPS);
    f
}

#[test]
fn mode_is_off_by_default_and_deposits_are_permissionless() {
    let f = parity_pool();
    let (mode, _) = f.pool.operator_status(&f.lp);
    assert!(!mode);
    assert!(f.deposit_balanced(1_000_000_000) > 0);
}

#[test]
fn non_operator_deposit_is_rejected() {
    let f = parity_pool();
    f.pool.set_operator_mode(&true);
    let amounts = Vec::from_array(&f.env, [1_000_000_000i128, 1_000_000_000i128]);
    assert_eq!(
        f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX),
        Err(Ok(OrbswapError::NotOperator))
    );
}

#[test]
fn allowlisted_operator_can_deposit() {
    let f = parity_pool();
    f.pool.set_operator_mode(&true);
    f.pool.set_operator(&f.lp, &true);
    let (mode, allowed) = f.pool.operator_status(&f.lp);
    assert!(mode && allowed);
    assert!(f.deposit_balanced(1_000_000_000) > 0);
}

#[test]
fn revoked_operator_can_still_withdraw() {
    let f = parity_pool();
    f.pool.set_operator_mode(&true);
    f.pool.set_operator(&f.lp, &true);
    f.deposit_balanced(1_000_000_000);

    f.pool.set_operator(&f.lp, &false);
    let shares = f.pool.shares_of(&f.lp);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let got = f.pool.withdraw(&f.lp, &(shares / 2), &mins, &u64::MAX);
    assert!(
        got.get_unchecked(0) > 0,
        "revoking LP rights must never trap an LP's capital"
    );
}

#[test]
fn swapping_stays_open_to_everyone_in_operator_mode() {
    let (f, _) = rate_pool();
    f.pool.set_operator_mode(&true);
    // `lp` is deliberately NOT allowlisted.
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(
        out > 0,
        "the whole point is that anyone can trade against the operator's inventory"
    );
}

#[test]
fn a_stranger_can_trade_but_not_provide() {
    let (f, _) = rate_pool();
    f.pool.set_operator_mode(&true);
    let stranger = Address::generate(&f.env);
    let amounts = Vec::from_array(&f.env, [10_000_000i128, 10_000_000_000i128]);
    assert_eq!(
        f.pool.try_deposit(&stranger, &amounts, &0, &u64::MAX),
        Err(Ok(OrbswapError::NotOperator))
    );
}

#[test]
fn disabling_the_mode_restores_open_provision() {
    let f = parity_pool();
    f.pool.set_operator_mode(&true);
    let amounts = Vec::from_array(&f.env, [1_000_000_000i128, 1_000_000_000i128]);
    assert!(f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX).is_err());
    f.pool.set_operator_mode(&false);
    assert!(f.deposit_balanced(1_000_000_000) > 0);
}
