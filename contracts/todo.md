# Rate-Aware Pools — Implementation Plan

Extend the existing Orbswap pool so its **balanced point sits at an oracle FX rate**
instead of hardcoded 1:1, enabling `USDC/<local-currency>` pools. Single-operator
(anchor-run) design; the pool is a quoting-and-settlement primitive over one
operator's inventory, not a public multi-LP venue.

Status of the base: 215 tests, fuzzed, clippy-clean, deployed on testnet.
This plan **adds to** that; it does not restructure it.

---
## 0. The defect this plan exists to fix

**Proven at the math level — `orbswap-math/tests/rate_shift.rs`, 7 tests, passing.**

### What actually happens (measured, not predicted)

A rate update revalues one leg. Stored **native** reserves do not change, but
`internal = native x scale x rate` does — on the non-USD leg only. The pool's state
point leaves the curve sideways.

The pool then does **not** freeze. `csemm::swap_out` computes
`new_y = partner(new_x, ...)` — solving the out-leg **from the in-leg alone**,
using the passed-in `y` only for `amount_out = y - new_y`. So the post-state is
on-curve *by construction*, the guard at
[lib.rs:1259](orbswap-pool/src/lib.rs#L1259) **passes**, and the swap is accepted.

Instead, the whole revaluation is paid out as a **one-shot pot to whoever trades
first**. Measured with a 1 bp shift on a balanced circle pool:

| Trade size (WAD) | Extra extracted | Share of discrepancy |
|---|---|---|
| 1e17 | 1e14 | **100%** |
| 1e16 | 1e14 | **100%** |
| 1e14 | 1e14 | **100%** |
| 1e12 | 1e14 | **100%** |
| 1e10 | 1e14 | **100%** |

Across a 10,000,000x range of trade sizes the extraction is **identical and total**.
A dust trade of 1e10 extracts 1e14 — **10,000x its own input**. After that one
trade the pool is back on-curve and behaves normally.

`ndim::swap_out_n` behaves the same way: the out-leg absorbs the off-curve residual
of the untouched legs, so the n-dimensional guard at
[lib.rs:1275](orbswap-pool/src/lib.rs#L1275) also passes.

### Why this is worse than ordinary LVR

Correcting a mispricing normally requires an arbitrageur to commit capital
proportional to the mispricing, and competition compresses the profit. Here the
pot is **fixed, total, and free** — the winner is whoever lands first, at any size.

Concretely: a $1M pool, a 1% rate move, a $10,000 pot, taken by a $0.01 trade.
At Lightecho's 10-minute feed cadence that is **144 such events per day**.

### What this changes about the fix

`re_anchor()` is still the mechanism, but its purpose is **not** to unfreeze a dead
pool — it is to move the pool onto the new curve position **before anyone can trade
against the stale one**. That makes the freshness gate load-bearing rather than
defensive: the pool must **refuse to trade** from the moment the rate is known to
have moved until the re-anchor lands.

Sequencing is therefore mandatory and atomic-in-effect:

```
rate moves -> pool CLOSED -> re_anchor() -> pool OPEN at the new curve
```

Any window where the pool is open and off-curve is a window where the pot is free.

### Why this is cheap to fix here

`s` (liquidity scale) and `total_shares` (LP claims) are **already separate storage
keys** (`DataKey::S`, `DataKey::TotalShares`), kept equal only by convention in
deposit/withdraw ([lib.rs:363-364, 385-386, 717-718](orbswap-pool/src/lib.rs#L363-L364)).
So re-anchoring means recomputing `s` alone, deliberately breaking
`total_shares == s`. Share value then marks to market — the correct semantics.

## 1. Scope

| In | Out |
|---|---|
| `SuperElliptical`, 2-token, `USDC/<local>` | `Circular` tick pools (peg hardcoded at 45°) |
| SEP-40 oracle client, cached rate | Sub-degree tick resolution (former §5.2) |
| Explicit admin `re_anchor()` | Automatic / trustless repegging |
| Deviation bounds, staleness, circuit breaker | n > 2 rate-aware pools |
| Operator LP allowlist | Upgradability, role-based access control |
| Frontend rate + spread display | Multi-hop routing across rate-aware pools |

**Invariants that must survive unchanged:**

- `balance == reserves + ProtocolOwed + LpFeesOwed`, exact to the integer
- All rounding favors the pool
- No floats, no `panic!`/`unwrap` in library paths
- `orbswap-math` stays `#![no_std]` and zero-dependency — **all oracle code lives in
  `orbswap-pool`**, never in the math crate

---

## 2. New error variants

Append to `OrbswapError` ([errors.rs](orbswap-pool/src/errors.rs)); next free
discriminant is **23**.

| Variant | Disc. | Raised when |
|---|---|---|
| `RateStale` | 23 | Cached rate older than `max_age_secs` |
| `RateDeviation` | 24 | New rate deviates from last accepted by more than `max_deviation_bps` |
| `RateBreakerTripped` | 25 | Breaker latched; swaps/deposits halted |
| `OffCurve` | 26 | State off-invariant beyond tolerance; `re_anchor()` required |
| `NotOperator` | 27 | Caller not on the LP allowlist while operator mode is on |
| `OracleUnavailable` | 28 | SEP-40 feed returned `None` or the call failed |
| `InvalidRateConfig` | 29 | Bad feed address, bounds, or decimals at configure time |

---

## 3. New storage keys

Append to `DataKey` ([storage.rs](orbswap-pool/src/storage.rs)). All **instance**
storage except where noted.

| Key | Holds |
|---|---|
| `RateConfig` | `RateConfig` struct (below); absent ⇒ pool is parity-mode |
| `Rates` | `Vec<i128>` last-accepted rate per token, WAD, parallel to `Config.tokens` |
| `RateLastTime` | `u64` ledger timestamp of the last accepted rate |
| `RateBreaker` | `bool` latched breaker flag |
| `OperatorMode` | `bool` LP allowlist enforced |
| `Operator(Address)` | `bool` per-address LP permission (**persistent**) |

### Types (`types.rs`)

```rust
#[contracttype]
pub struct RateConfig {
    pub feed: Address,           // SEP-40 PriceFeed contract
    pub quote_index: u32,        // index into Config.tokens priced by the feed
    pub numeraire_index: u32,    // the leg pinned at rate = WAD (the USDC side)
    pub cross: bool,             // true ⇒ divide two lastprice calls (XLM-denominated feed)
    pub max_age_secs: u64,       // staleness threshold
    pub max_deviation_bps: i128, // per-update move that trips the breaker
    pub feed_decimals: u32,      // cached from feed.decimals()
}
```

`Rates` holds WAD multipliers with the **numeraire pinned at `WAD`** (the USDC leg
is always exactly 1.0), so only `quote_index` ever moves.

---

## 4. New events

Append to [events.rs](orbswap-pool/src/events.rs), matching the existing
`#[contractevent]` struct + snake_case publisher pattern.

| Event | Fields |
|---|---|
| `RateUpdated` | `token: Address, old_rate: i128, new_rate: i128, timestamp: u64` |
| `ReAnchored` | `old_s: i128, new_s: i128, rate: i128` |
| `RateBreakerChanged` | `tripped: bool, reason: Symbol` |
| `OperatorChanged` | `who: Address, allowed: bool` |
| `RateConfigured` | `feed: Address, quote_index: u32, max_age_secs: u64, max_deviation_bps: i128` |

---

## Phase 0 — Prove the defect first ✅ DONE (7 tests)

**Nothing else starts until this fails for the documented reason.**

Split in two, because the math-level proof needs **no new contract code** and can
run today, while the contract-level proof can only exist after Phase 2.

### 0a — Math level (runs immediately) — `orbswap-math/tests/rate_shift.rs`

The tolerance arithmetic that makes this decisive:

| Quantity | Value | Source |
|---|---|---|
| On-curve residual from transcendental rounding | ~1e-13 relative | [csemm.rs `invariant_holds` docs](orbswap-math/src/csemm.rs#L196) |
| Contract tolerance `INVARIANT_EPSILON` | 1e-9 relative | [lib.rs:39](orbswap-pool/src/lib.rs#L39) |
| Smallest realistic FX move (1 bp) | 1e-4 relative | — |

A 1 bp rate move is **100,000× larger** than the guard's tolerance. There is no
rate change small enough to slip through.

| Test | Asserts | Status |
|---|---|---|
| `baseline_balanced_point_is_on_curve` | `x = y = 1.0` satisfies the circle at 2+√2 | PASS |
| `one_bp_rate_shift_leaves_the_curve` | 1 bp revaluation ⇒ `invariant_holds == false` | PASS |
| `swap_from_off_curve_is_accepted_not_rejected` | Post-state is on-curve ⇒ contract guard **passes**; no freeze | PASS |
| `any_trade_size_extracts_the_entire_discrepancy` | Across 7 orders of magnitude, extraction is exactly 100% | PASS |
| `dust_trade_extracts_many_multiples_of_its_own_size` | 1e10 input extracts >1000x itself | PASS |
| `extraction_is_one_shot_then_the_pool_is_normal` | First trader takes the pot; second gets an ordinary fill | PASS |
| `ndim_self_heals_the_same_way` | n-dim guard also passes; same payout defect | PASS |

**Recorded result:** there is no "breaking delta" to find — the guard never fires.
The correct number for the proposal is the **extraction ratio**: 100% of the
revaluation, at any trade size, to the first trader.

### 0b — Contract level (after Phase 2) — `orbswap-pool/src/tests/rates.rs`

| Test | Asserts |
|---|---|
| `rate_change_bricks_pool_without_reanchor` | Init CSEMM, deposit balanced, shift rate, `swap` ⇒ `Err(InvariantViolation)` |
| `bricked_pool_rejects_every_swap` | Both directions, many sizes — all revert; pool is permanently dead |
| `bricked_pool_still_allows_withdraw` | Confirms the exit path survives (pre-existing behavior, must not regress) |

Deliverable: the exact breaking delta, recorded in writing. This is the evidence
the proposal cites as a self-found defect.

---

## Phase 1 — SEP-40 oracle client ✅ DONE (15 tests)

New module `orbswap-pool/src/rates.rs`.

### SEP-40 interface — verified against the spec 2026-08-21

Source: [stellar-protocol/ecosystem/sep-0040.md](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0040.md)

```rust
#[contracttype]
pub struct PriceData { price: i128, timestamp: u64 }

#[contracttype]
enum Asset { Stellar(Address), Other(Symbol) }

pub trait PriceFeedTrait {
    fn base(env: Env) -> Asset;
    fn assets(env: Env) -> Vec<Asset>;
    fn decimals(env: Env) -> u32;
    fn resolution(env: Env) -> u32;
    fn price(env: Env, asset: Asset, timestamp: u64) -> Option<PriceData>;
    fn prices(env: Env, asset: Asset, records: u32) -> Option<Vec<PriceData>>;
    fn lastprice(env: Env, asset: Asset) -> Option<PriceData>;
}
```

We consume exactly three: **`decimals`** (once, at `configure_rates`),
**`lastprice`** (each `poke_rate`), and **`base`** (once, to assert the feed quotes
against the numeraire we expect).

**Note:** `twap` is **not** in SEP-40 — it is a Reflector extension. Do not depend
on it, or the pool stops being feed-agnostic. If TWAP smoothing is wanted later,
build it from `prices(asset, records)`, which *is* standard.

**Cross-rate warning:** Lightecho feeds are denominated against XLM, not USD. A
`USDC/<local>` pool therefore needs `lastprice(local) / lastprice(USDC)` — two
calls and a division, with the deviation check applied to the **resulting
cross-rate**, not the individual legs. Reflector majors may quote against USD
directly. `RateConfig` must record which mode the feed is in.

| Function | Input | Output | Notes |
|---|---|---|---|
| `fetch_rate` | `env, &RateConfig` | `Result<(i128, u64), OrbswapError>` | Cross-contract `lastprice`; normalizes `feed_decimals → WAD`. `None` ⇒ `OracleUnavailable` |
| `get_rates` | `env` | `Vec<i128>` | Cached rates; all-`WAD` when `RateConfig` absent |
| `set_rates` | `env, &Vec<i128>, u64` | `()` | Writes `Rates` + `RateLastTime` |
| `is_fresh` | `env, &RateConfig, now: u64` | `bool` | `now - RateLastTime <= max_age_secs` |
| `deviation_bps` | `old: i128, new: i128` | `Result<i128, OrbswapError>` | Absolute relative move in bps |
| `require_usable` | `env, &RateConfig` | `Result<(), OrbswapError>` | Composite guard: breaker not tripped **and** `is_fresh` |

### Tests — `tests/rates_oracle.rs`

| Test | Asserts |
|---|---|
| `fetch_normalizes_decimals` | 14-dec and 18-dec mock feeds both yield WAD |
| `fetch_missing_price_errors` | Feed returning `None` ⇒ `OracleUnavailable` |
| `stale_rate_detected` | Advance ledger past `max_age_secs` ⇒ `is_fresh == false` |
| `deviation_bps_symmetric` | up-move and down-move of equal ratio give equal bps |
| `deviation_bps_overflow_safe` | Extreme `i128` inputs ⇒ `Overflow`, never panic |
| `parity_mode_returns_wad` | No `RateConfig` ⇒ `get_rates` all `WAD` |

A **mock SEP-40 feed contract** goes in `tests/mock_feed.rs`: settable price,
settable timestamp, settable decimals, plus a mode that returns `None`.

---

## Phase 2 — Thread rates through the mapping ✅ DONE (20 tests)

The mechanical change. `internal()` gains a rate argument; every caller updates.

| Function | Before | After |
|---|---|---|
| `internal` | `(native, scale) -> Result<i128>` | `(native, scale, rate) -> Result<i128>` |
| `native_from_internal` | `(internal, scale)` | `(internal, scale, rate)` |

**Call sites to update** (all in [lib.rs](orbswap-pool/src/lib.rs)): 103–104, 348,
350, 369–370, 377–378, 473–474, 591–592, 1146–1147, 1219–1220, 1334, 1356–1357.

Rounding direction at each site must be **preserved exactly** — the rate multiply
must not silently flip a `Rounding::Down` into a value that favors the trader.

### New entrypoints

| Function | Signature | Auth | Returns / Errors |
|---|---|---|---|
| `configure_rates` | `(env, feed: Address, quote_index: u32, max_age_secs: u64, max_deviation_bps: i128)` | `admin` | `Result<(), OrbswapError>`; `InvalidRateConfig` on bad index/bounds; `AlreadyInitialized` if rates already configured |
| `poke_rate` | `(env)` | none (permissionless keeper) | `Result<i128, OrbswapError>` — fetches, checks deviation, accepts or trips breaker. Emits `RateUpdated` |
| `get_rate` | `(env, token: Address)` | none (view) | `Result<i128, OrbswapError>` — cached WAD rate |
| `rate_status` | `(env)` | none (view) | `(rate: i128, last_time: u64, fresh: bool, breaker: bool)` |

`poke_rate` is permissionless **by design** — anyone may push a fresh price, but
only within `max_deviation_bps`, and it can only ever *trip* the breaker, never
clear it.

### Tests — `tests/rates_mapping.rs`

| Test | Asserts |
|---|---|
| `parity_pool_unchanged` | With no `RateConfig`, every existing swap/deposit result is **bit-identical** to the pre-change suite |
| `balanced_point_at_rate` | Rate 1:1000 ⇒ balanced reserves are 1:1000, not 1:1 |
| `quote_matches_rate_at_balance` | At balance, `quote` returns oracle rate ± fee, within 1 bp |
| `rounding_still_favors_pool` | Round-trip swap at non-unit rate never returns more than input |
| `configure_rates_bad_index` | `quote_index >= tokens.len()` ⇒ `InvalidRateConfig` |
| `configure_rates_twice` | Second call ⇒ `AlreadyInitialized` |
| `poke_within_deviation_accepts` | Move under bound ⇒ accepted, `RateUpdated` emitted |
| `poke_beyond_deviation_trips` | Move over bound ⇒ `RateDeviation`, breaker latched |

---

## Phase 3 — Re-anchor ✅ DONE (16 tests)

The heart of the plan.

| Function | Signature | Auth | Returns / Errors |
|---|---|---|---|
| `re_anchor` | `(env, deadline: u64)` | `admin` | `Result<i128, OrbswapError>` — new `s`. Emits `ReAnchored` |
| `is_on_curve` | `(env)` | none (view) | `bool` — current state within `INVARIANT_EPSILON` |
| `curve_drift` | `(env)` | none (view) | `i128` — signed relative drift, WAD; monitoring hook |

**Contract of `re_anchor`:** recompute `s` such that the invariant holds at the
current `(reserves, rates)`. `total_shares` is left **untouched** — this is the
deliberate break of the `total_shares == s` convention, and it is what marks share
value to market.

Doc comment must state plainly: *after `re_anchor`, `total_shares != s`, and
`shares_of(who) / total_shares` remains each LP's fractional claim.*

**Swap-path guard.** Every mutating path (`swap`, `swap_exact_out`, `deposit`,
`add_liquidity`) gains an early `rates::require_usable()` plus an on-curve check,
returning `OffCurve` rather than `InvariantViolation` when a re-anchor is pending.
`withdraw` is **exempt** — LPs must always be able to exit.

### Tests — `tests/rates_reanchor.rs`

| Test | Asserts |
|---|---|
| `reanchor_restores_curve` | Phase 0's bricking sequence + `re_anchor` ⇒ swap succeeds |
| `reanchor_preserves_share_ratios` | Two LPs at 70/30 hold 70/30 after re-anchor |
| `reanchor_preserves_solvency` | `balance == reserves + ProtocolOwed + LpFeesOwed` still exact |
| `reanchor_marks_to_market` | Withdrawing after an adverse rate move returns fewer units of the depreciated leg |
| `reanchor_no_free_value` | Deposit → re-anchor → withdraw never returns more than deposited |
| `reanchor_requires_admin` | Non-admin ⇒ `Unauthorized` |
| `reanchor_empty_pool` | `s == 0` ⇒ no-op, no panic |
| `swap_blocked_while_off_curve` | Off-curve ⇒ `OffCurve`, not `InvariantViolation` |
| `withdraw_allowed_while_off_curve` | Withdraw succeeds even off-curve |

---

## Phase 4 — Oracle safety ✅ DONE (13 tests + 6 adversarial)

Directly answers the Feb 2026 YieldBlox/Blend V2 precedent: a correctly-functioning
oracle faithfully reporting a manipulated price.

| Function | Signature | Auth | Returns |
|---|---|---|---|
| `trip_breaker` | `(env, reason: Symbol)` | `admin` | `Result<(), OrbswapError>` |
| `reset_breaker` | `(env)` | `admin` | `Result<(), OrbswapError>` — clears latch; **admin-only, never automatic** |
| `set_rate_bounds` | `(env, max_age_secs: u64, max_deviation_bps: i128)` | `admin` | `Result<(), OrbswapError>` |

**Breaker semantics — must hold under test:**

1. Trips automatically when `poke_rate` sees a move beyond `max_deviation_bps`
2. Trips automatically when a swap finds the cached rate stale
3. While tripped: `swap`, `swap_exact_out`, `deposit`, `add_liquidity` all revert
4. While tripped: **`withdraw` and `remove_liquidity` still work** — non-negotiable
5. Only `admin` clears it; no time-based auto-reset

### Tests — `tests/rates_safety.rs`

| Test | Asserts |
|---|---|
| `stale_rate_blocks_swap` | Past `max_age_secs` ⇒ `RateStale` |
| `stale_rate_allows_withdraw` | Withdraw succeeds while stale |
| `breaker_blocks_swap_and_deposit` | Both ⇒ `RateBreakerTripped` |
| `breaker_allows_withdraw` | Exit path open while tripped |
| `breaker_latches` | Rate returning to normal does **not** auto-clear |
| `breaker_reset_admin_only` | Non-admin `reset_breaker` ⇒ `Unauthorized` |
| `deviation_bound_enforced` | 100× move (the YieldBlox shape) ⇒ trips, no trade at bad price |
| `oracle_unavailable_blocks_swap` | Feed `None` ⇒ `OracleUnavailable`, no fallback to stale |

### Adversarial — append to `tests/adversarial.rs`

| Test | Asserts |
|---|---|
| `sandwich_around_rate_update` | swap → `poke_rate` → swap in one ledger extracts nothing beyond fees |
| `reanchor_frontrun` | Swap immediately before an admin `re_anchor` gains no advantage |
| `repeated_poke_no_drift` | 1000 `poke_rate` calls at an unchanged price leave state identical |

---

## Phase 5 — Operator mode ✅ DONE (7 tests)

| Function | Signature | Auth | Returns |
|---|---|---|---|
| `set_operator_mode` | `(env, enabled: bool)` | `admin` | `Result<(), OrbswapError>` |
| `set_operator` | `(env, who: Address, allowed: bool)` | `admin` | `Result<(), OrbswapError>` — emits `OperatorChanged` |
| `is_operator` | `(env, who: Address)` | none (view) | `bool` |

When `OperatorMode` is on, `deposit` / `add_liquidity` require an allowlisted
caller (`NotOperator` otherwise). **`withdraw` is never gated** — an LP whose
permission is revoked must still be able to exit.

`swap` is **not** gated: the whole point is that anyone can trade against the
operator's inventory.

### Tests — `tests/operator.rs`

| Test | Asserts |
|---|---|
| `operator_mode_off_is_permissionless` | Default behavior unchanged |
| `non_operator_deposit_rejected` | ⇒ `NotOperator` |
| `operator_deposit_allowed` | Allowlisted LP succeeds |
| `revoked_operator_can_still_withdraw` | Exit path open after revocation |
| `swap_open_to_anyone` | Non-operator swaps fine in operator mode |

---

## Phase 6 — Deploy, frontend, measurement ✅ DONE

| Deliverable | Where | Notes |
|---|---|---|
| SEP-40 feed stub | `orbswap-feed-stub/` | Operator-controlled prices, faithful SEP-40 surface, 7 tests. **Testnet only** — production points at Reflector/Lightecho by address, no code change |
| Deploy script | `scripts/deploy_rate_pool.sh` | Feed → pool → `configure_rates` → equal-**value** seed → smoke test of the full repeg cycle → operator mode |
| Keeper | `scripts/keeper.sh` | `poke_rate` → `re_anchor` if needed → append spread row. `--once` for cron |
| Spread log | `deployments/spread_log.csv` | `utc,rate_wad,oracle_out,pool_out,spread_bps,fresh,breaker,reanchored` |
| Frontend reads | `frontend/lib/stellar/rates.ts` | `rateStatus`, `getRate`, `isOnCurve`, `curveDrift`, `operatorStatus`, `spreadBps` |
| Frontend UI | `frontend/components/app/swap/RateBanner.tsx` | Live rate, spread vs mid, and which of the three halts is active |

**Not done: the deployment itself.** The scripts are written and syntax-checked
but have not been run — that needs a funded testnet identity and network access.
Run `bash scripts/deploy_rate_pool.sh`, paste the resulting pool/feed addresses
into `RATE_POOL` in `frontend/lib/stellar/rates.ts`, then start the keeper.

### The spread log is the deliverable

Everything else exists to produce it. Each keeper tick records what the pool
**would** quote against the oracle mid; a week of those rows is the artifact an
anchor is shown to answer *"would you route conversion through this?"* (§10).
`spread_bps` is computed identically in `keeper.sh` and `rates.ts::spreadBps`, so
the dashboard and the log can never disagree.

---

## 7. Budgets — MEASURED (Phases 0–5 complete)

| Contract | Baseline | Now | Limit | Headroom |
|---|---|---|---|---|
| **orbswap-pool** | 89.0 KB | **111.3 KB** | 128 KB | **16.7 KB** |
| orbswap-factory | 12.0 KB | 12.1 KB | 128 KB | — |
| orbswap-router | 12.0 KB | 12.6 KB | 128 KB | — |
| orbswap-feed-stub (new) | — | 11.8 KB | 128 KB | testnet only |

Phases 0–3 cost 19.2 KB; Phases 4–5 cost only **3.1 KB** — far under the 9–18 KB
estimate, because merging `trip_breaker`/`reset_breaker` into `set_breaker(bool)`
and folding the operator views into `operator_status` removed most of the
per-entrypoint SDK dispatch overhead. **No further mitigations were needed.**

**Oracle calls per swap: zero.** Swaps read the cached rate; only `poke_rate`
touches the feed, which keeps the CPU budget flat and gives the staleness and
deviation checks a single choke point.

Remaining mitigations, if Phase 6 or an audit needs room:

1. Drop `curve_drift` (derivable from `is_on_curve` + reserves)
2. Fold `rate_status` into `get_rate` as a tuple return
3. Feature-gate the tick-pool code out of rate-aware builds

---

## 7b. File-change map

Every file this plan touches, and what happens to it.

### New files

| File | Contents |
|---|---|
| `orbswap-math/tests/rate_shift.rs` | Phase 0a — math-level proof of the defect |
| `orbswap-pool/src/rates.rs` | SEP-40 client, rate cache, freshness/deviation helpers |
| `orbswap-pool/src/tests/mock_feed.rs` | Mock SEP-40 feed: settable price/timestamp/decimals, `None` mode |
| `orbswap-pool/src/tests/rates.rs` | Phase 0b — contract-level bricking proof |
| `orbswap-pool/src/tests/rates_oracle.rs` | Phase 1 tests |
| `orbswap-pool/src/tests/rates_mapping.rs` | Phase 2 tests |
| `orbswap-pool/src/tests/rates_reanchor.rs` | Phase 3 tests |
| `orbswap-pool/src/tests/rates_safety.rs` | Phase 4 tests |
| `orbswap-pool/src/tests/operator.rs` | Phase 5 tests |
| `orbswap-math/fuzz/fuzz_targets/rate_reanchor_roundtrip.rs` | Fuzz: deposit → shift → re-anchor → withdraw never gains value |
| `scripts/deploy_rate_pool.sh` | Deploy + configure + seed |
| `scripts/keeper_poke.sh` | Cadenced `poke_rate` + spread logger |
| `deployments/testnet_rates.json` | Deployment record |
| `frontend/lib/stellar/rates.ts` | `getRate`, `rateStatus`, `isOnCurve` reads |

### Modified files

| File | Change |
|---|---|
| `orbswap-pool/src/lib.rs` | `mod rates;`; `internal`/`native_from_internal` gain a rate arg; **11 call-site clusters** (103–104, 348, 350, 369–370, 377–378, 473–474, 591–592, 1146–1147, 1219–1220, 1334, 1356–1357); new entrypoints; guards on mutating paths |
| `orbswap-pool/src/errors.rs` | 7 variants, discriminants 23–29 |
| `orbswap-pool/src/storage.rs` | 6 `DataKey` variants + typed accessors |
| `orbswap-pool/src/events.rs` | 5 `#[contractevent]` structs + publishers |
| `orbswap-pool/src/types.rs` | `RateConfig` struct |
| `orbswap-pool/src/tests/mod.rs` | Declare new test modules; fixture helpers for rate pools |
| `orbswap-pool/src/tests/adversarial.rs` | 3 rate-specific adversarial tests |
| `orbswap-pool-interface/src/lib.rs` | Add new entrypoints to `PoolInterface` so factory/router see them |
| `orbswap-math/fuzz/Cargo.toml` | Register the new fuzz target |
| `frontend/lib/stellar/config.ts` | Rate-pool address + token entry |
| `frontend/components/app/swap/SwapWidget.tsx` | Rate display, spread vs oracle mid, staleness/breaker banner |

### Deliberately untouched

`orbswap-math/src/*` (except the new test) — the invariant math does not change.
`orbswap-factory`, `orbswap-router` — beyond the interface addition.
All tick-pool code paths — Circular pools stay at parity.

---

## 8. Sequencing

| Day | Work |
|---|---|
| 1 | Phase 0 failing tests; Phase 1 oracle client + mock feed; **measure wasm size** |
| 2 | Phase 2 rates through the mapping; parity-unchanged suite green |
| 3 | Phase 3 re-anchor + on-curve guards; solvency and no-free-value tests |
| 4 | Phase 4 safety + adversarial; Phase 5 operator mode |
| 5 | Phase 6 deploy, seed, keeper, frontend, start the spread log |

Ordering rule: **no phase starts until the previous phase's tests are green.**
Phases 4–5 are the ones to cut if days run short; Phases 0–3 are the deliverable.

---

## 8b. Verification — full results

Run on 2026-08-21, Rust 1.91.0, stellar-cli optimize.

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | **PASS** |
| `RUSTFLAGS="-D warnings" cargo clippy --all-targets --all-features` | **PASS**, 0 warnings |
| `cargo test --all` | **319 passing, 0 failing** |
| wasm build + optimize (4 contracts) | all under the 128 KB limit |
| `tsc --noEmit` + `eslint` (frontend) | clean |
| `next build` | 9 routes, succeeds |

### Fuzzing — 1,000,000 runs per target, zero crashes

| Target | Runs | Result |
|---|---|---|
| `rate_reanchor` (new) | 1,000,000 | pass |
| `roundtrip` | 1,000,000 | pass |
| `swap_invariant` | 1,000,000 | pass |
| `csemm_ccmm_equiv` | 1,000,000 | pass |
| `fixed_point` | 1,000,000 | pass |
| `tick_crossing` | 1,000,000 | pass |

The five pre-existing targets were re-run specifically to confirm the
`invariant_residual_n` extraction in `ndim` changed no behavior.

### Simulation — `orbswap-pool/tests/rate_simulation.rs` (7 tests)

60 keeper cycles of a depreciating currency with two-way trade flow, asserting
solvency and "never open off-curve" **at every step**:

```
60 cycles: 60 repegs, 180 swaps landed, 0 refused
final rate 5.26e14 WAD (quote leg -47%)
s = 7.54e21   total_shares = 1.00e22
```

`s` fell ~25% while `total_shares` held exactly — share value marked to market,
LP claims undiluted. That is the Phase 3 contract, demonstrated over 60 cycles
rather than asserted once.

Also covered: an attacker cannot land a trade in the off-curve window at any size
or direction (15 cycles x 4 sizes x 2 directions); a 100x shock halts trading
while withdrawals stay open; a stale pool recovers with no operator; operator mode
holds across a full cycle.

**The spread widens as the pool skews** — 131 → 832 → 1379 bps over 12 cycles of
one-sided flow. That widening is the automated inventory management, so it is
asserted rather than tolerated.

### Curve calibration — `orbswap-pool/tests/curve_calibration.rs` (3 tests)

Single-leg slippage vs a frictionless fill, 10,000-unit pool, 30 bps fee:

| alpha | 1% trade | 10% | 25% | 40% |
|---|---|---|---|---|
| 2.001 | 0 | 0 | — | 2 |
| 2.01 | — | 7 | — | 29 |
| **2.05** | 3 | **34** | 85 | 138 |
| 2.20 | — | 118 | — | 465 |
| 3.414 (circle) | 41 | **396** | 939 | 1432 |
| 6.00 (boxy) | 55 | 529 | 1229 | 1838 |

**At 10% of reserves `alpha = 2.05` quotes 11.6x tighter than the circle, and
`alpha = 2.01` about 56x.** This changed the deploy default: `deploy_rate_pool.sh`
now ships `alpha = 2.05`, not `2+sqrt(2)`. An FX pool takes its price from the
oracle, so the curve's remaining job is inventory management and it should sit far
flatter than the circle the demo pools use.

The trade-off is real and documented: flatter means less resistance to being
drained, so the oracle guards carry proportionally more of the safety burden.

### One measurement that looked like a bug and was not

Large round trips cost *less* than two nominal fees (51–59 bps vs 60). Reproduced
on **parity** pools, so nothing the rate work introduced. Cause: the second fee is
levied on a notional already reduced by leg-one slippage. `back < spend` holds in
every case and solvency stays exact. It is also why round-trip cost is a poor
proxy for quote tightness — hence the single-leg metric above.

---

## 9. Definition of done

- [x] Phase 0 tests exist (proposal write-up still pending)
- [x] `cargo test --all` green — **319** (from 215)
- [x] `cargo clippy --all-targets --all-features` clean under `-D warnings`
- [x] `cargo fmt --all -- --check` clean
- [x] Parity pools bit-identical to pre-change behavior
- [x] Pool wasm under 128 KB, optimized (111.3 KB)
- [x] Fuzz target `rate_reanchor` added and run at 1M iterations; all 6 targets re-run clean
- [ ] Live testnet pool quoting a real pair at a real oracle rate
- [ ] Seven consecutive days of spread-comparison data collected

---

## 10. End goals

**Engineering.** The pool's balanced point is a parameter, not a constant. Orbswap
becomes the only AMM on Stellar that can quote a non-parity pair — a peso against a
dollar, a yield-bearing wrapper against its base — with oracle failure treated as a
first-class design case rather than post-audit hardening.

**Evidence.** One week of published spread data comparing the pool's on-chain quote
against the anchor's actual off-chain conversion spread. This is the number that
decides whether the thesis is real, and it is the only deliverable that a reviewer,
an anchor, or a grant committee cannot get anywhere else.

**Commercial.** One anchor willing to answer, on the record: *would you route
conversion through this?* The ask is deliberately zero-cost — quote their pair for a
week, show them the comparison, take the answer either way. A "no" in August is
worth more than three more months of building.

**Positioning.** Orbswap stops competing for $47M of Stellar DeFi TVL and starts
being **settlement infrastructure an anchor operates**. That reframing is what makes
it fundable as payments infrastructure rather than as another venue.

**Honest failure condition.** If the spread comparison shows the pool cannot beat
anchor-internal conversion, **publish that**. A documented negative result on
on-chain FX for Stellar is genuinely useful to everyone who would otherwise try it
next, and it is a better outcome than a live pool nobody routes through.

---

## 11. Known limitations — state these, do not hide them

1. **Re-anchoring is manual.** The pool closes between a rate move and the admin
   `re_anchor`. Acceptable for single-operator; unacceptable for a public pool.
   Automatic repegging is the mainnet item.
2. **Circular tick pools stay at parity.** The `[0,90]` angle space hardcodes 45° as
   the peg; non-parity ticks are a second repeg problem layered on the first.
3. **Soroban sits outside the classic payment path.** `PathPaymentStrictSend`
   traverses SDEX orderbooks and classic liquidity pools, not Soroban contracts.
   Reachability is via Soroban aggregators and direct calls — **verify current
   protocol behavior before claiming otherwise.**
4. **LVR is real and unmitigated.** Every rate update creates an arbitrage
   opportunity against LPs. Single-operator design makes this an accepted cost of
   operations, not a solved problem. A public multi-LP rate-aware pool would bleed.
5. **No upgradability.** A deployed pool cannot be patched. Unchanged from the
   base contracts.
