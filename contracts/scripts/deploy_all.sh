#!/usr/bin/env bash
# Deploy the FULL Orbswap system to testnet: pool wasm upload → factory → a pool
# (via the factory) → router → a smoke swap. Writes deployments/testnet_full.json.
#
# Prereqs: stellar-cli 27+, rust 1.91 + wasm32v1-none. Network access.
# Usage:   bash scripts/deploy_all.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Optional local config: copy .env.example → .env and edit. All values have defaults.
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-orbswap}"
# Separate issuer (a Stellar asset can't be minted to its own issuer).
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"
FEE_BPS="${POOL_FEE_BPS:-30}"
TOKEN_A_CODE="${TOKEN_A_CODE:-USDA}"
TOKEN_B_CODE="${TOKEN_B_CODE:-USDB}"
mkdir -p deployments
OUT=deployments/${NET}_full.json
TWO_PLUS_SQRT2=3414213562373095049

say() { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }

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
say "Build + optimize all contract wasms"
cargo build --target wasm32v1-none --release \
  -p orbswap-pool -p orbswap-factory -p orbswap-router
REL=target/wasm32v1-none/release
for w in orbswap_pool orbswap_factory orbswap_router; do
  stellar contract optimize --wasm "$REL/$w.wasm"
done

# ---------------------------------------------------------------- upload pool wasm
say "Upload pool wasm → hash"
POOL_HASH=$(stellar contract upload --wasm "$REL/orbswap_pool.optimized.wasm" \
  --source "$IDENT" --network "$NET")
echo "pool wasm hash: $POOL_HASH"

# ---------------------------------------------------------------- factory
say "Deploy + initialize factory"
FACTORY=$(stellar contract deploy --wasm "$REL/orbswap_factory.optimized.wasm" \
  --source "$IDENT" --network "$NET")
stellar contract invoke --id "$FACTORY" --source "$IDENT" --network "$NET" -- \
  initialize --admin "$DEPLOYER" --pool_wasm_hash "$POOL_HASH"
echo "factory: $FACTORY"

# ---------------------------------------------------------------- router
say "Deploy router"
ROUTER=$(stellar contract deploy --wasm "$REL/orbswap_router.optimized.wasm" \
  --source "$IDENT" --network "$NET")
echo "router: $ROUTER"

# ---------------------------------------------------------------- tokens
say "Test tokens ($TOKEN_A_CODE, $TOKEN_B_CODE)"
deploy_token() {
  stellar contract asset deploy --asset "$1:${ISSUER_ADDR}" \
    --source "$ISSUER" --network "$NET" >/dev/null 2>&1 || true
  stellar contract id asset --asset "$1:${ISSUER_ADDR}" --network "$NET"
}
USDA=$(deploy_token "$TOKEN_A_CODE"); USDB=$(deploy_token "$TOKEN_B_CODE")
# Deployer must trust each asset before holding the minted balance.
for CODE in "$TOKEN_A_CODE" "$TOKEN_B_CODE"; do
  stellar tx new change-trust --source "$IDENT" --network "$NET" \
    --line "${CODE}:${ISSUER_ADDR}" >/dev/null
done
for T in "$USDA" "$USDB"; do
  stellar contract invoke --id "$T" --source "$ISSUER" --network "$NET" \
    -- mint --to "$DEPLOYER" --amount 10000000000 >/dev/null
done
echo "USDA=$USDA  USDB=$USDB"

# ---------------------------------------------------------------- create pool via factory
say "create_pool via factory (Circular, $FEE_BPS bps)"
POOL=$(stellar contract invoke --id "$FACTORY" --source "$IDENT" --network "$NET" -- \
  create_pool \
  --tokens "[\"$USDA\",\"$USDB\"]" --mode Circular \
  --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" \
  --fee_bps "$FEE_BPS" --pool_admin "$DEPLOYER")
POOL=$(echo "$POOL" | tr -d '"')
echo "pool: $POOL"

# ---------------------------------------------------------------- deposit + swap via router
say "Deposit 100+100 then router swap 10 USDA"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- \
  deposit --from "$DEPLOYER" --amounts '[1000000000,1000000000]' \
  --min_shares 0 --deadline 18446744073709551615
stellar contract invoke --id "$ROUTER" --source "$IDENT" --network "$NET" -- \
  swap_exact_in --user "$DEPLOYER" --pools "[\"$POOL\"]" \
  --token_in "$USDA" --amount_in 100000000 --min_out 0 \
  --deadline 18446744073709551615

say "Reserves"
stellar contract invoke --id "$POOL" --source "$IDENT" --network "$NET" -- get_reserves

# ---------------------------------------------------------------- record
cat > "$OUT" <<JSON
{
  "network": "$NET",
  "deployer": "$DEPLOYER",
  "pool_wasm_hash": "$POOL_HASH",
  "factory": "$FACTORY",
  "router": "$ROUTER",
  "pool": "$POOL",
  "tokens": { "$TOKEN_A_CODE": "$USDA", "$TOKEN_B_CODE": "$USDB" }
}
JSON
say "Wrote $OUT"; cat "$OUT"
