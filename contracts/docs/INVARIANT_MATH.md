# Orbswap — Invariant Math

> **Source:** Tolstikov, Wentz, Schiarizzi — *Concentrated N-dimensional AMM with
> Polar Coordinates in Rust* (Sept 19, 2025).
>
> This document reconstructs every equation in the paper and pins down the math
> **edge cases** that the `libs/orbswap-math` implementation must respect. Each
> equation was verified numerically against its special cases and against the
> paper's own Rust test vector (see [Verification](#appendix-verification-log)).
>
> **Accuracy contract:** where the paper's PDF text layer is genuinely ambiguous
> (garbled fractions / superscripts), it is flagged with ⚠ **VERIFY** rather than
> guessed. Do not implement a ⚠-flagged relation without confirming it against the
> cited Desmos model first.

---

## 0. The shape ladder

Orbswap is one family of curves controlled by two shape parameters `α`, `β`.
Tuning them walks between three classic AMMs:

| Regime | Reached by | Curve |
|---|---|---|
| **CSMM** (constant sum, `x + y = k`) | `α = β → 2⁺` | straight line |
| **CCMM** (concentrated circular) | `α = β = 2 + √2` | circle (Eq. 1) |
| **LMSR** (log market scoring rule) | `α = β → ∞` | boxy / fat-tailed |

The general curve is the **CSEMM** (concentrated super-elliptical, Eq. 2); CCMM is
just its `u = 2` special case.

---

## 1. CCMM invariant (Eq. 1)

$$(x - k)^2 + (y - k)^2 = k^2$$

A circle **centered at `(k, k)`** with **radius `k`**. `k` is the offset that pins
liquidity. Unlike Uniswap v2's `x·y = k` (which spans price `0 → ∞` with unbounded
reserves), this curve "folds in on itself" and confines reserves to a finite box.

### Price

Implicit differentiation gives the marginal price:

$$p \;=\; -\frac{dy}{dx} \;=\; \frac{x - k}{y - k}$$

### Valid arc and the three landmark points

The tradeable arc is the **lower-left quarter**, from `(0, k)` to `(k, 0)`:

| Point | `x` | `y` | Price `p` |
|---|---|---|---|
| Left edge | `0` | `k` | `+∞` |
| **Balanced (p = 1)** | `k(1 − 1/√2) ≈ 0.292893·k` | same | `1` |
| Right edge | `k` | `0` | `0` |

### ⚠ Edge case — negative-price fold (MUST be disabled for stablecoins)

Solving Eq. 1 for the lower branch (Eq. 5) and continuing past `x = k` re-enters a
region where `(x − k)` and `(y − k)` have **opposite signs**, so price goes
**negative**. This is the red curve in the paper's Fig. 3A.

- Example: `x = 1.5k ⇒ y = 0.1340·k`, price `= −0.5774`.
- **Usable stablecoin domain is `x ∈ [0, k]`, `y ∈ [0, k]`.** Reject or cap any
  swap that would push a reserve past `k`.

---

## 2. Cartesian swap for CCMM (Eqs. 5–6)

### Closed form (Eq. 5) — lower branch of Eq. 1

$$y \;=\; k - \sqrt{2kx - x^2}$$

Derivation: `k² − (x − k)² = 2kx − x²`, so `√(k² − (x − k)²) = √(2kx − x²)`.

### Swap (Eq. 6)

$$\Delta y \;=\; -\sqrt{2k(x - \Delta x) - (x - \Delta x)^2} \;+\; k \;-\; y
\;=\; y_{\text{new}} - y$$

i.e. compute `y_new` from the post-trade reserve `x_new`, then `Δy = y_new − y`.

> **Sign convention.** The paper writes the post-trade reserve as `x − Δx`. Fix one
> convention in code (whether a positive `Δx` adds to or removes from `x`) and test
> the invariant residual after every swap. Do not carry the paper's sign blindly
> into both `x` and `y` update paths.

### ⚠ Edge cases

1. **Radicand non-negativity:** `2kx − x² = x(2k − x) ≥ 0 ⇒ x ∈ [0, 2k]`. Only
   `x ∈ [0, k]` is valid for stablecoins (see §1). `x_new > k` crosses into the
   negative-price fold → reject.
2. **Rounding must favor the pool.** Round `Δy` (out) **down** and any `Δx` (in)
   **up**. This is `isqrt`'s job: `isqrt` truncates toward zero, so wire the
   direction deliberately.
3. **`Δy` cannot exceed `y`** (can't drain more than the reserve). Boundary is
   exactly `x_new = k ⇒ y_new = 0`.

---

## 3. CSEMM invariant (Eq. 2) — the general curve

$$\left(\frac{x}{\alpha} - 1\right)^{u(\alpha)}
\;+\;
\left(\frac{y}{\beta} - 1\right)^{u(\beta)} \;=\; 1$$

with the **exponent function**

$$u(x) \;=\; \frac{\ln 2}{\ln\!\left(\dfrac{x}{x - 1}\right)}$$

`α`, `β` widen/narrow and **skew** the left and right tails independently. Because
`α ≠ β ⇒ u(α) ≠ u(β)`, the curve is **asymmetric** — that asymmetry is the whole
point (capital-efficient skew), and it forces the reversed inverse in §5.

### Behavior of `u(x)`

| `x` (`= α` or `β`) | `x/(x−1)` | `u(x)` | Exponent → shape | Regime |
|---|---|---|---|---|
| `→ 2⁺` | `→ 2` | `→ 1` | linear | **CSMM** `x + y = k` |
| `2 + √2 ≈ 3.41421` | `√2` | **`2`** | quadratic | **CCMM** (Eq. 1) |
| `→ ∞` | `→ 1⁺` | `→ +∞` | boxy | **LMSR**, fat tails |

**Circle recovery (proof the ladder is consistent):** at `α = β = 2 + √2`,
`u = 2`, and Eq. 2 becomes `(x/α − 1)² + (y/β − 1)² = 1`; multiply by `α²` with
`α = β` to get `(x − α)² + (y − α)² = α²`, which is Eq. 1 with `k = α = 2 + √2`.
This is the version launched on Arbitrum (6-dimensional pool).

### ⚠ Edge cases for `u(x)` (critical for `csemm.rs`)

1. **Domain `x > 1` strictly.** At `x = 1`, `x/(x−1) → ∞`, `u → 0` (degenerate
   zero exponent). In practice require **`α, β ≥ 2`**.
2. **`x ∈ (1, 2)` gives `u < 1`** → a concave, astroid/star curve. Almost certainly
   invalid for a pool; guard against it.
3. **`x = 2` gives exactly `u = 1`** (`ln2 / ln2`) — the CSMM boundary.
4. `u` requires a fixed-point `ln`. `ln(x/(x−1))` for `x` near 1 is a **small
   positive** number being divided into `ln 2`, so precision loss there maps to
   large `u` — bound the input range.

---

## 4. Cartesian swap for CSEMM (Eqs. 7–8)

### Closed form (Eq. 7) — solve Eq. 2 for `y`

$$y \;=\; \beta\left[\,1 - \Big(1 - \big(\tfrac{x}{\alpha} - 1\big)^{u(\alpha)}\Big)^{1/u(\beta)}\,\right]$$

Equivalently, in the paper's leading-minus form:
`y = −β[ (1 − (x/α − 1)^{u(α)})^{1/u(β)} − 1 ]`.

> Verified: at `α = β = k`, `u = 2`, this reduces to `y = k − √(2kx − x²)` (Eq. 5)
> to machine precision.

### Swap (Eq. 8)

$$\Delta y \;=\; \beta\left[\,1 - \Big(1 - \big(\tfrac{x + \Delta x}{\alpha} - 1\big)^{u(\alpha)}\Big)^{1/u(\beta)}\,\right] - y \;=\; y_{\text{new}} - y$$

### ⚠ Edge cases

1. **Outer-root base non-negativity:**
   `1 − (x/α − 1)^{u(α)} ≥ 0`. If a trade drives this negative, the fractional root
   `(·)^{1/u(β)}` is undefined (NaN in float, garbage in fixed-point). **This
   expression hitting 0 is the invariant boundary — cap the trade there.**
2. **Negative base to a non-integer power.** When `x < α`, the base `(x/α − 1)` is
   **negative**, and `(negative)^{u(α)}` with non-integer `u(α)` is undefined in
   real arithmetic. Handle the domain/sign consistently with the valid arc (the
   balanced point sits at `x < α`), e.g. via a modeled `|·|`-with-sign convention —
   **decide this explicitly, do not let `pow` see a raw negative base.**
3. **Fixed-point `pow` with non-integer exponent** (`1/u(β)`, `u(α)`): implement as
   `exp(e · ln b)`. Bound `b` and `e`, choose precision so rounding still favors the
   pool. `ln`/`exp`/`pow` here are the `csemm.rs` items flagged "blocked — needs
   design" in the TODO; they are the hardest and least closed-form part.

---

## 5. Inverse swap `Δx` from `Δy` (Eq. 9) — **the asymmetry gotcha**

Because the superellipse is asymmetric, you **cannot** reuse Eq. 8 with the
arguments naively transposed. Solving Eq. 2 for `x` **reverses the `u` order**:
`u(β)` moves inside, `1/u(α)` moves outside, and `α`/`β` swap roles:

$$\Delta x \;=\; \alpha\left[\,1 - \Big(1 - \big(\tfrac{y + \Delta y}{\beta} - 1\big)^{u(\beta)}\Big)^{1/u(\alpha)}\,\right] - x$$

> **Note on the printed Eq. 9.** The paper's typeset Eq. 9 carries a leading `−β`
> and appears to swap an `α`/`β` label in transcription. The **operative rule** it
> states in prose is unambiguous: *"for `Δx`, due to the asymmetric nature of the
> superellipse, our swap function order for `u(x)` is reversed."* The form above is
> the mathematically-derived inverse of Eq. 2 (solve for `x`), and it implements
> exactly that rule. Use this form; treat the printed labels as a typo.

**Edge cases:** identical in spirit to §4 with the roles swapped — outer base
`1 − (y/β − 1)^{u(β)} ≥ 0`, and `(y/β − 1)` may be negative (guard the `pow`).

---

## 6. Polar swap function (Section 2.1, Appendix 3.3)

Instead of moving along the Cartesian curve, Orbswap rotates around a **focal point
at distance `L`**, where **`L` = liquidity**. From the paper's Rust `get_delta_x`:

$$\Delta x \;=\; -\,L\sqrt{1 - \left(\frac{y_{\text{in}}}{L}\right)^2} \;-\; L\cos(135°)$$

- `135° = 180° − 45°`; `cos(135°) = −√2/2 ≈ −0.707107`.
- **Verified against the paper's test vector:** `L = 10`, `y_in = 6.07106781187`
  ⇒ output `= −0.875135 ≈ −0.875` (matches the "Desmos Line 14" comment).

The `45°` tick is where all `n` tokens are priced at `$1.00` (balanced). As a swap
crosses a tick, adjust **`L`** to change price impact (concentration).

### ⚠ Edge cases

1. **Radicand `1 − (y_in / L)² ≥ 0 ⇒ |y_in| ≤ L`.** In the paper's `f64` code
   `y_in > L` yields `NaN`. In fixed-point you must **reject/clamp**, and this
   boundary is the natural **tick-crossing trigger** (the input has consumed the
   whole tick). Carry the remainder into the next tick with its own `L`.
2. **Determinism:** all trig must come from a **compile-time cos/sin lookup table**
   at tick angles, in fixed-point — never runtime floats (see §9, Note 1).

---

## 7. Elliptical skew (Section 2.3, Eq. 3)

The superelliptical skew has **no closed-form polar solution**, so skew is
approximated with a plain ellipse:

$$\left(\frac{x}{a} - L\right)^2 + \left(\frac{y}{b} - L\right)^2 \;=\; L^2$$

- Center `(a·L, b·L)`; extents `x ∈ [0, 2aL]`, `y ∈ [0, 2bL]`. Parameters `a`, `b`
  set the shift/skew intensity.
- **`a = b = 1` recovers the CCMM circle** with `k = L` — use this as a regression
  test for `apply_skew`.
- Empirically (Fig. 1B) stablecoin liquidity belongs **to the left** of price (a
  redemption-fee-barrier asymmetry on one issuer), so skew is capital-efficient.

**Edge case:** keep `a, b > 0`. `a ≠ b` produces the intended asymmetric extents.

---

## 8. Liquidity fingerprints

### 8.1 Unimodal fingerprint (Eq. 4)

Second derivative of reserves w.r.t. `√price`, in tick space `t`:

$$L(t) \;=\; \pm\,\frac{2k\,e^{3t/2}}{\left(1 + e^{2t}\right)^{3/2}}$$

- Bell-shaped, peaked at `t = 0` where `L(0) = 2k / 2^{3/2} = k/√2 ≈ 0.707107·k`
  (verified), decaying to `0` in both tails.
- The `±` reflects the negative-price fold.
- Value function is the Legendre transform; Greeks `Δc, Γc, Θc` are in the paper's
  Desmos [12].

### 8.2 Multimodal fingerprint (Eq. 10)

$$r(\theta) \;=\; \frac{L}{\beta\,\sqrt{\,1 - \tfrac{1}{2}\sin^2(\alpha\theta)\,}}$$

with `α ∈ {4, 6, 8, …}` (intervals of 2).

- `α = 4` ≈ Curve; `α = 6` bimodal; **`α = 8` trimodal** (peaks at `0`, `+1%`,
  `−1%`) — good for CDP stablecoins like DAI (mint/burn produce sinusoidal bumps,
  Fig. 4).
- **Benign domain:** `1 − ½·sin²(αθ) ∈ [½, 1]` (verified), so `r` is bounded in
  `[L/β, √2·L/β]` — no singularities.

> ⚠ **VERIFY — the `β`–`α` relation.** The PDF text layer garbles this as
> `"β = α2"`, which is equally consistent with **`β = α/2`** or **`β = α²`**. The
> two give very different amplitudes (`α = 4 ⇒ β = 2` vs `β = 16`). **Confirm
> against Desmos [14] (`dq1ao5mryr`) before implementing.** Everything else in
> Eq. 10 is confirmed.

---

## 9. Implementation constraints (from the paper) → maps to `orbswap-math`

1. **Note 1 — Determinism.** Use **fixed-point** math everywhere. Floating point is
   non-deterministic across hardware; the `f64` snippet in the paper is
   illustrative only. Drives `fixed_point.rs` (`isqrt`, `FIXED_SCALE`) and the
   fixed-point `ln` / `exp` / `pow` helpers.
2. **Note 2 — Scaling.** Unscaled decimals in the sample must be scaled (WAD-style,
   like prb-math) for integer VMs. On Soroban, pick a single `FIXED_SCALE` and
   thread it through every operation.

---

## 10. Edge-case checklist (the ones that actually bite)

1. **CCMM radicand** `x(2k − x) ≥ 0` → domain `[0, 2k]`, usable `[0, k]`; reject
   negative-price crossings past `x = k`.
2. **CSEMM outer-root base** `1 − (x/α − 1)^{u(α)} ≥ 0` → invariant boundary; cap
   the trade there.
3. **Negative base to non-integer power** when `x < α` — never hand `pow` a raw
   negative base; decide the sign convention explicitly.
4. **`u(x)` domain** `x > 1` (use `α, β ≥ 2`); `x = 1` blows up; `x ∈ (1, 2)` gives
   concave `u < 1`.
5. **Polar radicand** `|y_in| ≤ L` → NaN guard = tick-crossing trigger; carry the
   remainder into the next tick's `L`.
6. **`Δx` vs `Δy` asymmetry** (Eq. 9): the `u` order reverses — do not reuse the
   `Δy` formula transposed.
7. **Rounding direction** always toward the pool; fuzz the invariant residual after
   every swap (`fuzz_targets/swap_invariant.rs`).
8. **Skew degenerate check** `a = b = 1` must equal the circle (`k = L`) — a
   regression test for `apply_skew`.
9. **`β`–`α` in Eq. 10** is ⚠ unresolved from the source; confirm before coding.

---

## Appendix — Verification log

All checks below were run in Python (`float64`) and pass; reproduce them before
trusting any refactor of the fixed-point ports.

| Check | Expected | Result |
|---|---|---|
| `u(2)` | `1` | `1.000000` ✓ |
| `u(2 + √2)` | `2` | `2.000000` ✓ |
| `u(10)`, `u(100)` | `> 2`, growing | `6.578813`, `68.967564` ✓ |
| Eq. 7 vs Eq. 5 at `α = β = 2+√2` | identical | diff `≤ 6.7e-16` ✓ |
| Polar `get_delta_x` (`L=10`, `y_in=6.07106781187`) | `−0.875` | `−0.875135` ✓ |
| Negative-price at `x = 1.5k` | `p < 0` | `y/k = 0.1340`, `p = −0.5774` ✓ |
| Balanced point `x/k` | `1 − 1/√2` | `0.292893`, `p = 1` ✓ |
| Fingerprint peak `L(0)/k` | `1/√2` | `0.707107` ✓ |
| Multimodal denom `1 − ½sin²` range | `[0.5, 1.0]` | `[0.5, 1.0]` ✓ |

> Two source ambiguities remain flagged and are **not** treated as settled:
> the printed Eq. 9 labels (§5) and the Eq. 10 `β`–`α` relation (§8.2). The Eq. 10
> Desmos model (`dq1ao5mryr`) is a JS app and must be checked manually in a browser
> (verified unfetchable 2026-07-11).

## Appendix — Reference-implementation caveat (researched 2026-07-11)

The paper's github reference [7] (Orbswap V0, deployed on Arbitrum at
`0x22f8…ae44`, files `OrbPool.sol` + `RootLib.sol`) was inspected and is
**prototype-grade — do not treat it as a math spec**:

1. `RootLib.nRoot(x, n) = x ** (1/n)` — Solidity integer division makes
   `1/n = 0` for `n ≥ 2`, so every root evaluates to `x⁰ = 1`. Broken.
2. The swap prices via a **linearized average execution price**
   `amountOut = amountIn · (I(x+Δx) − I(x))/Δx` instead of solving the invariant
   exactly (Eqs. 6/8), and it **mutates `L` inside `swap`** — swaps must move
   reserves along the curve; only deposits/withdrawals change the liquidity scale.
3. Useful corroborations only: WAD `1e18` fixed-point scale; LP shares minted
   ≈ ΔL (matches the shares ∝ liquidity-scale model).

For the N-dimensional generalization, anchor on **Orbital** [5] instead: sphere
invariant `Σ(xᵢ − r)² = r²`, ticks as planes normal to the equal-price vector
(`r·1⃗ = c`, spherical caps), boundary detection via the normalized projection
`r·1⃗/R`, trade segmentation by solving for the boundary crossing, and
interior/boundary tick consolidation (torus invariant). In `n = 2` an Orbital
"cap" is exactly an angle range around 45° — i.e. this paper's polar ticks.
