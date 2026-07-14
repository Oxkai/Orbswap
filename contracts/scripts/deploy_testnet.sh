#!/usr/bin/env bash
# Deploy an Orbswap N-token pool (default: 4-token SuperElliptical) + test tokens to
# Stellar testnet, then run a smoke swap. Writes addresses to deployments/<network>.json.
#
# Prereqs: stellar-cli 27+, rust 1.91 + wasm32v1-none target. Network access.
# Usage:   bash scripts/deploy_testnet.sh
#
# NOTE: 4-token pools are deployed DIRECTLY here — the factory (deploy_all.sh) only
# supports 2-token pools. n>2 must be SuperElliptical (Circular is 2-token only).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Optional local config: copy .env.example → .env and edit. All values have defaults.
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-orbswap}"
# Test tokens are issued from a SEPARATE key: a Stellar asset cannot be minted to its
# own issuer, so the deployer must NOT be the issuer. Auto-generated + funded.
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"
FEE_BPS="${POOL_FEE_BPS:-30}"
# Space-separated asset codes → pool size. 2..8 codes. n>2 ⇒ SuperElliptical.
TOKEN_CODES="${TOKEN_CODES:-USDA USDB USDC USDD}"
mkdir -p deployments
OUT=deployments/${NET}.json
TWO_PLUS_SQRT2=3414213562373095049

say() { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }

read -ra CODES <<< "$TOKEN_CODES"
N=${#CODES[@]}
if [ "$N" -lt 2 ] || [ "$N" -gt 8 ]; then
  echo "ERROR: need 2..8 token codes, got $N ($TOKEN_CODES)" >&2; exit 1
fi
# Circular is 2-token only; anything larger is the superellipse.
if [ "$N" -eq 2 ]; then MODE=Circular; else MODE=SuperElliptical; fi

# ---------------------------------------------------------------- identity
say "Identity"
if ! stellar keys address "$IDENT" >/dev/null 2>&1; then
  stellar keys generate "$IDENT" --network "$NET" --fund
fi
DEPLOYER=$(stellar keys address "$IDENT")
stellar keys fund "$IDENT" --network "$NET" 2>/dev/null || true
echo "deployer: $DEPLOYER"

if ! stellar keys address "$ISSUER" >/dev/null 2>&1; then
  stellar keys generate "$ISSUER" --network "$NET" --fund
fi
ISSUER_ADDR=$(stellar keys address "$ISSUER")
stellar keys fund "$ISSUER" --network "$NET" 2>/dev/null || true
echo "issuer:   $ISSUER_ADDR"

# ---------------------------------------------------------------- build
say "Build + optimize pool wasm"
cargo build --target wasm32v1-none --release -p orbswap-pool
RAW=target/wasm32v1-none/release/orbswap_pool.wasm
stellar contract optimize --wasm "$RAW"
WASM=target/wasm32v1-none/release/orbswap_pool.optimized.wasm

# ---------------------------------------------------------------- test tokens
# One SAC per code, issued by $ISSUER; deployer trusts each then gets 1000 units minted.
say "Test tokens ($N): ${CODES[*]}"
IDS=()
MINT=10000000000 # 1000 units at 7 decimals
for CODE in "${CODES[@]}"; do
  stellar contract asset deploy --asset "${CODE}:${ISSUER_ADDR}" \
    --source "$ISSUER" --network "$NET" >/dev/null 2>&1 || true
  ID=$(stellar contract id asset --asset "${CODE}:${ISSUER_ADDR}" --network "$NET")
  IDS+=("$ID")
  stellar tx new change-trust --source "$IDENT" --network "$NET" \
    --line "${CODE}:${ISSUER_ADDR}" >/dev/null
  stellar contract invoke --id "$ID" --source "$ISSUER" --network "$NET" \
    -- mint --to "$DEPLOYER" --amount "$MINT" >/dev/null
  echo "  $CODE = $ID"
done

# JSON arrays for tokens and (balanced) deposit amounts.
TOKENS_JSON=$(printf '"%s",' "${IDS[@]}"); TOKENS_JSON="[${TOKENS_JSON%,}]"
DEP=1000000000 # 100 units of each (balanced first deposit)
# Vec<i128> args must be JSON arrays of STRINGS, e.g. ["1000000000", …].
AMTS_JSON=$(for _ in "${CODES[@]}"; do printf '"%s",' "$DEP"; done); AMTS_JSON="[${AMTS_JSON%,}]"

# ---------------------------------------------------------------- deploy + init
say "Deploy pool"
POOL=$(stellar contract deploy --wasm "$WASM" --source "$IDENT" --network "$NET")
echo "pool: $POOL"

say "Initialize ($MODE, $N tokens, $FEE_BPS bps)"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- \
  initialize \
  --tokens "$TOKENS_JSON" \
  --mode "$MODE" \
  --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" \
  --fee_bps "$FEE_BPS" --admin "$DEPLOYER"

# ---------------------------------------------------------------- deposit + swap
say "Deposit 100 of each (balanced)"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- \
  deposit --from "$DEPLOYER" \
  --amounts "$AMTS_JSON" --min_shares 0 --deadline 18446744073709551615

# Swap 10 of token[0] → token[last].
LAST=$((N - 1))
say "Quote + swap 10 ${CODES[0]} -> ${CODES[$LAST]}"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- \
  quote --token_in "${IDS[0]}" --amount_in 100000000 --token_out "${IDS[$LAST]}"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- \
  swap --from "$DEPLOYER" --token_in "${IDS[0]}" --amount_in 100000000 \
  --token_out "${IDS[$LAST]}" --min_out 0 --deadline 18446744073709551615

say "Reserves after swap"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- get_reserves

# ---------------------------------------------------------------- record
TOKENS_OBJ=""
for i in "${!CODES[@]}"; do
  TOKENS_OBJ+="\"${CODES[$i]}\": \"${IDS[$i]}\","
done
TOKENS_OBJ="{${TOKENS_OBJ%,}}"

cat > "$OUT" <<JSON
{
  "network": "$NET",
  "deployer": "$DEPLOYER",
  "issuer": "$ISSUER_ADDR",
  "mode": "$MODE",
  "pool": "$POOL",
  "tokens": $TOKENS_OBJ
}
JSON
say "Wrote $OUT"
cat "$OUT"
