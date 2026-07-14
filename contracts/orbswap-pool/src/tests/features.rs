//! Tests for protocol fee, depeg auto-eject, and LP-share transfer.

use super::Fixture;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Vec};

// ---------------------------------------------------------------- protocol fee

#[test]
fn protocol_fee_accrues_and_collects() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(100); // 1% swap fee
    f.deposit_balanced(1_000_000_000);
    f.pool.set_protocol_fee_bps(&5_000); // protocol takes 50% of the fee

    let amount_in = 100_000_000i128;
    f.pool
        .swap(&f.lp, &f.token_a, &amount_in, &f.token_b, &0, &u64::MAX);

    // fee = 1% of 100M = 1_000_000; protocol = 50% = 500_000, LP cut = 500_000
    // (both in token A, held outside the curve).
    let owed = f.pool.protocol_owed();
    assert_eq!(owed.get_unchecked(0), 500_000, "protocol owed token A");
    assert_eq!(owed.get_unchecked(1), 0);
    let lp_owed = f.pool.lp_fees_owed();
    assert_eq!(lp_owed.get_unchecked(0), 500_000, "LP fee owed token A");

    // Solvency: pool balance == stored reserve + protocol owed + LP fees owed.
    let reserves = f.pool.get_reserves();
    assert_eq!(
        f.balance(&f.token_a, &f.pool.address),
        reserves.get_unchecked(0) + owed.get_unchecked(0) + lp_owed.get_unchecked(0),
        "balance == reserve + protocol owed + lp owed"
    );

    // Collect to a fresh recipient.
    let treasury = Address::generate(&f.env);
    let collected = f.pool.collect_protocol_fees(&treasury);
    assert_eq!(collected.get_unchecked(0), 500_000);
    assert_eq!(f.balance(&f.token_a, &treasury), 500_000);
    // Owed zeroed; balance now equals reserve + the LP fee pot (still outside curve).
    assert_eq!(f.pool.protocol_owed().get_unchecked(0), 0);
    assert_eq!(
        f.balance(&f.token_a, &f.pool.address),
        f.pool.get_reserves().get_unchecked(0) + 500_000
    );
}

#[test]
fn zero_protocol_fee_keeps_all_for_lps() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(100);
    f.deposit_balanced(1_000_000_000);
    // Default protocol bps = 0.
    f.pool
        .swap(&f.lp, &f.token_a, &100_000_000, &f.token_b, &0, &u64::MAX);
    assert_eq!(f.pool.protocol_owed().get_unchecked(0), 0);
    // Curve reserve holds only the NET (99M added); the full 1M fee goes to the LP
    // pot outside the curve. Together they equal the full 100M input.
    assert_eq!(f.pool.get_reserves().get_unchecked(0), 1_099_000_000);
    assert_eq!(f.pool.lp_fees_owed().get_unchecked(0), 1_000_000);
    assert_eq!(
        f.balance(&f.token_a, &f.pool.address),
        1_100_000_000,
        "reserve + LP pot == full input"
    );
}

#[test]
#[should_panic] // InvalidConfig: protocol bps > 100%
fn protocol_fee_bps_capped() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(100);
    f.pool.set_protocol_fee_bps(&10_001);
}

// ---------------------------------------------------------------- depeg eject

#[test]
fn disallowed_token_blocks_swap_in_and_deposit_only() {
    let f = Fixture::new(7, 5_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);

    // Eject token A.
    f.pool.set_allowed(&f.token_a, &false);
    assert!(!f.pool.is_allowed(&f.token_a));
    assert!(f.pool.is_allowed(&f.token_b));

    // Swapping A IN is blocked...
    let r = f
        .pool
        .try_swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(r.is_err(), "swap-in of ejected token must revert");

    // ...but swapping A OUT (B in) stays open — arbitrage drains the depegged coin.
    let out = f
        .pool
        .swap(&f.lp, &f.token_b, &10_000_000, &f.token_a, &0, &u64::MAX);
    assert!(out > 0, "swap-out of ejected token should work");

    // Deposits are frozen while any token is ejected.
    let amounts = Vec::from_array(&f.env, [10_000_000i128, 10_000_000i128]);
    let rd = f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX);
    assert!(rd.is_err(), "deposit must be frozen during eject");

    // Withdrawals stay open (LPs can exit).
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &1_000, &mins, &u64::MAX);

    // Re-allow restores normal operation.
    f.pool.set_allowed(&f.token_a, &true);
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(out > 0);
}

// ---------------------------------------------------------------- LP transfer

#[test]
fn lp_shares_transfer() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let minted = f.deposit_balanced(1_000_000_000);
    let bob = Address::generate(&f.env);

    let half = minted / 2;
    f.pool.transfer_shares(&f.lp, &bob, &half);
    assert_eq!(f.pool.shares_of(&f.lp), minted - half);
    assert_eq!(f.pool.shares_of(&bob), half);

    // Bob can withdraw with his shares.
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let out = f.pool.withdraw(&bob, &half, &mins, &u64::MAX);
    assert!(out.get_unchecked(0) > 0);
    assert_eq!(f.pool.shares_of(&bob), 0);
}

#[test]
#[should_panic] // InsufficientLiquidity: transfer more than owned
fn lp_transfer_over_balance() {
    let f = Fixture::new(7, 1_000_000_000);
    f.init_circular(0);
    let minted = f.deposit_balanced(1_000_000_000);
    let bob = Address::generate(&f.env);
    f.pool.transfer_shares(&f.lp, &bob, &(minted + 1));
}
