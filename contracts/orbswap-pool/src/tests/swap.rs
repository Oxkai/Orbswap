//! Tests for `swap` (todo.md §2.6).

use super::Fixture;

#[test]
fn swap_matches_quote_and_moves_balances() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0); // no fee, to check raw curve math
    f.deposit_balanced(1_000_000_000); // 100 tokens each

    let amount_in = 100_000_000i128; // 10 tokens of A
    let quoted = f.pool.quote(&f.token_a, &amount_in, &f.token_b);
    assert!(quoted > 0, "quote positive");

    let lp_b_before = f.balance(&f.token_b, &f.lp);
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &amount_in, &f.token_b, &0, &u64::MAX);
    assert_eq!(out, quoted, "executed == quote");

    // Circular is concentrated: ~10% swap yields a bit under 10 tokens out.
    assert!(out > 90_000_000 && out < amount_in, "out={out}");
    // Balances moved by exactly the swap amounts.
    assert_eq!(f.balance(&f.token_a, &f.pool.address), 1_100_000_000);
    assert_eq!(f.balance(&f.token_b, &f.pool.address), 1_000_000_000 - out);
    assert_eq!(f.balance(&f.token_b, &f.lp), lp_b_before + out);
}

#[test]
fn roundtrip_no_free_money() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);

    let dx = 50_000_000i128; // 5 tokens A in
    let out_b = f
        .pool
        .swap(&f.lp, &f.token_a, &dx, &f.token_b, &0, &u64::MAX);
    // Swap the B back to A.
    let back_a = f
        .pool
        .swap(&f.lp, &f.token_b, &out_b, &f.token_a, &0, &u64::MAX);
    assert!(back_a <= dx, "trader profited: dx={dx} back={back_a}");
}

#[test]
fn fee_reduces_output() {
    let f_nofee = Fixture::new(7, 2_000_000_000);
    f_nofee.init_circular(0);
    f_nofee.deposit_balanced(1_000_000_000);
    let out_nofee = f_nofee
        .pool
        .quote(&f_nofee.token_a, &100_000_000, &f_nofee.token_b);

    let f_fee = Fixture::new(7, 2_000_000_000);
    f_fee.init_circular(100); // 1%
    f_fee.deposit_balanced(1_000_000_000);
    let out_fee = f_fee
        .pool
        .quote(&f_fee.token_a, &100_000_000, &f_fee.token_b);

    // A 1% fee must reduce the output vs the no-fee pool.
    assert!(
        out_fee < out_nofee,
        "fee should reduce output: {out_fee} !< {out_nofee}"
    );
    // Roughly ~1% less (the curve is near-linear for a small trade at balance).
    assert!(
        out_fee > out_nofee * 98 / 100,
        "fee far too large: {out_fee} vs {out_nofee}"
    );
}

#[test]
fn fee_stays_with_lps() {
    // The full input (including fee) lands in the pool; only the net moves along the
    // curve. The fee is held in the LP-fee pot OUTSIDE the curve (default protocol
    // bps = 0 ⇒ the whole fee is the LPs'), so pool balance still grows by the full
    // amount_in while the curve reserve grows only by the net.
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(100); // 1%
    f.deposit_balanced(1_000_000_000);

    let amount_in = 100_000_000i128;
    let fee = amount_in / 100; // 1%
    let out = f
        .pool
        .swap(&f.lp, &f.token_a, &amount_in, &f.token_b, &0, &u64::MAX);

    // On-chain balance grew by the FULL input; the fee sits in the LP pot, the rest
    // (net) in the curve reserve. reserve_out -= out.
    assert_eq!(
        f.balance(&f.token_a, &f.pool.address),
        1_000_000_000 + amount_in
    );
    assert_eq!(
        f.pool.get_reserves().get_unchecked(0),
        1_000_000_000 + amount_in - fee
    );
    assert_eq!(f.pool.lp_fees_owed().get_unchecked(0), fee);
    assert_eq!(f.balance(&f.token_b, &f.pool.address), 1_000_000_000 - out);
}

#[test]
fn superelliptical_swap_works() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_superelliptical(3 * super::_WAD, 5 * super::_WAD, 0);
    f.deposit_balanced(1_000_000_000);
    let out = f.pool.quote(&f.token_a, &100_000_000, &f.token_b);
    assert!(out > 0, "csemm quote positive: {out}");
}

#[test]
#[should_panic] // InvalidAmount: same token
fn same_token_rejected() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    f.pool
        .swap(&f.lp, &f.token_a, &1_000_000, &f.token_a, &0, &u64::MAX);
}

#[test]
#[should_panic] // SlippageExceeded
fn min_out_slippage() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    f.pool.swap(
        &f.lp,
        &f.token_a,
        &100_000_000,
        &f.token_b,
        &i128::MAX,
        &u64::MAX,
    );
}
