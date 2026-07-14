#!/usr/bin/env bash
# Seed a FRESH 4-token Orbswap pool on Stellar testnet to EXACTLY 24M total liquidity
# using THREE separate on-chain LP identities — the live analog of the Rust
# `multi_lp_seed_to_24m` simulation:
#
#     Alice 3,000,000 of each  → 12M
#     Bob   2,000,000 of each  → 20M
#     Carol 1,000,000 of each  → 24M   (6,000,000 per token, balanced)
#
# A fresh pool is deployed so the seeding is clean & balanced (the smoke-test pool
# from deploy_testnet.sh already holds imbalanced dust). Writes the result to
# deployments/<network>_seeded.json.
#
# Prereqs: stellar-cli 27+, rust 1.91 + wasm32v1-none. Network access.
# Usage:   bash scripts/seed_testnet.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-orbswap}"          # deploys + admins the pool
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"  # issues the test tokens
FEE_BPS="${POOL_FEE_BPS:-30}"
TOKEN_CODES="${TOKEN_CODES:-USDA USDB USDC USDD}"
TWO_PLUS_SQRT2=3414213562373095049
M=10000000                                     # 1 display token (7-dec) in native units
mkdir -p deployments
OUT=deployments/${NET}_seeded.json

# Three LPs and how many DISPLAY tokens of EACH asset they seed (3+2+1 = 6M/token → 24M).
LP_NAMES=(orbswap-alice orbswap-bob orbswap-carol)
LP_SEED=(3000000 2000000 1000000)

say()  { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }
sub()  { printf '\033[0;36m   %s\033[0m\n' "$*"; }

read -ra CODES <<< "$TOKEN_CODES"
N=${#CODES[@]}
[ "$N" -ge 2 ] && [ "$N" -le 8 ] || { echo "need 2..8 token codes" >&2; exit 1; }
[ "$N" -eq 2 ] && MODE=Circular || MODE=SuperElliptical

ensure_key() {  # name → ensure identity exists + funded, echo address
  local name=$1
  stellar keys address "$name" >/dev/null 2>&1 || stellar keys generate "$name" --network "$NET" --fund >/dev/null
  stellar keys fund "$name" --network "$NET" >/dev/null 2>&1 || true
  stellar keys address "$name"
}

invoke() { stellar contract invoke --id "$1" --source "$2" --network "$NET" -- "${@:3}"; }

# ---------------------------------------------------------------- identities
say "Identities"
DEPLOYER=$(ensure_key "$IDENT");  echo "deployer: $DEPLOYER"
ISSUER_ADDR=$(ensure_key "$ISSUER"); echo "issuer:   $ISSUER_ADDR"

# ---------------------------------------------------------------- build
say "Build + optimize pool wasm"
cargo build --target wasm32v1-none --release -p orbswap-pool
stellar contract optimize --wasm target/wasm32v1-none/release/orbswap_pool.wasm
WASM=target/wasm32v1-none/release/orbswap_pool.optimized.wasm

# ---------------------------------------------------------------- tokens
say "Test tokens ($N): ${CODES[*]}"
IDS=()
for CODE in "${CODES[@]}"; do
  stellar contract asset deploy --asset "${CODE}:${ISSUER_ADDR}" \
    --source "$ISSUER" --network "$NET" >/dev/null 2>&1 || true
  ID=$(stellar contract id asset --asset "${CODE}:${ISSUER_ADDR}" --network "$NET")
  IDS+=("$ID")
  sub "$CODE = $ID"
done
TOKENS_JSON=$(printf '"%s",' "${IDS[@]}"); TOKENS_JSON="[${TOKENS_JSON%,}]"

# ---------------------------------------------------------------- deploy + init
say "Deploy + initialize pool ($MODE, $N tokens, $FEE_BPS bps)"
POOL=$(stellar contract deploy --wasm "$WASM" --source "$IDENT" --network "$NET")
echo "pool: $POOL"
invoke "$POOL" "$IDENT" initialize \
  --tokens "$TOKENS_JSON" --mode "$MODE" \
  --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" \
  --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >/dev/null

# ---------------------------------------------------------------- seed via 3 LPs
running=0
for idx in "${!LP_NAMES[@]}"; do
  LP="${LP_NAMES[$idx]}"; SEED="${LP_SEED[$idx]}"
  NATIVE=$(( SEED * M ))
  AMTS=$(printf '"%s",' $(for _ in "${CODES[@]}"; do echo "$NATIVE"; done)); AMTS="[${AMTS%,}]"

  say "LP $((idx+1))/${#LP_NAMES[@]}: $LP seeds $SEED of each token"
  LP_ADDR=$(ensure_key "$LP"); sub "address: $LP_ADDR"

  # Trustline + mint each asset to this LP.
  for i in "${!CODES[@]}"; do
    stellar tx new change-trust --source "$LP" --network "$NET" \
      --line "${CODES[$i]}:${ISSUER_ADDR}" >/dev/null
    invoke "${IDS[$i]}" "$ISSUER" mint --to "$LP_ADDR" --amount "$NATIVE" >/dev/null
  done

  # Deposit (balanced). First LP's deposit sets the liquidity scale S.
  invoke "$POOL" "$LP" deposit --from "$LP_ADDR" \
    --amounts "$AMTS" --min_shares 0 --deadline 18446744073709551615 >/dev/null

  running=$(( running + SEED * N ))
  sub "running total liquidity: $(printf "%'d" "$running") tokens"
  echo "   reserves: $(invoke "$POOL" "$IDENT" get_reserves)"
done

# ---------------------------------------------------------------- verify
say "Final state"
RES=$(invoke "$POOL" "$IDENT" get_reserves)
echo "reserves: $RES"
echo "total shares: $(invoke "$POOL" "$IDENT" total_shares)"
echo ">>> seeded total liquidity target: 24,000,000 tokens (6,000,000 per asset) <<<"

# ---------------------------------------------------------------- record
TOKENS_OBJ=""
for i in "${!CODES[@]}"; do TOKENS_OBJ+="\"${CODES[$i]}\": \"${IDS[$i]}\","; done
TOKENS_OBJ="{${TOKENS_OBJ%,}}"
LPS_OBJ=""
for idx in "${!LP_NAMES[@]}"; do
  LPS_OBJ+="\"${LP_NAMES[$idx]}\": {\"address\": \"$(stellar keys address "${LP_NAMES[$idx]}")\", \"seed_each\": ${LP_SEED[$idx]}},"
done
LPS_OBJ="{${LPS_OBJ%,}}"

cat > "$OUT" <<JSON
{
  "network": "$NET",
  "deployer": "$DEPLOYER",
  "issuer": "$ISSUER_ADDR",
  "mode": "$MODE",
  "pool": "$POOL",
  "fee_bps": $FEE_BPS,
  "seeded_total_tokens": 24000000,
  "tokens": $TOKENS_OBJ,
  "lps": $LPS_OBJ
}
JSON
say "Wrote $OUT"; cat "$OUT"
