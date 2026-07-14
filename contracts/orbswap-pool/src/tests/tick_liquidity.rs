//! Concentrated-liquidity (tick mode) deposit/withdraw — M3. Verifies position
//! add/remove conservation and solvency, and that the fungible-share path is
//! disabled once ticks are on. Swap-side (tick walking) is M5.

use super::Fixture;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Vec};

const M: i128 = 10_000_000; // 1 token (7-dec)

fn amounts(f: &Fixture, x: i128, y: i128) -> Vec<i128> {
    Vec::from_array(&f.env, [x, y])
}

/// Solvency: pool balance == reserve + LP fees owed + protocol owed (fees held
/// outside the curve).
fn assert_solvent(f: &Fixture) {
    let r = f.pool.get_reserves();
    let lp = f.pool.lp_fees_owed();
    let po = f.pool.protocol_owed();
    assert_eq!(
        f.balance(&f.token_a, &f.pool.address),
        r.get_unchecked(0) + lp.get_unchecked(0) + po.get_unchecked(0),
        "token A"
    );
    assert_eq!(
        f.balance(&f.token_b, &f.pool.address),
        r.get_unchecked(1) + lp.get_unchecked(1) + po.get_unchecked(1),
        "token B"
    );
}

fn setup() -> Fixture {
    let f = Fixture::new(7, 100_000_000 * M);
    f.init_circular(30);
    f.pool.enable_ticks();
    f
}

#[test]
fn full_range_add_then_remove_conserves() {
    let f = setup();
    let seed = 1_000_000 * M; // 1,000,000 tokens each, balanced
    let l = f
        .pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &0, &90, &0, &u64::MAX);
    assert!(l > 0, "credited liquidity positive");
    assert_solvent(&f);

    // Remove all credited liquidity → get back ≤ deposited (MINIMUM_LIQUIDITY stays).
    let mins = amounts(&f, 0, 0);
    let out = f
        .pool
        .remove_liquidity(&f.lp, &0, &90, &l, &mins, &u64::MAX);
    assert!(
        out.get_unchecked(0) <= seed,
        "x out {} > in {seed}",
        out.get_unchecked(0)
    );
    assert!(
        out.get_unchecked(1) <= seed,
        "y out {} > in {seed}",
        out.get_unchecked(1)
    );
    // Both sides nearly recovered (only the locked minimum + rounding stays).
    assert!(out.get_unchecked(0) > seed - seed / 1000);
    assert_solvent(&f);
    // Position gone.
    assert_eq!(
        f.pool
            .try_remove_liquidity(&f.lp, &0, &90, &1, &mins, &u64::MAX)
            .err()
            .unwrap()
            .unwrap(),
        crate::OrbswapError::PositionNotFound
    );
}

#[test]
fn concentrated_position_is_single_sided_off_balance() {
    let f = setup();
    let seed = 500_000 * M;
    // First: full-range to set θc = 45.
    f.pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &0, &90, &0, &u64::MAX);
    assert_solvent(&f);

    // A tight range straddling balance (44..46): pulls both tokens.
    let l2 = f
        .pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &44, &46, &0, &u64::MAX);
    assert!(l2 > 0);
    assert_solvent(&f);

    // A range entirely above balance [50,60] at θc=45 is single-sided (all Y).
    let before_a = f.balance(&f.token_a, &f.lp);
    let l3 = f
        .pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &50, &60, &0, &u64::MAX);
    assert!(l3 > 0);
    // token A (X) untouched — position above current price is pure Y.
    assert_eq!(
        f.balance(&f.token_a, &f.lp),
        before_a,
        "X should not be pulled"
    );
    assert_solvent(&f);
}

#[test]
fn tick_mode_disables_share_path() {
    let f = setup();
    // deposit() and withdraw() are rejected once ticks are on.
    let e = f
        .pool
        .try_deposit(&f.lp, &amounts(&f, M, M), &0, &u64::MAX)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(e, crate::OrbswapError::TickModeActive);
}

#[test]
fn add_liquidity_requires_tick_mode() {
    // A plain Circular pool (no enable_ticks) rejects add_liquidity.
    let f = Fixture::new(7, 1_000_000 * M);
    f.init_circular(30);
    let e = f
        .pool
        .try_add_liquidity(&f.lp, &amounts(&f, M, M), &0, &90, &0, &u64::MAX)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(e, crate::OrbswapError::TickModeOnly);
}

#[test]
fn first_add_must_be_full_range() {
    let f = setup();
    let e = f
        .pool
        .try_add_liquidity(&f.lp, &amounts(&f, M, M), &40, &50, &0, &u64::MAX)
        .err()
        .unwrap()
        .unwrap();
    assert_eq!(e, crate::OrbswapError::InvalidTickRange);
}

#[test]
fn bad_range_rejected() {
    let f = setup();
    // lower >= upper
    assert!(f
        .pool
        .try_add_liquidity(&f.lp, &amounts(&f, M, M), &50, &50, &0, &u64::MAX)
        .is_err());
    // upper > 90
    assert!(f
        .pool
        .try_add_liquidity(&f.lp, &amounts(&f, M, M), &0, &91, &0, &u64::MAX)
        .is_err());
}

#[test]
fn partial_remove_keeps_position() {
    let f = setup();
    let seed = 800_000 * M;
    let l = f
        .pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &0, &90, &0, &u64::MAX);
    let mins = amounts(&f, 0, 0);
    // Remove half.
    f.pool
        .remove_liquidity(&f.lp, &0, &90, &(l / 2), &mins, &u64::MAX);
    assert_solvent(&f);
    // The other half is still removable.
    let out = f
        .pool
        .remove_liquidity(&f.lp, &0, &90, &(l / 2), &mins, &u64::MAX);
    assert!(out.get_unchecked(0) > 0 && out.get_unchecked(1) > 0);
    assert_solvent(&f);
}

#[test]
fn tick_swap_full_range_matches_share_pool() {
    // M4 golden regression: a single full-range tick position must swap identically
    // to today's share-based Circular pool (same circle, same output).
    let amt = 1_000_000 * M;
    let swap_in = 100 * M;

    let s = Fixture::new(7, 10_000_000 * M);
    s.init_circular(30);
    s.pool.deposit(&s.lp, &amounts(&s, amt, amt), &0, &u64::MAX);
    let out_share = s
        .pool
        .swap(&s.lp, &s.token_a, &swap_in, &s.token_b, &0, &u64::MAX);

    let t = setup();
    t.pool
        .add_liquidity(&t.lp, &amounts(&t, amt, amt), &0, &90, &0, &u64::MAX);
    let out_tick = t
        .pool
        .swap(&t.lp, &t.token_a, &swap_in, &t.token_b, &0, &u64::MAX);

    let diff = (out_share - out_tick).abs();
    assert!(
        diff <= out_share / 100_000 + 16,
        "share {out_share} vs tick {out_tick} (diff {diff})"
    );
}

#[test]
fn tick_swap_is_solvent_and_moves_price() {
    let f = setup();
    let seed = 1_000_000 * M;
    f.pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &0, &90, &0, &u64::MAX);
    assert_solvent(&f);

    // Swap X→Y, then Y→X; solvency holds throughout and round-trip doesn't profit.
    let before_a = f.balance(&f.token_a, &f.lp);
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &(500 * M), &f.token_b, &0, &u64::MAX);
    assert!(out > 0 && out < 500 * M, "out {out}");
    assert_solvent(&f);
    let back = f
        .pool
        .swap(&f.lp, &f.token_b, &out, &f.token_a, &0, &u64::MAX);
    assert!(back <= 500 * M, "round trip profited: {back}");
    assert_solvent(&f);
    let _ = before_a;
}

#[test]
fn tick_lp_earns_fees() {
    let f = setup();
    let seed = 1_000_000 * M;
    let l = f
        .pool
        .add_liquidity(&f.lp, &amounts(&f, seed, seed), &0, &90, &0, &u64::MAX);

    // Trades generate fees into the LP pot.
    for _ in 0..5 {
        let out = f
            .pool
            .swap(&f.lp, &f.token_a, &(2_000 * M), &f.token_b, &0, &u64::MAX);
        f.pool
            .swap(&f.lp, &f.token_b, &out, &f.token_a, &0, &u64::MAX);
    }
    let pot = f.pool.lp_fees_owed();
    assert!(
        pot.get_unchecked(0) > 0 && pot.get_unchecked(1) > 0,
        "fees should accrue both sides: {:?}",
        (pot.get_unchecked(0), pot.get_unchecked(1))
    );
    assert_solvent(&f);

    // Removing the (sole) position reclaims essentially all the fees.
    let mins = amounts(&f, 0, 0);
    f.pool
        .remove_liquidity(&f.lp, &0, &90, &l, &mins, &u64::MAX);
    let after = f.pool.lp_fees_owed();
    // Only the locked-minimum's tiny share may remain.
    assert!(
        after.get_unchecked(0) < pot.get_unchecked(0) / 100 + 1,
        "x fees not paid"
    );
    assert!(
        after.get_unchecked(1) < pot.get_unchecked(1) / 100 + 1,
        "y fees not paid"
    );
    assert_solvent(&f);
}

#[test]
fn enable_ticks_only_circular_and_pre_liquidity() {
    // SuperElliptical can't enable ticks.
    let f = Fixture::new(7, 1_000_000 * M);
    f.init_superelliptical(3 * super::_WAD, 5 * super::_WAD, 30);
    assert_eq!(
        f.pool.try_enable_ticks().err().unwrap().unwrap(),
        crate::OrbswapError::TickModeOnly
    );

    // Circular with existing share liquidity can't switch to ticks.
    let g = Fixture::new(7, 10_000_000 * M);
    g.init_circular(30);
    let bal = amounts(&g, M, M);
    g.pool.deposit(&g.lp, &bal, &0, &u64::MAX);
    assert_eq!(
        g.pool.try_enable_ticks().err().unwrap().unwrap(),
        crate::OrbswapError::AlreadyInitialized
    );
    let _ = Address::generate(&g.env);
}
