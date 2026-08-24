//! Phase 0a — the defect an oracle-fed rate multiplier introduces, proven at the
//! math level (no contract code required). See `todo.md` §0.
//!
//! **The finding.** A rate update revalues one leg, moving the pool off the
//! invariant. The pool does *not* freeze: [`swap_out`] solves the out-leg from the
//! in-leg alone, so any trade returns an on-curve post-state and the contract's
//! guard accepts it. Instead the entire revaluation is paid out as a **one-shot
//! pot to whoever trades first — at any trade size, including dust.**
//!
//! This is strictly worse than ordinary LVR: correcting a mispricing normally
//! requires capital proportional to it. Here it is free.

mod common;

use common::*;
use orbswap_math::csemm::{invariant_holds, swap_out};
use orbswap_math::ndim::{invariant_holds_n, swap_out_n};

const TWO_PLUS_SQRT2: i128 = 3_414_213_562_373_095_049;
/// The contract's post-swap guard tolerance (orbswap-pool/src/lib.rs:39).
const INVARIANT_EPSILON: i128 = 1_000_000_000; // 1e-9 relative
/// One basis point — the smallest FX move worth modelling.
const ONE_BP: i128 = 10_000;

/// The circle's balanced point is x = y = 1.0 WAD at α = β = 2+√2.
fn balanced_2() -> (i128, i128, i128, i128) {
    (S, S, TWO_PLUS_SQRT2, TWO_PLUS_SQRT2)
}

#[test]
fn baseline_balanced_point_is_on_curve() {
    let (x, y, a, b) = balanced_2();
    assert!(
        invariant_holds(x, y, a, b, INVARIANT_EPSILON),
        "x = y = 1.0 must satisfy the circle at 2+√2"
    );
}

#[test]
fn one_bp_rate_shift_leaves_the_curve() {
    let (x, y, a, b) = balanced_2();
    let y_shift = y + y / ONE_BP;
    assert!(
        !invariant_holds(x, y_shift, a, b, INVARIANT_EPSILON),
        "a 1 bp revaluation is 1e-4 relative — 100000x the 1e-9 guard tolerance"
    );
}

#[test]
fn swap_from_off_curve_is_accepted_not_rejected() {
    let (x, y, a, b) = balanced_2();
    let y_shift = y + y / ONE_BP;
    let (_, nx, ny) = swap_out(x, y_shift, a, b, S / 100).expect("swap must succeed");
    assert!(
        invariant_holds(nx, ny, a, b, INVARIANT_EPSILON),
        "post-state is solved on-curve from new_x, so the contract guard PASSES — \
         the pool does not brick, it pays out"
    );
}

#[test]
fn any_trade_size_extracts_the_entire_discrepancy() {
    let (x, y, a, b) = balanced_2();
    let y_shift = y + y / ONE_BP;
    let discrepancy = y_shift - y;

    // Trade sizes spanning seven orders of magnitude.
    for div in [10i128, 100, 10_000, 1_000_000, 100_000_000] {
        let dx = S / div;
        let honest = swap_out(x, y, a, b, dx).expect("honest").0;
        let off = swap_out(x, y_shift, a, b, dx).expect("off-curve").0;
        assert_eq!(
            off - honest,
            discrepancy,
            "trade size {dx} must extract exactly the full discrepancy, not a share of it"
        );
    }
}

#[test]
fn dust_trade_extracts_many_multiples_of_its_own_size() {
    let (x, y, a, b) = balanced_2();
    let y_shift = y + y / ONE_BP;
    let dust = S / 100_000_000; // 1e10 WAD
    let honest = swap_out(x, y, a, b, dust).expect("honest").0;
    let off = swap_out(x, y_shift, a, b, dust).expect("off-curve").0;
    let extra = off - honest;
    assert!(
        extra > dust * 1_000,
        "dust input {dust} extracted {extra} — over 1000x its own size"
    );
}

#[test]
fn extraction_is_one_shot_then_the_pool_is_normal() {
    let (x, y, a, b) = balanced_2();
    let mut cur_x = x;
    let mut cur_y = y + y / ONE_BP;
    let dx = S / 1_000_000;

    let first = swap_out(cur_x, cur_y, a, b, dx).expect("first");
    cur_x = first.1;
    cur_y = first.2;
    assert!(
        invariant_holds(cur_x, cur_y, a, b, INVARIANT_EPSILON),
        "one trade restores the curve"
    );

    let second = swap_out(cur_x, cur_y, a, b, dx).expect("second").0;
    assert!(
        first.0 > second * 10,
        "the first trader took the pot ({}); the second gets an ordinary fill ({})",
        first.0,
        second
    );
}

#[test]
fn ndim_self_heals_the_same_way() {
    let a = TWO_PLUS_SQRT2;
    let params = [a, a, a, a];
    // 4-token balanced point: Σ(1−x/α)^2 = 1 → x/α = 1/2.
    let bal = a / 2;
    let on = [bal, bal, bal, bal];
    assert!(invariant_holds_n(&on, &params, INVARIANT_EPSILON));

    // Revalue an untouched leg, then trade between two others.
    let mut off = on;
    off[3] += off[3] / ONE_BP;
    assert!(!invariant_holds_n(&off, &params, INVARIANT_EPSILON));

    let (_, nin, nout) = swap_out_n(&off, &params, 0, 1, a / 200).expect("ndim swap");
    let mut post = off;
    post[0] = nin;
    post[1] = nout;
    assert!(
        invariant_holds_n(&post, &params, INVARIANT_EPSILON),
        "the out-leg absorbs the off-curve residual of the untouched legs, so the \
         n-dim guard also PASSES — same payout defect, not a freeze"
    );
}
