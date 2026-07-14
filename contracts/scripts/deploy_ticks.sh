#!/usr/bin/env bash
# Deploy a 2-token **Circular concentrated-liquidity (tick) pool** on testnet:
# enable ticks → seed a full-range position → add a concentrated position → swap.
# Writes deployments/<network>_ticks.json.
#
# Prereqs: stellar-cli 27+, rust 1.91 + wasm32v1-none. Network access.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-orbswap}"
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"
FEE_BPS="${POOL_FEE_BPS:-30}"
A_CODE="${TICK_TOKEN_A:-CIRA}"
B_CODE="${TICK_TOKEN_B:-CIRB}"
TWO_PLUS_SQRT2=3414213562373095049
M=10000000
MAXU64=18446744073709551615
mkdir -p deployments
OUT=deployments/${NET}_ticks.json

say() { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }
inv() { stellar contract invoke --id "$1" --source "$2" --network "$NET" -- "${@:3}"; }

ensure_key() {
  stellar keys address "$1" >/dev/null 2>&1 || stellar keys generate "$1" --network "$NET" --fund >/dev/null
  stellar keys fund "$1" --network "$NET" >/dev/null 2>&1 || true
  stellar keys address "$1"
}

say "Identities"
DEPLOYER=$(ensure_key "$IDENT");  echo "deployer: $DEPLOYER"
ISSUER_ADDR=$(ensure_key "$ISSUER"); echo "issuer:   $ISSUER_ADDR"

say "Build + optimize pool wasm"
cargo build --target wasm32v1-none --release -p orbswap-pool
stellar contract optimize --wasm target/wasm32v1-none/release/orbswap_pool.wasm
WASM=target/wasm32v1-none/release/orbswap_pool.optimized.wasm

say "Test tokens ($A_CODE, $B_CODE)"
deploy_token() {
  stellar contract asset deploy --asset "$1:${ISSUER_ADDR}" --source "$ISSUER" --network "$NET" >/dev/null 2>&1 || true
  stellar contract id asset --asset "$1:${ISSUER_ADDR}" --network "$NET"
}
TA=$(deploy_token "$A_CODE"); TB=$(deploy_token "$B_CODE")
echo "  $A_CODE=$TA"; echo "  $B_CODE=$TB"
MINT=$(( 5000000 * M )) # 5,000,000 of each to the deployer (the LP)
for CODE in "$A_CODE" "$B_CODE"; do
  stellar tx new change-trust --source "$IDENT" --network "$NET" --line "${CODE}:${ISSUER_ADDR}" >/dev/null
done
inv "$TA" "$ISSUER" mint --to "$DEPLOYER" --amount "$MINT" >/dev/null
inv "$TB" "$ISSUER" mint --to "$DEPLOYER" --amount "$MINT" >/dev/null

say "Deploy + initialize pool (Circular, $FEE_BPS bps)"
POOL=$(stellar contract deploy --wasm "$WASM" --source "$IDENT" --network "$NET")
echo "pool: $POOL"
inv "$POOL" "$IDENT" initialize \
  --tokens "[\"$TA\",\"$TB\"]" --mode Circular \
  --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" \
  --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >/dev/null

say "Enable concentrated-liquidity ticks"
inv "$POOL" "$IDENT" enable_ticks >/dev/null

say "Seed full-range position: 1,000,000 of each"
FR=$(( 1000000 * M ))
inv "$POOL" "$IDENT" add_liquidity --from "$DEPLOYER" \
  --amounts "[\"$FR\",\"$FR\"]" --lower 0 --upper 90 --min_liquidity 0 --deadline "$MAXU64"

say "Add a concentrated position around balance [40, 50]"
CC=$(( 500000 * M ))
inv "$POOL" "$IDENT" add_liquidity --from "$DEPLOYER" \
  --amounts "[\"$CC\",\"$CC\"]" --lower 40 --upper 50 --min_liquidity 0 --deadline "$MAXU64"

say "Swap 1,000 $A_CODE -> $B_CODE"
inv "$POOL" "$IDENT" swap --from "$DEPLOYER" --token_in "$TA" --amount_in $(( 1000 * M )) \
  --token_out "$TB" --min_out 0 --deadline "$MAXU64"

say "Pool tick state"
echo "current_tick:     $(inv "$POOL" "$IDENT" current_tick)"
echo "active_liquidity: $(inv "$POOL" "$IDENT" active_liquidity)"
echo "reserves:         $(inv "$POOL" "$IDENT" get_reserves)"

cat > "$OUT" <<JSON
{
  "network": "$NET",
  "mode": "Circular-ticks",
  "deployer": "$DEPLOYER",
  "issuer": "$ISSUER_ADDR",
  "pool": "$POOL",
  "fee_bps": $FEE_BPS,
  "tokens": { "$A_CODE": "$TA", "$B_CODE": "$TB" }
}
JSON
say "Wrote $OUT"; cat "$OUT"
