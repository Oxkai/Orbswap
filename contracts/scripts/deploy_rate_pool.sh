#!/usr/bin/env bash
# Deploy a RATE-AWARE pool on Stellar testnet:
#   • SEP-40 feed stub (operator-controlled prices — testnet only)
#   • SuperElliptical USDC/NGNC pool whose balanced point tracks the feed
#   • configure_rates → seed with EQUAL VALUE (not equal units)
#   • smoke: quote, poke_rate, re_anchor, quote again
# Writes deployments/<network>_rates.json.
#
# In production the pool points at Reflector or Lightecho by address instead —
# no contract change, because it consumes any SEP-40 feed. See orbswap-feed-stub.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-orbswap}"
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"
FEE_BPS="${POOL_FEE_BPS:-30}"
# Curve shape. NOT 2+sqrt(2) (the circle) — an FX pool takes its price from the
# oracle, so the curve's job is inventory management, and it should quote far
# tighter at the balanced point. Measured at a 10k pool (tests/curve_calibration.rs):
#
#   alpha    slippage @10% of reserves
#   2.01       7 bps
#   2.05      34 bps      <- default: tight, but still curves away from a drain
#   3.414    396 bps      <- the circle
#
# Flatter quotes tighter but resists draining less, so the oracle guards carry
# more of the safety burden. Override with POOL_ALPHA.
ALPHA="${POOL_ALPHA:-2050000000000000000}"
M=10000000                       # 1.0 at 7 decimals
MAXU64=18446744073709551615
FEED_DEC=14                      # matches Reflector
ONE_FEED=100000000000000         # 1.0 at 14 decimals

# 1 NGNC = 0.001 USDC. Change these two to retarget the demo pair.
QUOTE_CODE="${QUOTE_CODE:-NGNC}"
NUM_CODE="${NUM_CODE:-USDC}"
QUOTE_PRICE="${QUOTE_PRICE:-$(( ONE_FEED / 1000 ))}"
MAX_AGE_SECS="${MAX_AGE_SECS:-3600}"
MAX_DEVIATION_BPS="${MAX_DEVIATION_BPS:-500}"

# Seed: equal VALUE on both legs. 10k USDC ↔ 10,000,000 NGNC at 0.001.
SEED_NUM="${SEED_NUM:-10000}"
SEED_QUOTE=$(( SEED_NUM * ONE_FEED / QUOTE_PRICE ))

mkdir -p deployments
OUT="deployments/${NET}_rates.json"

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

say "Build + optimize wasm"
cargo build --target wasm32v1-none --release -p orbswap-pool -p orbswap-feed-stub
REL=target/wasm32v1-none/release
for w in orbswap_pool orbswap_feed_stub; do
  stellar contract optimize --wasm "$REL/$w.wasm" >/dev/null
done
POOL_WASM="$REL/orbswap_pool.optimized.wasm"
FEED_WASM="$REL/orbswap_feed_stub.optimized.wasm"
printf 'pool wasm: %s KB (limit 128)\n' "$(( $(wc -c < "$POOL_WASM") / 1024 ))"

say "Tokens"
NUM=$(token_id "$NUM_CODE"); QUOTE=$(token_id "$QUOTE_CODE")
echo "  $NUM_CODE=$NUM"; echo "  $QUOTE_CODE=$QUOTE"
for pair in "$NUM_CODE:$NUM:$(( SEED_NUM * 3 ))" "$QUOTE_CODE:$QUOTE:$(( SEED_QUOTE * 3 ))"; do
  code="${pair%%:*}"; rest="${pair#*:}"; tid="${rest%%:*}"; amt="${rest##*:}"
  stellar tx new change-trust --source "$IDENT" --network "$NET" --line "${code}:${ISSUER_ADDR}" >/dev/null
  inv "$tid" "$ISSUER" mint --to "$DEPLOYER" --amount "$(( amt * M ))" >/dev/null
done

say "SEP-40 feed stub (testnet stand-in for Reflector/Lightecho)"
FEED=$(stellar contract deploy --wasm "$FEED_WASM" --source "$IDENT" --network "$NET")
inv "$FEED" "$IDENT" initialize --admin "$DEPLOYER" --decimals "$FEED_DEC" --base "$NUM" >/dev/null
inv "$FEED" "$IDENT" set_price --asset "$QUOTE" --price "$QUOTE_PRICE" >/dev/null
inv "$FEED" "$IDENT" set_price --asset "$NUM"   --price "$ONE_FEED"    >/dev/null
echo "feed: $FEED  (1 $QUOTE_CODE = $QUOTE_PRICE / $ONE_FEED $NUM_CODE)"

say "Rate-aware pool  $NUM_CODE/$QUOTE_CODE  (alpha=$ALPHA)"
# Token order matters: index 0 = numeraire, index 1 = quote.
POOL=$(stellar contract deploy --wasm "$POOL_WASM" --source "$IDENT" --network "$NET")
inv "$POOL" "$IDENT" initialize --tokens "[\"$NUM\",\"$QUOTE\"]" \
  --mode SuperElliptical --alpha "$ALPHA" --beta "$ALPHA" \
  --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >/dev/null

# configure_rates must run BEFORE any liquidity: converting a live parity pool
# would revalue one leg and hand the difference to the first trader (todo.md §0).
inv "$POOL" "$IDENT" configure_rates --feed "$FEED" --quote_index 1 --numeraire_index 0 \
  --cross false --max_age_secs "$MAX_AGE_SECS" --max_deviation_bps "$MAX_DEVIATION_BPS" >/dev/null
echo "pool: $POOL"
echo "  rate: $(inv "$POOL" "$IDENT" get_rate --token "$QUOTE")"

say "Seed with EQUAL VALUE ($SEED_NUM $NUM_CODE ↔ $SEED_QUOTE $QUOTE_CODE)"
inv "$POOL" "$IDENT" deposit --from "$DEPLOYER" \
  --amounts "[\"$(( SEED_NUM * M ))\",\"$(( SEED_QUOTE * M ))\"]" \
  --min_shares 0 --deadline "$MAXU64" >/dev/null
echo "  reserves: $(inv "$POOL" "$IDENT" get_reserves)"
echo "  on_curve: $(inv "$POOL" "$IDENT" is_on_curve)"

say "Smoke: quote → move rate → pool closes → re_anchor → reopens"
echo "  quote 1 $NUM_CODE  → $(inv "$POOL" "$IDENT" quote --token_in "$NUM" --amount_in "$M" --token_out "$QUOTE") $QUOTE_CODE"
inv "$FEED" "$IDENT" set_price --asset "$QUOTE" --price "$(( QUOTE_PRICE * 101 / 100 ))" >/dev/null
inv "$POOL" "$IDENT" poke_rate >/dev/null
echo "  after +1% rate move, needs_reanchor: $(inv "$POOL" "$IDENT" needs_reanchor)"
inv "$POOL" "$IDENT" re_anchor --deadline "$MAXU64" >/dev/null
echo "  after re_anchor,     needs_reanchor: $(inv "$POOL" "$IDENT" needs_reanchor)"
echo "  quote 1 $NUM_CODE  → $(inv "$POOL" "$IDENT" quote --token_in "$NUM" --amount_in "$M" --token_out "$QUOTE") $QUOTE_CODE"

say "Operator mode (anchor is the sole LP; anyone may trade)"
inv "$POOL" "$IDENT" set_operator --who "$DEPLOYER" --allowed true >/dev/null
inv "$POOL" "$IDENT" set_operator_mode --enabled true >/dev/null
echo "  operator_status(deployer): $(inv "$POOL" "$IDENT" operator_status --who "$DEPLOYER")"

cat > "$OUT" <<JSON
{
  "network": "$NET",
  "deployer": "$DEPLOYER",
  "issuer": "$ISSUER_ADDR",
  "feed": "$FEED",
  "feed_decimals": $FEED_DEC,
  "pool": "$POOL",
  "fee_bps": $FEE_BPS,
  "quote_index": 1,
  "numeraire_index": 0,
  "max_age_secs": $MAX_AGE_SECS,
  "max_deviation_bps": $MAX_DEVIATION_BPS,
  "tokens": { "$NUM_CODE": "$NUM", "$QUOTE_CODE": "$QUOTE" }
}
JSON
say "Wrote $OUT"; cat "$OUT"
