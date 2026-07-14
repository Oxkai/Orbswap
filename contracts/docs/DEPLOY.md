# Deploying & testing Orbswap

## Status: the pool is testnet-ready

The `orbswap-pool` contract compiles to a **33 KB** wasm, has **31 passing tests**,
and implements the full swap lifecycle (initialize / deposit / swap / withdraw),
fees, events, pausability, and a price oracle. You can deploy it to Stellar testnet
today and trade against it.

What is **not** deployed yet (Phase 3+): the **factory** (one-click pool creation)
and the **router** (multi-hop). For a single-pool test, neither is needed.

## Prerequisites

- `stellar-cli` 27+ (`stellar --version`)
- Rust 1.91 + the `wasm32v1-none` target (pinned via `rust-toolchain.toml`)
- Network access to testnet

## One-command demo

```bash
bash scripts/deploy_testnet.sh
```

This will: create/fund an `orbswap` testnet identity → build + optimize the wasm →
deploy two test tokens (USDA, USDB) and mint 1000 of each → deploy the pool →
`initialize` it as a **Circular** 30-bps pool → deposit 100+100 → quote + execute a
10-USDA→USDB swap → print reserves → write all addresses to
`deployments/testnet.json`.

## Manual steps (what the script automates)

```bash
# 1. identity + funding
stellar keys generate --global orbswap --network testnet --fund
DEPLOYER=$(stellar keys address orbswap)

# 2. build + optimize
cargo build --target wasm32v1-none --release -p orbswap-pool
stellar contract optimize --wasm target/wasm32v1-none/release/orbswap_pool.wasm

# 3. deploy
POOL=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/orbswap_pool.optimized.wasm \
  --source orbswap --network testnet)

# 4. test tokens (issuer = deployer = SAC admin, so it can mint)
stellar contract asset deploy --asset "USDA:$DEPLOYER" --source orbswap --network testnet
USDA=$(stellar contract id asset --asset "USDA:$DEPLOYER" --network testnet)
stellar contract invoke --id "$USDA" --source orbswap --network testnet \
  -- mint --to "$DEPLOYER" --amount 10000000000
# ...repeat for USDB...

# 5. initialize (Circular: alpha=beta=2+√2 in WAD)
stellar contract invoke --id "$POOL" --source orbswap --network testnet -- \
  initialize --tokens "[\"$USDA\",\"$USDB\"]" --mode Circular \
  --alpha 3414213562373095049 --beta 3414213562373095049 \
  --fee_bps 30 --admin "$DEPLOYER"

# 6. deposit / swap (7-decimal tokens: 1000000000 = 100 units)
stellar contract invoke --id "$POOL" --source orbswap --network testnet -- \
  deposit --from "$DEPLOYER" --amounts '[1000000000,1000000000]' \
  --min_shares 0 --deadline 18446744073709551615

stellar contract invoke --id "$POOL" --source orbswap --network testnet -- \
  swap --from "$DEPLOYER" --token_in "$USDA" --amount_in 100000000 \
  --token_out "$USDB" --min_out 0 --deadline 18446744073709551615
```

## Contract interface (current)

| Function | Notes |
|---|---|
| `initialize(tokens, mode, alpha, beta, fee_bps, admin)` | one-time; `Circular` needs α=β=2+√2 (`3414213562373095049`) |
| `deposit(from, amounts, min_shares, deadline) -> shares` | first deposit must be **balanced**; later ones **proportional** |
| `withdraw(from, shares, min_amounts, deadline) -> amounts` | proportional; locked minimum stays |
| `swap(from, token_in, amount_in, token_out, min_out, deadline) -> out` | fee taken from input |
| `quote(token_in, amount_in, token_out) -> out` | view |
| `get_reserves` / `get_config` / `total_shares` / `shares_of` / `get_liquidity_scale` | views |
| `get_spot_price` / `price_cumulative` | oracle (TWAP = Δcum/Δt off-chain) |
| `pause_deposits/swaps/withdrawals/all(bool)` / `paused()` | admin-gated |

## Notes / gotchas

- **SuperElliptical pools**: pass `--mode SuperElliptical --alpha <WAD> --beta <WAD>`
  with α, β ≥ 2·1e18. They enforce a min-trade size + post-swap invariant check.
- **Decimals**: the pool reads each token's `decimals()` at init and normalizes to
  18 internally. Stellar Asset Contracts are 7-decimal.
- **Amounts** in CLI are native base units (7-dec token → multiply display units by 1e7).
- The exact `stellar contract asset` / enum-arg syntax can vary slightly by CLI
  version — if a command errors, check `stellar contract <cmd> --help`.
