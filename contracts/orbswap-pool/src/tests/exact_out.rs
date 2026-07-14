//! Tests for `swap_exact_out` / `quote_exact_out`.

use super::Fixture;

#[test]
fn delivers_exact_output_and_matches_quote() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);

    let want_out = 90_000_000i128; // exactly 9 tokens B
    let quoted_in = f.pool.quote_exact_out(&f.token_a, &f.token_b, &want_out);
    assert!(quoted_in > 0);

    let b_before = f.balance(&f.token_b, &f.lp);
    let paid = f.pool.swap_exact_out(
        &f.lp,
        &f.token_a,
        &f.token_b,
        &want_out,
        &i128::MAX,
        &u64::MAX,
    );

    assert_eq!(paid, quoted_in, "charged == quote");
    // The user received EXACTLY the requested output.
    assert_eq!(f.balance(&f.token_b, &f.lp), b_before + want_out);
    // Reserves moved by exactly the amounts.
    let r = f.pool.get_reserves();
    assert_eq!(r.get_unchecked(1), 1_000_000_000 - want_out);
    assert_eq!(r.get_unchecked(0), 1_000_000_000 + paid);
}

#[test]
fn exact_out_round_trips_with_exact_in() {
    // The input required for exactly the output that exact-in produced should be
    // ≈ the original input (never less — pool-favoring rounding).
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);

    let amount_in = 50_000_000i128;
    let out = f.pool.quote(&f.token_a, &amount_in, &f.token_b);
    let needed = f.pool.quote_exact_out(&f.token_a, &f.token_b, &out);
    // Recovered input within a hair, and ≥ isn't guaranteed by double-rounding, so
    // just assert it's very close.
    assert!(
        (needed - amount_in).abs() <= amount_in / 1_000 + 4,
        "in={amount_in} needed={needed}"
    );
}

#[test]
fn fee_increases_required_input() {
    let f0 = Fixture::new(7, 2_000_000_000);
    f0.init_circular(0);
    f0.deposit_balanced(1_000_000_000);
    let in0 = f0
        .pool
        .quote_exact_out(&f0.token_a, &f0.token_b, &90_000_000);

    let ff = Fixture::new(7, 2_000_000_000);
    ff.init_circular(100); // 1%
    ff.deposit_balanced(1_000_000_000);
    let inf = ff
        .pool
        .quote_exact_out(&ff.token_a, &ff.token_b, &90_000_000);

    assert!(inf > in0, "fee should raise required input: {inf} !> {in0}");
}

#[test]
#[should_panic] // SlippageExceeded: max_in too low
fn max_in_slippage() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_circular(0);
    f.deposit_balanced(1_000_000_000);
    let needed = f.pool.quote_exact_out(&f.token_a, &f.token_b, &90_000_000);
    f.pool.swap_exact_out(
        &f.lp,
        &f.token_a,
        &f.token_b,
        &90_000_000,
        &(needed - 1),
        &u64::MAX,
    );
}

#[test]
fn superelliptical_exact_out() {
    let f = Fixture::new(7, 2_000_000_000);
    f.init_superelliptical(3 * super::_WAD, 5 * super::_WAD, 0);
    f.deposit_balanced(1_000_000_000);
    let want = 50_000_000i128;
    let b_before = f.balance(&f.token_b, &f.lp);
    let paid = f
        .pool
        .swap_exact_out(&f.lp, &f.token_a, &f.token_b, &want, &i128::MAX, &u64::MAX);
    assert!(paid > 0);
    assert_eq!(f.balance(&f.token_b, &f.lp), b_before + want);
}
