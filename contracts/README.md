# Orbswap — contracts

A concentrated **N-dimensional AMM** on Stellar/Soroban, implementing
*Concentrated N-dimensional AMM with Polar Coordinates in Rust* (Tolstikov, Wentz, Schiarizzi,
2025). Pools trade on the Orbswap invariant — a **circle** (CCMM) or a **superellipse** (CSEMM) —
for stable-swap-style concentrated liquidity around the \$1 balanced point.

The whole invariant lives in one pure-Rust library ([`orbswap-math`](orbswap-math/src)); the pool
contract ([`orbswap-pool`](orbswap-pool/src)) links it in and adds custody, fees, and LP
accounting on top. For the project overview and the app, see the [root README](../readme.md).

> **Status: testnet-ready.** math → pool → factory → router complete — **215 tests, clippy-clean,
> fuzzed, all 3 contracts build & optimize to wasm** (pool 57 KB · factory 12 KB · router 13 KB).
> Deployable today as a fixed, non-upgradeable pool; see [Status](#status) for the gap to mainnet.

---

## How it works

The math is a self-contained library, so the invariant is testable in isolation and links straight
into the pool — no cross-contract calls, all deterministic integer math.

- **Invariant.** For two tokens the curve is the circle `(x−k)² + (y−k)² = k²` (`k = S·(2+√2)`);
  the superellipse generalizes it to a tunable shape and to `n ≤ 8` assets. Reserves run in
  normalized space `x̂ = internal / S ∈ [0, α]`, so the **liquidity scale `S`** (= `total_shares`)
  is a separate axis from the **shape `α, β`** (fixed at init).
  ([`ccmm`](orbswap-math/src/ccmm.rs) · [`csemm`](orbswap-math/src/csemm.rs) · [`ndim`](orbswap-math/src/ndim.rs))
- **Polar swap & ticks.** On the circle a swap rotates in polar coordinates from a focal point at
  distance `L` (liquidity) — one step along the arc, solved on virtual reserves with a 256-bit
  `isqrt_wide` (the ≈`L²` radicand overflows an `i128` otherwise). Concentrated liquidity is
  Uniswap-v3-style ticks in **angle space** (integer degrees 0–90, 45° = the peg): per-position
  `L` + `feeGrowthInside`, a `u128` tick bitmap, and `cross_tick` as price walks the circle.
  ([`polar`](orbswap-math/src/polar.rs) · [`ticks`](orbswap-math/src/ticks.rs) · [`circle_liq`](orbswap-math/src/circle_liq.rs) · [docs/TICK_DESIGN.md](docs/TICK_DESIGN.md))
- **Fees outside the curve.** Only the *net* input moves along the invariant, so pricing matches
  the paper exactly; the LP cut accrues to a pot paid pro-rata on withdraw and the protocol cut to
  `ProtocolOwed`. Every pool holds the solvency invariant
  `balance == reserves + ProtocolOwed + LpFeesOwed`, exact to the integer. ([`fees`](orbswap-math/src/fees.rs))
- **Skew & multimodal.** An elliptical form skews concentration per-axis — the paper's fix for
  Orbital's symmetric ticks — and the fingerprint module carries the multimodal distributions.
  ([`skew`](orbswap-math/src/skew.rs) · [`fingerprint`](orbswap-math/src/fingerprint.rs))
- **Determinism & safety.** WAD fixed point, no floats; rounding **always favors the pool**; a
  post-swap invariant check and a minimum-trade guard protect CSEMM precision near the asymptote.
  Fuzzed with 5 cargo-fuzz targets. Full derivation: [docs/INVARIANT_MATH.md](docs/INVARIANT_MATH.md).

---

## Crates

| Crate | What it is |
|-------|-----------|
| [`orbswap-math`](orbswap-math/src) | Pure `#![no_std]`, zero-dep, no-float fixed-point (WAD) math: `ccmm`, `csemm`, `ndim`, `polar`, `ticks`, `circle_liq`, `skew`, `fees`, `oracle`, `fingerprint`. Fuzzed. |
| [`orbswap-pool`](orbswap-pool/src) | The pool: liquidity, swaps, quotes, fees (outside the curve), protocol fee, oracle, pause, depeg block, LP-share transfer. 2-token and N-token (n ≤ 8); `Circular` or `SuperElliptical`. |
| [`orbswap-pool-interface`](orbswap-pool-interface/src) | Shared types + `#[contractclient]` so factory/router call a pool without linking its wasm symbols. |
| [`orbswap-factory`](orbswap-factory/src) | `create_pool` (deploys + initializes), `sha256(PoolKey)` registry, canonical token ordering, dedup. |
| [`orbswap-router`](orbswap-router/src) | Stateless multi-hop `swap_exact_in`/`swap_exact_out`, `quote_path`, add/remove-liquidity passthroughs. No custody. |

**Optimized wasm:** pool 57 KB · factory 12 KB · router 13 KB (well under Soroban limits).

---

## Pool entrypoints

```rust
// Liquidity — fungible shares (SuperElliptical) or concentrated positions (Circular)
deposit(from, amounts: Vec<i128>, min_shares, deadline) -> shares
withdraw(from, shares, min_amounts: Vec<i128>, deadline) -> Vec<i128>
enable_ticks()                                                     // Circular only · admin · pre-liquidity
add_liquidity(from, [x_max, y_max], lower, upper, min_liquidity, deadline) -> L
remove_liquidity(from, lower, upper, liquidity, min_amounts, deadline) -> Vec<i128>

// Swaps + quotes
swap(from, token_in, amount_in, token_out, min_out, deadline) -> amount_out
swap_exact_out(from, token_in, token_out, amount_out, max_in, deadline) -> amount_in
quote(token_in, amount_in, token_out) -> amount_out               // read-only
quote_exact_out(token_in, token_out, amount_out) -> amount_in     // read-only

// Views
get_reserves() · total_shares() · shares_of(who) · get_spot_price() · price_cumulative()
lp_fees_owed() · protocol_owed() · tick_mode() · active_liquidity() · current_tick()
position_liquidity(owner, lower, upper)

// Admin (one `admin`, set at init)
pause_deposits/pause_swaps/pause_withdrawals/pause_all(bool)
set_protocol_fee_bps(bps) · collect_protocol_fees(to) · set_allowed(token, bool) · transfer_shares(from, to, amount)
```

Every mutating call starts with `from.require_auth()` — the caller's single signature authorizes
the call *and* its token pulls. `deposit`/`withdraw` are the fungible-share path; a `Circular` pool
switches to the concentrated path with `enable_ticks`, then `add_liquidity`/`remove_liquidity`.

---

## Status

Testnet-ready, **not** mainnet-hardened.

**Done**
- [x] CCMM + CSEMM invariant, N-token (n ≤ 8), normalized `S`-scaling
- [x] `deposit` / `withdraw` fungible LP shares, `MINIMUM_LIQUIDITY` lock
- [x] Polar concentrated-liquidity **tick pool** — `add_liquidity` / `remove_liquidity`, angle-space ticks, per-position fees, tick-walk swap (**live on testnet**)
- [x] `swap` + `swap_exact_out` + quotes; fees held outside the curve; protocol fee
- [x] Spot + cumulative price oracle (off-chain TWAP)
- [x] Pausability, depeg block (`set_allowed`), LP `transfer_shares`
- [x] Factory (`create_pool`, sha256 registry, canonical ordering, dedup) + stateless multi-hop router
- [x] Fuzzed math (5 targets) + adversarial probes; 215 tests; clippy-clean

**Deferred to mainnet**
- [ ] Upgradability — no `upgrade` entrypoint; a deployed pool can't be patched
- [ ] Real access control — single `admin`, no roles or two-step ownership transfer
- [ ] N-dimensional polar ticks — the 2-token circle has ticks; N-token concentration is by curve shape only
- [ ] Depeg rebalance — only the block-flag half exists; no renormalization over surviving tokens after an eject
- [ ] External audit + on-contract fuzzing

---

## Build & test

```bash
# Prereqs: Rust 1.91 + `wasm32v1-none` target, stellar-cli 27+, Task (optional).

task gate          # full gate: test + clippy + fmt + wasm build (or the raw commands below)

cargo test                                              # 215 tests
cargo clippy --all-targets                              # zero warnings
cargo fmt --all -- --check
cargo build --target wasm32v1-none --release \
  -p orbswap-pool -p orbswap-factory -p orbswap-router
```

Fuzz the math lib (nightly): `cd orbswap-math && cargo +nightly fuzz run swap_invariant -- -max_total_time=30`.

---

## Deploy to testnet

All scripts are self-contained: they create + friendbot-fund the identities they need, build &
optimize the wasm, issue test tokens, deploy, and write the addresses to `deployments/<network>*.json`.

```bash
cp .env.example .env             # optional — every value has a default
bash scripts/deploy_testnet.sh   # one N-token pool (default 4) + tokens + a swap
bash scripts/deploy_all.sh       # pool wasm → factory → router → 2-token pool → router swap
bash scripts/seed_testnet.sh     # fresh 4-token pool seeded to 24M by 3 LP accounts
bash scripts/deploy_ticks.sh     # 2-token Circular tick pool + concentrated positions
```

Config (all optional, via `.env`): `STELLAR_NETWORK`, `DEPLOYER_IDENTITY`, `POOL_FEE_BPS`,
`TOKEN_CODES` (space-separated, 2..8 → pool size). To deploy with your own key, add it to the
stellar-cli keystore (`stellar keys add …`) and set `DEPLOYER_IDENTITY` — the secret never touches
`.env`. `deploy_all.sh` uses the factory, which is **2-token only**; 4-token pools are deployed
directly by the other scripts. Full guide: [docs/DEPLOY.md](docs/DEPLOY.md). Deployed addresses
land in [`deployments/`](deployments/) (git-tracked, shareable).

---

## Live deployments (Stellar testnet)

Network passphrase: `Test SDF Network ; September 2015`.

**Seeded 4-token pool** — 24M total liquidity (6M per asset), 30 bps, SuperElliptical. Seeded by
three LP accounts (Alice 3M, Bob 2M, Carol 1M of each). Source of truth:
[`deployments/testnet_seeded.json`](deployments/testnet_seeded.json).

| Contract | Address |
|---|---|
| **Pool** | [`CBMYB2V3U4IMQBNRGSSE2B7646YG756KJONZPAKAAJYFQ7L6OJGDNDLW`](https://stellar.expert/explorer/testnet/contract/CBMYB2V3U4IMQBNRGSSE2B7646YG756KJONZPAKAAJYFQ7L6OJGDNDLW) |
| USDA | `CBIMNDUMDFBE22ZLLGRLY46J2E4GTFGHOCA2KVE75HLZGJQBELEV4EPL` |
| USDB | `CD3SALJPZFKLBE5RBBLV2DDSBVUIDQRHMADKJBRY2VSVBL3KITUWO5JA` |
| USDC | `CBNDCO3DMKFVCSVFPHMYK6KSD6CCKVUMI3TFK6ZJ3BP7NCNLUJBJAB6Z` |
| USDD | `CDJVB5YTBHBNIZNVRKE7VYKQ6OHWEDDEYKO7U4DSEHPRGPERGEMTTPCB` |

Accounts:

| Role | Address |
|---|---|
| Deployer / admin | `GDLAEZGPYY6QDVHIEFWME3UFG6475EOADUDZ4MDEEHBEK6GOLDGIEX3O` |
| Token issuer | `GCASMKFHTQRAAMDGB4IMS3ZXI2FZR7G4XS74MVTBR5D2UADD63ALRN4U` |
| LP — Alice (3M each) | `GBELZVUNGL2GUFABOOBVIBUXWVMI6QEYJIPOYFO4B7H4LGJLBOZ6GUTK` |
| LP — Bob (2M each) | `GCBT2NTPUZGL5G5KPRFBXB5A5POP55LT76T4QDXM2XXK2UKU6J7ATSM2` |
| LP — Carol (1M each) | `GC5SCW2NJEBOFOO4LHCJEMBGTSQ5DRWG6HYGWDEWVXN7HMHUTPLFN5CU` |

**Circular concentrated-liquidity (tick) pool** — 2-token CCMM pool with Uniswap-v3-style polar
ticks. Full-range + concentrated `[40,50]` positions, tick-walk swap, per-tick fees. Deployed by
[`deploy_ticks.sh`](scripts/deploy_ticks.sh); [`deployments/testnet_ticks.json`](deployments/testnet_ticks.json).

| Contract | Address |
|---|---|
| **Tick pool** | [`CCAZ3IADGGP4K5NRWMM5RCA63J76SHDITSY6HJLCUEXGAKUFAMEWC2NL`](https://stellar.expert/explorer/testnet/contract/CCAZ3IADGGP4K5NRWMM5RCA63J76SHDITSY6HJLCUEXGAKUFAMEWC2NL) |
| CIRA | `CAL5IWELZEBZ3V7JT5W3CS2RABEWHBSJCJ6QBSZYGSZ33SJ3OTS3EXRV` |
| CIRB | `CCPYE62VMOIQIOMANUHLD5YWZCPJ5P7G6XDPSAIRP4QZNJE5AUDIC5NG` |

**Periphery (factory + router)** — the factory (`create_pool`) stamps out new 2-token pools; the
stateless router does multi-hop swaps and never custodies tokens.
[`deployments/testnet_full.json`](deployments/testnet_full.json).

| Contract | Address |
|---|---|
| **Factory** | [`CC7J3JNSBILDA264Y3YKFQUQ6KAEIICPTENS2FN3O7BLYSFDCKVYDGEN`](https://stellar.expert/explorer/testnet/contract/CC7J3JNSBILDA264Y3YKFQUQ6KAEIICPTENS2FN3O7BLYSFDCKVYDGEN) |
| **Router** | [`CAV7RWVFGHLKH64R7IGKP5HCQ57SM5WTX2CDNTWBXW5C2S4346YIZUVW`](https://stellar.expert/explorer/testnet/contract/CAV7RWVFGHLKH64R7IGKP5HCQ57SM5WTX2CDNTWBXW5C2S4346YIZUVW) |

A **smoke-test pool** (`CB4LRV7NFOIOHSYGHZBIXYLRAHVSXPZ5AZPIIRO4MKC4GUWDX7H73QXT`) from the initial
`deploy_testnet.sh` run also exists in [`deployments/testnet.json`](deployments/testnet.json).

> Testnet contracts are non-upgradeable and may be reset; treat these as demo addresses.
> Re-running the scripts regenerates the JSON records above.

---

## License

MIT.
