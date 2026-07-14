# Tick design — concentrated liquidity for the 2-token Circular (CCMM) pool

> Scope: **`PoolMode::Circular`, exactly 2 tokens**. SuperElliptical pools stay
> single-range (no polar form → no ticks; paper §2.3). This spec is the contract-
> level model that composes the already-tested `orbswap-math` primitives
> (`polar.rs`, `ticks.rs`) into a Uniswap-v3-style concentrated-liquidity engine in
> **angle space**. Nothing here changes SuperElliptical behavior.

## 1. Geometry — the arc and its coordinates

The CCMM circle is centered at `(k, k)` with radius `k`; the tradeable arc is the
lower-left quarter, parameterized by a **tick angle `θ ∈ [0°, 90°]`**:

```
x(θ) = k · (1 − cos θ)      y(θ) = k · (1 − sin θ)
```

| θ | x | y | price p = (x−k)/(y−k) | meaning |
|---|---|---|---|---|
| 0° | 0 | k | +∞ | all Y |
| 45° | 0.2929·k | 0.2929·k | 1 | **balanced ($1 = $1)** |
| 90° | k | 0 | 0 | all X |

`Direction::Up` = θ increasing (pool gains X). `Down` = θ decreasing (pool gains Y).
All trig comes from the compile-time WAD `cos_deg`/`sin_deg` table (`polar.rs`) — no
runtime floats, ever.

## 2. Liquidity `L` and positions

A **position** is `L` units of liquidity concentrated over `[θ_lower, θ_upper]`
(integer degrees, `0 ≤ θ_lower < θ_upper ≤ 90`). `L` plays the role of the circle's
`k` **for that position only**, active exactly while the pool's current angle `θc`
lies in the position's range.

**Deposit amounts** (derived from the arc integrals `x(θ)`, `y(θ)`), at current
angle `θc`:

```
if θc ≤ θ_lower:   all Y →  Y = L·(sin θ_lower − sin θ_upper)·(−1)= L·(sin θ_upper' ...)   (see below)
if θc ≥ θ_upper:   all X
else (in range):   X = L·(cos θ_lower − cos θc)
                   Y = L·(sin θ_upper − sin θc)
```

Canonical (in-range) case — the ratio `X:Y = (cos θ_lower − cos θc) : (sin θ_upper −
sin θc)` is **fixed** by the tick choice, so a deposit must match it. Given user
`(x_in, y_in)`, compute `L = min( x_in/(cos θ_lower − cos θc), y_in/(sin θ_upper −
sin θc) )` (round **down** → pool-favoring), pull exactly `L·(…)` of each, refund the
dust. Out-of-range positions are single-sided (one amount is 0).

Verification: full range `[0,90]`, `θc = 45°` → `X = L(cos0 − cos45) = 0.2929 L`,
`Y = L(sin90 − sin45) = 0.2929 L` — balanced, and equals `x(45)` with `L = k`. ✓

## 3. Storage (Circular pools only)

Added to instance storage, all gated behind `mode == Circular`:

- `TickAngle` — current pool angle `θc` (i128 WAD-free integer degrees × RES; see §7).
- `ActiveLiquidity` — `L` summed over positions spanning `θc` (i128).
- `TickNet(angle)` — per initialized tick, the `liquidity_net` applied on an upward
  cross (`+L` at a position's lower tick, `−L` at its upper). Persistent map.
- `TickBitmap` — `u128`, one bit per integer degree `0..90` (`flip_tick`).
- `FeeGrowthGlobal[2]` — cumulative fee-per-unit-liquidity, per token (i128, WAD).
- `TickFeeOutside(angle)[2]` — v3 fee-growth-outside bookkeeping per tick.
- `Position(owner, lower, upper)` — `{ liquidity, fee_growth_inside_last[2] }`
  (persistent). Positions are **not** fungible LP shares; each range is its own key.

`total_shares`/`S` remain the pool-wide accounting for **withdraw-all** and the
`MINIMUM_LIQUIDITY` lock; per-position `L` is the concentrated overlay.

## 4. Swap — tick walking

Uses `ticks::segment_swap` + `polar::get_delta_x/у` + `ticks::cross_tick`:

```
remaining = net_input (after fee)
loop:
  θ_next = next_initialized_tick(bitmap, θc, direction)      # edge of current tick
  to_boundary = input that moves θc → θ_next at current L    # via polar radicand edge
  seg = segment_swap(remaining, to_boundary)
  out += fill segment inside [θc, θ_next] at active L (polar get_delta)
  accrue fee-growth for this segment to FeeGrowthGlobal (÷ active L)
  remaining = seg.carry
  if !seg.reached_boundary: break                            # filled inside the tick
  cross θ_next: ActiveLiquidity = cross_tick(L, TickNet(θ_next), direction)
                flip TickFeeOutside(θ_next); advance θc = θ_next
  if remaining == 0 or no next tick: break
```

Post-swap: assert the segment math kept reserves on-arc (mirror of the
SuperElliptical `invariant_holds` guard). `S`/`total_shares` **never** change in a
swap. Emit `TickCrossed` per boundary.

Radicand `|input| ≤ L` (`polar::TickBoundary`) is the natural crossing trigger.

## 5. Withdraw

Burn `L` (or a fraction) from `Position(owner, lower, upper)`:
- Collect owed fees first: `owed_i = L · (FeeGrowthInside_i − fee_growth_inside_last_i)`.
- Return tokens = the position's current `X`/`Y` at `θc` (§2 formula with the
  removed `L`), rounded **down**.
- Update `TickNet(lower) −= L`, `TickNet(upper) += L`; if `θc ∈ range`, `ActiveLiquidity −= L`.

## 6. Fees per tick (v3 model)

`fee_growth_inside_i = FeeGrowthGlobal_i − below(lower) − above(upper)` using
`TickFeeOutside`. Fees accrue **outside the curve** (consistent with the pool's
existing fees-outside-curve model) and are paid on withdraw/collect. Rounding: fee
up, owed down.

## 7. Fixed-point & rounding

- Angles are **integer degrees** `0..90` at storage level; sub-degree precision for
  `θc` uses a resolution multiplier `RES` (e.g. 1e6) so `θc` moves continuously
  within a degree while ticks sit on integer degrees (`bitmap` is per-degree).
- Every rounding favors the pool: `L` down, output down, input up, fee up, owed down.
- Reuse the `md`/`Rounding` helpers already in `lib.rs`.

## 8. Backward compatibility

- SuperElliptical + N-token: **unchanged** (this whole subsystem is `Circular`-gated).
- A Circular pool with a **single full-range `[0,90]` position** must reproduce the
  current single-range `ccmm` behavior exactly (golden regression test).
- Existing Circular tests (init/deposit/swap) migrate to "deposit full-range then
  swap"; results must match the pre-tick numbers.

## 9. Open items / risks (resolve during build, gate on tests)

1. **`L ↔ S` reconciliation** for `MINIMUM_LIQUIDITY` and `total_shares` on the first
   full-range deposit — first deposit locks the minimum as today; concentrated
   positions add `L` without minting fungible shares.
2. **Multi-tick rounding drift** — each crossing rounds; the sum of segment outputs
   must be ≤ the single-shot output (no value creation). Property test ×10k.
3. **Position value conservation** — deposit-then-immediate-withdraw returns ≤
   deposited; swap-through-a-range then reverse can't profit. Adversarial tests.
4. **`to_boundary` computation** — the input that exactly reaches `θ_next` at active
   `L`, pool-favoring, via the polar radicand. Needs a tight integer derivation.
5. **⚠ Swap needs WIDE-PRECISION circle math (found during M5).** The exact circle
   swap is `new_y = L − √(new_x·(2L − new_x))` (`ccmm::swap_out`). That radicand is
   `≈ L²`, so `ccmm` overflows `i128` for `L ≳ 1e19` — and concentrated positions
   push `L` far higher. Working around it in **cos/sin space** (`isqrt` of
   `WAD² − c²`) avoids the overflow BUT floors the price move, which under-moves the
   price and lets a **reverse trade recover more than it paid** (measured: +4e6 on a
   1e21 round trip — tiny but a real value leak). Verdict: the swap step must compute
   the radicand `new_x·(2L − new_x)` in **256-bit** (the lib already has `wide_mul`)
   and take a **wide `isqrt`**, rounding the paired reserve UP like `ccmm` does, so
   it is exact + strictly pool-favoring at any `L`. **Gate M5 on a
   round-trip-no-profit fuzz target (×10k) before wiring the contract swap.** Until
   then, `circle_liq` ships deposit/withdraw only (no swap step).

## 10. Build milestones (each gated by its tests before the next)

1. Types + storage (`TickState`, `Position`, bitmap, fee-growth) — no behavior change.
2. `deposit` into a range (single tick first, then multi) + `withdraw` — conservation tests.
3. Full-range regression: `[0,90]` position ≡ current `ccmm` swap (golden).
4. Tick-walking `swap` (single tick → multi-tick) — segment/conservation/boundary tests.
5. Per-tick fees + `collect` — fee-growth correctness tests.
6. `TickCrossed` events + tick view methods (`current_tick`, `tick_liquidity`, positions).
7. Full gate, then deploy + frontend tick UI.
