//! Tests for pausability + events (todo.md §2.9, §2.10).

use super::Fixture;
use soroban_sdk::Vec;

fn setup() -> Fixture {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    f
}

#[test]
fn pause_swaps_blocks_only_swaps() {
    let f = setup();
    f.pool.pause_swaps(&true);
    assert!(f.pool.paused().swaps);

    // Swap is blocked.
    let r = f
        .pool
        .try_swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(r.is_err(), "swap should be paused");

    // Deposits and withdrawals still work.
    f.deposit_balanced(100_000_000);
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    f.pool.withdraw(&f.lp, &1_000, &mins, &u64::MAX);

    // Unpause restores swaps.
    f.pool.pause_swaps(&false);
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(out > 0);
}

#[test]
fn pause_deposits_blocks_only_deposits() {
    let f = setup();
    f.pool.pause_deposits(&true);
    let amounts = Vec::from_array(&f.env, [100_000_000i128, 100_000_000i128]);
    let r = f.pool.try_deposit(&f.lp, &amounts, &0, &u64::MAX);
    assert!(r.is_err(), "deposit should be paused");
    // Swap still works.
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(out > 0);
}

#[test]
fn pause_all_blocks_everything() {
    let f = setup();
    f.pool.pause_all(&true);
    let p = f.pool.paused();
    assert!(p.deposits && p.swaps && p.withdrawals);

    let r = f
        .pool
        .try_swap(&f.lp, &f.token_a, &10_000_000, &f.token_b, &0, &u64::MAX);
    assert!(r.is_err());
    let mins = Vec::from_array(&f.env, [0i128, 0i128]);
    let rw = f.pool.try_withdraw(&f.lp, &1_000, &mins, &u64::MAX);
    assert!(rw.is_err());

    f.pool.pause_all(&false);
    assert!(!f.pool.paused().swaps);
}
