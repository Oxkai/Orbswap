#!/usr/bin/env bash
# Full demo deployment on Stellar testnet:
#   • factory + router (periphery)
#   • 5 SAC test tokens, with USDC SHARED across both pools
#   • SuperElliptical pool  USDC/EURC/USDM/BRLT  → 24M TVL (6M each)
#   • Circular tick pool     USDC/NGNC           → 5M TVL  (2.5M each)
#   • 18 random swaps across both pools
# Writes deployments/testnet_demo.json.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-my-wallet}"
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"
FEE_BPS=30
TWO_PLUS_SQRT2=3414213562373095049
M=10000000
MAXU64=18446744073709551615
mkdir -p deployments
OUT=deployments/${NET}_demo.json

say() { printf '\n\033[1;36m== %s\033[0m\n' "$*"; }
inv() { stellar contract invoke --id "$1" --source "$2" --network "$NET" -- "${@:3}"; }
ensure_key() {
  stellar keys address "$1" >/dev/null 2>&1 || stellar keys generate "$1" --network "$NET" --fund >/dev/null
  stellar keys fund "$1" --network "$NET" >/dev/null 2>&1 || true
  stellar keys address "$1"
}
token_id() {
  stellar contract asset deploy --asset "$1:${ISSUER_ADDR}" --source "$ISSUER" --network "$NET" >/dev/null 2>&1 || true
  stellar contract id asset --asset "$1:${ISSUER_ADDR}" --network "$NET"
}

say "Identities"
DEPLOYER=$(ensure_key "$IDENT");     echo "deployer: $DEPLOYER"
ISSUER_ADDR=$(ensure_key "$ISSUER"); echo "issuer:   $ISSUER_ADDR"

say "Build + optimize wasm (pool, factory, router)"
cargo build --target wasm32v1-none --release -p orbswap-pool -p orbswap-factory -p orbswap-router
REL=target/wasm32v1-none/release
for w in orbswap_pool orbswap_factory orbswap_router; do
  stellar contract optimize --wasm "$REL/$w.wasm" >/dev/null
done
POOL_WASM="$REL/orbswap_pool.optimized.wasm"

say "Deploy factory + router"
POOL_HASH=$(stellar contract upload --wasm "$POOL_WASM" --source "$IDENT" --network "$NET")
FACTORY=$(stellar contract deploy --wasm "$REL/orbswap_factory.optimized.wasm" --source "$IDENT" --network "$NET")
inv "$FACTORY" "$IDENT" initialize --admin "$DEPLOYER" --pool_wasm_hash "$POOL_HASH" >/dev/null
ROUTER=$(stellar contract deploy --wasm "$REL/orbswap_router.optimized.wasm" --source "$IDENT" --network "$NET")
echo "factory: $FACTORY"; echo "router:  $ROUTER"

say "Test tokens (USDC shared across both pools)"
USDC=$(token_id USDC); EURC=$(token_id EURC); USDM=$(token_id USDM); BRLT=$(token_id BRLT); NGNC=$(token_id NGNC)
echo "  USDC=$USDC"; echo "  EURC=$EURC"; echo "  USDM=$USDM"; echo "  BRLT=$BRLT"; echo "  NGNC=$NGNC"

# Trustlines + generous mint to the deployer (seed + swap buffer).
# Parallel indexed arrays (macOS bash 3.2 has no associative arrays).
MCODES=(USDC EURC USDM BRLT NGNC)
MTIDS=("$USDC" "$EURC" "$USDM" "$BRLT" "$NGNC")
MAMTS=(15000000 10000000 10000000 10000000 6000000)
for i in "${!MCODES[@]}"; do
  stellar tx new change-trust --source "$IDENT" --network "$NET" --line "${MCODES[$i]}:${ISSUER_ADDR}" >/dev/null
  inv "${MTIDS[$i]}" "$ISSUER" mint --to "$DEPLOYER" --amount "$(( ${MAMTS[$i]} * M ))" >/dev/null
done

say "SuperElliptical pool  USDC/EURC/USDM/BRLT  (24M, 6M each)"
SUPER=$(stellar contract deploy --wasm "$POOL_WASM" --source "$IDENT" --network "$NET")
inv "$SUPER" "$IDENT" initialize --tokens "[\"$USDC\",\"$EURC\",\"$USDM\",\"$BRLT\"]" \
  --mode SuperElliptical --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >/dev/null
S6=$(( 6000000 * M ))
inv "$SUPER" "$IDENT" deposit --from "$DEPLOYER" --amounts "[\"$S6\",\"$S6\",\"$S6\",\"$S6\"]" --min_shares 0 --deadline "$MAXU64" >/dev/null
echo "super: $SUPER"; echo "  reserves: $(inv "$SUPER" "$IDENT" get_reserves)"

say "Circular tick pool  USDC/NGNC  (5M, 2.5M each)"
CIRCLE=$(stellar contract deploy --wasm "$POOL_WASM" --source "$IDENT" --network "$NET")
inv "$CIRCLE" "$IDENT" initialize --tokens "[\"$USDC\",\"$NGNC\"]" \
  --mode Circular --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >/dev/null
inv "$CIRCLE" "$IDENT" enable_ticks >/dev/null
C25=$(( 2500000 * M ))
inv "$CIRCLE" "$IDENT" add_liquidity --from "$DEPLOYER" --amounts "[\"$C25\",\"$C25\"]" --lower 0 --upper 90 --min_liquidity 0 --deadline "$MAXU64" >/dev/null
# a concentrated position around the peg too
CC=$(( 400000 * M ))
inv "$CIRCLE" "$IDENT" add_liquidity --from "$DEPLOYER" --amounts "[\"$CC\",\"$CC\"]" --lower 40 --upper 50 --min_liquidity 0 --deadline "$MAXU64" >/dev/null
echo "circle: $CIRCLE"; echo "  reserves: $(inv "$CIRCLE" "$IDENT" get_reserves)"; echo "  tick: $(inv "$CIRCLE" "$IDENT" current_tick)"

say "18 random swaps across both pools"
STK=("$USDC" "$EURC" "$USDM" "$BRLT")
ok=0
for i in $(seq 1 18); do
  if (( RANDOM % 3 == 0 )); then
    if (( RANDOM % 2 == 0 )); then TIN=$USDC; TOUT=$NGNC; else TIN=$NGNC; TOUT=$USDC; fi
    PID=$CIRCLE
  else
    a=$(( RANDOM % 4 )); b=$(( RANDOM % 4 )); while (( b == a )); do b=$(( RANDOM % 4 )); done
    TIN=${STK[$a]}; TOUT=${STK[$b]}; PID=$SUPER
  fi
  AMT=$(( (RANDOM % 4000 + 100) * M ))
  if inv "$PID" "$IDENT" swap --from "$DEPLOYER" --token_in "$TIN" --amount_in "$AMT" --token_out "$TOUT" --min_out 0 --deadline "$MAXU64" >/dev/null 2>&1; then
    ok=$(( ok + 1 )); printf '  swap %2d ok\n' "$i"
  else
    printf '  swap %2d skipped\n' "$i"
  fi
done
echo "swaps landed: $ok/18"

cat > "$OUT" <<JSON
{
  "network": "$NET",
  "deployer": "$DEPLOYER",
  "issuer": "$ISSUER_ADDR",
  "factory": "$FACTORY",
  "router": "$ROUTER",
  "super_pool": "$SUPER",
  "circle_pool": "$CIRCLE",
  "fee_bps": $FEE_BPS,
  "tokens": { "USDC": "$USDC", "EURC": "$EURC", "USDM": "$USDM", "BRLT": "$BRLT", "NGNC": "$NGNC" }
}
JSON
say "Wrote $OUT"; cat "$OUT"
