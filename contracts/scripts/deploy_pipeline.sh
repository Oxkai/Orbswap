#!/usr/bin/env bash
# ============================================================================
# Orbswap end-to-end deployment pipeline.
#
# Runs the EXACT same sequence on testnet and mainnet — a testnet run is a true
# rehearsal, not a different code path. We issue and mint all five test assets
# ourselves on both networks; the only mainnet extras are funding the issuer
# account (no friendbot) and the stage-0 preflight.
#
#   0  preflight  (mainnet)  RPC, balance + confirmation, before any spend
#   1  identities            deployer + issuer
#   2  build                 cargo build + stellar contract optimize
#   3  periphery             upload pool wasm -> factory -> router
#   4  tokens                5 self-issued USD test assets, all 1:1
#   5  pool A                SuperElliptical, 4 tokens  (deposit/withdraw)
#   6  pool B                Circular + ticks, 2 tokens (concentrated LP)
#   7  seed liquidity        balanced deposit + full-range & narrow positions
#   8  simulations           randomised swap flow to imbalance both pools
#   9  record                deployments/<net>_stable.json
#
# Usage:  bash scripts/deploy_pipeline.sh              # testnet (default)
#         STELLAR_NETWORK=mainnet bash scripts/deploy_pipeline.sh
#         SWAPS=40 bash scripts/deploy_pipeline.sh     # heavier simulation
# ============================================================================
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-my-wallet}"
ISSUER="${TOKEN_ISSUER_IDENTITY:-orbswap-issuer}"
FEE_BPS="${POOL_FEE_BPS:-30}"
SWAPS="${SWAPS:-18}"
# Modelled cost of a full mainnet run (~156 XLM fees + 3.5 locked in reserves),
# rounded up for surge pricing and retries. Checked in preflight, never spent
# directly. See docs/MAINNET.md for the breakdown.
REQUIRED_XLM="${REQUIRED_XLM:-220}"
# XLM the deployer sends to create the issuer account on mainnet (base reserve +
# fees for 5 SAC deploys and 5 mints). Friendbot covers this on testnet.
ISSUER_SEED_XLM="${ISSUER_SEED_XLM:-5}"
# Inclusion-fee bid per transaction, in stroops. The CLI default of 100 is below
# mainnet's surge price, and a losing bid does not fail loudly: the transaction
# sits unincluded until the client reports "transaction submission timeout" — a
# FALSE NEGATIVE that has already landed duplicate contracts here. 0.01 XLM is
# noise next to the resource fee, so bid it on every call.
INCLUSION_FEE="${INCLUSION_FEE:-100000}"
# Largest single swap in stage 8, in whole tokens (all legs are 1:1 USD).
SWAP_MAX="${SWAP_MAX:-1000}"

TWO_PLUS_SQRT2=3414213562373095049   # shape alpha/beta (WAD)
M=10000000                            # 1.0 at 7 decimals
MAXU64=18446744073709551615

# Stage-7 sizes: 6M per leg on the 4-token pool, 2.5M per leg on the tick pool.
SUPER_SEED=$(( 6000000 * M ))
CIRC_FULL=$((  2000000 * M ))
CIRC_NARROW=$(( 500000 * M ))
MINT_EACH=$(( 50000000 * M ))

# Five self-issued USD assets, all pegged 1:1 to each other — the regime the
# SuperElliptical curve is calibrated for. We issue and mint every one of them.
#
# TESTNET may use the tickers of real stablecoins. That is the long-standing
# convention there: testnet assets carry no value, wallets label the network, and
# nobody reads a testnet USDC as Circle's. It makes the demo legible.
#
# MAINNET may NOT. Issuing `USDC` from an issuer that is not Circle puts a
# counterfeit under their brand on the public ledger, so the guard below refuses
# it outright rather than leaving it to whoever edits this next.
#
# Pool A takes the first four; pool B takes CODES[0] + CODES[4], so CODES[0] is
# the shared leg that connects the two pools for routing.
if [ "$NET" = "mainnet" ]; then
  DEFAULT_CODES="USDA USDB USDD USDE USDF"
else
  DEFAULT_CODES="USDC PYUSD USDT DAI FDUSD"
fi
read -r -a CODES <<< "${TOKEN_CODES:-$DEFAULT_CODES}"
if [ "${#CODES[@]}" -ne 5 ]; then
  echo "✗ TOKEN_CODES must list exactly 5 asset codes (got ${#CODES[@]})." >&2; exit 1
fi
# Tickers of real, third-party-issued Stellar/crypto assets. Never ours to mint.
REAL_TICKERS=" USDC USDT PYUSD DAI FDUSD TUSD EURC XCHF IDRT GYEN XSGD USDGLO USDS USDX EURS BUSD "
if [ "$NET" = "mainnet" ]; then
  for CODE in "${CODES[@]}"; do
    case "$REAL_TICKERS" in
      *" $CODE "*)
        echo "✗ refusing to issue '$CODE' on mainnet: that ticker belongs to a real" >&2
        echo "  issuer, and minting it from our own key would be asset impersonation." >&2
        echo "  Use neutral codes (the mainnet default is: USDA USDB USDD USDE USDF)." >&2
        exit 1;;
    esac
  done
fi
SUPER_CODES=("${CODES[@]:0:4}")   # pool A legs
SHARED_CODE="${CODES[0]}"         # common to both pools
CIRC_CODE="${CODES[4]}"           # pool B's second leg

# Horizon differs per network; used for account existence + balance checks.
if [ "$NET" = "mainnet" ]; then HORIZON=https://horizon.stellar.org
else HORIZON=https://horizon-testnet.stellar.org; fi

mkdir -p deployments
OUT="deployments/${NET}_stable.json"
LOG="deployments/${NET}_pipeline.log"
: > "$LOG"

say()  { printf '\n\033[1;36m== %s\033[0m\n' "$*" | tee -a "$LOG"; }
note() { printf '   %s\n' "$*" | tee -a "$LOG"; }
inv()  { retry 4 stellar contract invoke --id "$1" --source "$2" --network "$NET" --inclusion-fee "$INCLUSION_FEE" -- "${@:3}"; }

# Public RPCs intermittently return "transaction submission timeout" while the
# transaction is still in flight, so every network call is retried. Uploads are
# idempotent (the CLI skips an already-installed wasm) and a retried deploy just
# mints a fresh contract id for a few thousand stroops, so retrying is cheap. An
# invoke that already landed fails the retry loudly rather than silently double
# -applying, which is what we want for initialize/enable_ticks.
retry() {
  local n="$1"; shift
  local i=1
  until "$@"; do
    if [ "$i" -ge "$n" ]; then
      printf '   ! %s failed after %s attempts\n' "$1" "$n" >&2
      return 1
    fi
    printf '   … retry %s/%s (%s)\n' "$i" "$n" "$1" >&2
    i=$(( i + 1 )); sleep 6
  done
}

# --------------------------------------------------------------- 1 identities
say "1/9  Identities  (network: $NET)"
# Both networks use the same two identities: the deployer pays for everything and
# admins the pools; the issuer issues the five test assets. They must be distinct —
# a Stellar asset cannot be minted to its own issuer.
if [ "$NET" = "mainnet" ]; then
  # No friendbot on mainnet. The deployer must already exist and hold XLM; the
  # issuer key is only generated locally here (creating it on-chain is a spend, so
  # that waits until stage 4, after preflight and confirmation).
  DEPLOYER=$(stellar keys address "$IDENT")
  stellar keys address "$ISSUER" >/dev/null 2>&1 || stellar keys generate "$ISSUER" --network "$NET" >/dev/null
  ISSUER_ADDR=$(stellar keys address "$ISSUER")
  note "deployer: $DEPLOYER  (must hold >= $REQUIRED_XLM XLM — verified in preflight)"
  note "issuer:   $ISSUER_ADDR"
else
  stellar keys address "$IDENT" >/dev/null 2>&1 || stellar keys generate "$IDENT" --network "$NET" --fund >/dev/null
  stellar keys fund "$IDENT" --network "$NET" >/dev/null 2>&1 || true
  DEPLOYER=$(stellar keys address "$IDENT")
  # Deliberately NOT friendbot-funded: the issuer account is created by
  # create-account from the deployer on both networks (stage 1b), so a testnet run
  # exercises the identical path mainnet will take.
  stellar keys address "$ISSUER" >/dev/null 2>&1 || stellar keys generate "$ISSUER" --network "$NET" >/dev/null
  ISSUER_ADDR=$(stellar keys address "$ISSUER")
  note "deployer: $DEPLOYER"
  note "issuer:   $ISSUER_ADDR"
fi

# ---------------------------------------------------------------- 0 preflight
# Mainnet only. Stage 3 spends ~154 XLM on wasm uploads and stage 7 needs real
# token balances, so everything that can be checked is checked BEFORE any spend:
# a run that dies at stage 7 has already burnt the uploads and cannot refund them.
if [ "$NET" = "mainnet" ]; then
  say "0/9  Preflight (mainnet)"

  if ! stellar network health --network "$NET" >/dev/null 2>&1; then
    echo "  ✗ RPC for '$NET' is unreachable. Configure one:" >&2
    echo "    stellar network add mainnet --rpc-url <url> \\" >&2
    echo "      --network-passphrase 'Public Global Stellar Network ; September 2015'" >&2
    exit 1
  fi
  note "✓ RPC healthy"

  ACCT_JSON=$(curl -s -m 20 "${HORIZON}/accounts/${DEPLOYER}" || true)
  if ! printf '%s' "$ACCT_JSON" | grep -q '"account_id"'; then
    echo "  ✗ $DEPLOYER does not exist on mainnet. Fund it first (min 1 XLM base reserve)." >&2
    exit 1
  fi

  # XLM balance vs. the modelled cost of a full run.
  XLM_BAL=$(printf '%s' "$ACCT_JSON" | python3 -c '
import json,sys
d=json.load(sys.stdin)
print(next((b["balance"] for b in d["balances"] if b["asset_type"]=="native"),"0"))')
  note "XLM balance: $XLM_BAL   (need ~$REQUIRED_XLM)"
  if ! python3 -c "import sys; sys.exit(0 if float('$XLM_BAL') >= float('$REQUIRED_XLM') else 1)"; then
    echo "  ✗ Insufficient XLM. Need ~$REQUIRED_XLM, have $XLM_BAL." >&2
    exit 1
  fi
  note "✓ XLM sufficient"

  # The five assets are self-issued, so there is nothing to hold up front: the
  # issuer mints them in stage 4. What we do need is the issuer ACCOUNT to exist
  # on-chain, which on mainnet costs a create-account from the deployer.
  ISS_JSON=$(curl -s -m 20 "${HORIZON}/accounts/${ISSUER_ADDR}" || true)
  if printf '%s' "$ISS_JSON" | grep -q '"account_id"'; then
    note "✓ issuer account exists"
    ISSUER_NEEDS_CREATE=0
  else
    note "issuer account does not exist yet — stage 4 will create it (${ISSUER_SEED_XLM} XLM)"
    ISSUER_NEEDS_CREATE=1
  fi

  note "assets: ${CODES[*]}  (self-issued, minted in stage 4 — nothing to pre-fund)"
  printf '   seeding: pool A %s x4, pool B %s + %s\n' \
    "$(python3 -c "print(f'{$SUPER_SEED/1e7:,.0f}')")" \
    "$(python3 -c "print(f'{$CIRC_FULL/1e7:,.0f}')")" \
    "$(python3 -c "print(f'{$CIRC_NARROW/1e7:,.0f}')")" | tee -a "$LOG"

  if [ "${DRY_RUN:-0}" = 1 ]; then
    say "DRY_RUN=1 — preflight passed, stopping before any spend"; exit 0
  fi

  # Irreversible from here: uploads cannot be refunded.
  if [ "${CONFIRM:-}" != "DEPLOY MAINNET" ]; then
    printf '\n\033[1;33m  About to spend ~%s XLM of real funds on %s.\033[0m\n' "$REQUIRED_XLM" "$NET"
    printf '  Type exactly: DEPLOY MAINNET\n  > '
    read -r reply
    [ "$reply" = "DEPLOY MAINNET" ] || { echo "  aborted."; exit 1; }
  fi
fi

# ------------------------------------------------------- 1b issuer account
# Runs on BOTH networks and BEFORE the uploads. Stage 3 spends ~154 XLM that
# cannot be refunded, so the one step with no testnet precedent must fail (if it
# is going to) while walking away still costs nothing.
if ! curl -s -m 15 "${HORIZON}/accounts/${ISSUER_ADDR}" | grep -q '"account_id"'; then
  say "1b/9  Create issuer account (${ISSUER_SEED_XLM} XLM from deployer)"
  stellar tx new create-account --source-account "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE" \
    --destination "$ISSUER_ADDR" \
    --starting-balance $(( ISSUER_SEED_XLM * M )) >>"$LOG" 2>&1
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    curl -s -m 15 "${HORIZON}/accounts/${ISSUER_ADDR}" | grep -q '"account_id"' && break
    sleep 2
  done
  curl -s -m 15 "${HORIZON}/accounts/${ISSUER_ADDR}" | grep -q '"account_id"' || {
    echo "  ✗ issuer account was not created — aborting before any upload spend." >&2; exit 1; }
  note "✓ issuer account live"
else
  say "1b/9  Issuer account already exists"
fi

# ------------------------------------------------------------------- 2 build
say "2/9  Build + optimize wasm"
cargo build --target wasm32v1-none --release \
  -p orbswap-pool -p orbswap-factory -p orbswap-router >>"$LOG" 2>&1
REL=target/wasm32v1-none/release
for w in orbswap_pool orbswap_factory orbswap_router; do
  stellar contract optimize --wasm "$REL/$w.wasm" >>"$LOG" 2>&1
  note "$(basename "$REL/$w.optimized.wasm")  $(wc -c <"$REL/$w.optimized.wasm" | tr -d ' ') bytes"
done
POOL_WASM="$REL/orbswap_pool.optimized.wasm"

# --------------------------------------------------------------- 3 periphery
say "3/9  Periphery: pool wasm -> factory -> router"
POOL_HASH=$(retry 4 stellar contract upload --wasm "$POOL_WASM" --source "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE")
note "pool wasm hash: $POOL_HASH"
FACTORY=$(retry 4 stellar contract deploy --wasm "$REL/orbswap_factory.optimized.wasm" --source "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE")
inv "$FACTORY" "$IDENT" initialize --admin "$DEPLOYER" --pool_wasm_hash "$POOL_HASH" >>"$LOG" 2>&1
note "factory: $FACTORY"
ROUTER=$(retry 4 stellar contract deploy --wasm "$REL/orbswap_router.optimized.wasm" --source "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE")
note "router:  $ROUTER"

# ------------------------------------------------------------------ 4 tokens
# Identical on both networks: WE issue all five assets and mint freely. Codes are
# deliberately neutral (see the CODES comment) so nothing impersonates a real
# Stellar asset on the public ledger.
say "4/9  Tokens: ${CODES[*]}  (issued by $ISSUER_ADDR)"
IDS=()
for CODE in "${CODES[@]}"; do
  ISS="$ISSUER_ADDR"
  # Idempotent: the SAC may already be deployed from an earlier run.
  retry 3 stellar contract asset deploy --asset "${CODE}:${ISS}" --source "$ISSUER" --network "$NET" --inclusion-fee "$INCLUSION_FEE" >>"$LOG" 2>&1 || true
  ID=$(stellar contract id asset --asset "${CODE}:${ISS}" --network "$NET")
  IDS+=("$ID")
  # Deployer must trust the asset before it can hold a balance.
  retry 3 stellar tx new change-trust --source "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE" --line "${CODE}:${ISS}" >>"$LOG" 2>&1 || true
  inv "$ID" "$ISSUER" mint --to "$DEPLOYER" --amount "$MINT_EACH" >>"$LOG" 2>&1
  note "$(printf '%-7s %s' "$CODE" "$ID")"
done
SHARED="${IDS[0]}"   # ${SHARED_CODE} - in both pools
CIRC_B="${IDS[4]}"   # ${CIRC_CODE} - pool B only

# ------------------------------------------------------------------ 5 pool A
# create_pool on the factory is hard-limited to 2 tokens, so the 4-token pool
# is deployed straight from the wasm hash and initialized directly.
say "5/9  Pool A - SuperElliptical, 4 tokens (${SUPER_CODES[*]})"
SUPER=$(retry 4 stellar contract deploy --wasm "$POOL_WASM" --source "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE")
inv "$SUPER" "$IDENT" initialize \
  --tokens "[\"${IDS[0]}\",\"${IDS[1]}\",\"${IDS[2]}\",\"${IDS[3]}\"]" --mode SuperElliptical \
  --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >>"$LOG" 2>&1
note "super pool: $SUPER"

# ------------------------------------------------------------------ 6 pool B
# Second leg is the 5th asset; the first leg is shared with pool A so the two
# pools are connected and the router can hop between them.
say "6/9  Pool B - Circular + ticks, 2 tokens ($SHARED_CODE/$CIRC_CODE)"
CIRCLE=$(retry 4 stellar contract deploy --wasm "$POOL_WASM" --source "$IDENT" --network "$NET" --inclusion-fee "$INCLUSION_FEE")
inv "$CIRCLE" "$IDENT" initialize \
  --tokens "[\"$SHARED\",\"$CIRC_B\"]" --mode Circular \
  --alpha "$TWO_PLUS_SQRT2" --beta "$TWO_PLUS_SQRT2" --fee_bps "$FEE_BPS" --admin "$DEPLOYER" >>"$LOG" 2>&1
inv "$CIRCLE" "$IDENT" enable_ticks >>"$LOG" 2>&1
note "circle pool: $CIRCLE  (tick mode on)"

# ----------------------------------------------------------- 7 seed liquidity
say "7/9  Seed liquidity"
inv "$SUPER" "$IDENT" deposit --from "$DEPLOYER" \
  --amounts "[\"$SUPER_SEED\",\"$SUPER_SEED\",\"$SUPER_SEED\",\"$SUPER_SEED\"]" \
  --min_shares 0 --deadline "$MAXU64" >>"$LOG" 2>&1
note "pool A: $(( SUPER_SEED / M )) of each leg  ->  $(( 4 * SUPER_SEED / M )) total"

# First add must be full-range and balanced: it sets theta_c = 45deg.
inv "$CIRCLE" "$IDENT" add_liquidity --from "$DEPLOYER" \
  --amounts "[\"$CIRC_FULL\",\"$CIRC_FULL\"]" --lower 0 --upper 90 \
  --min_liquidity 0 --deadline "$MAXU64" >>"$LOG" 2>&1
note "pool B: full-range [0,90]  $(( CIRC_FULL / M )) + $(( CIRC_FULL / M ))"
# Narrow band around the peg, where a 1:1 stable pair actually trades.
inv "$CIRCLE" "$IDENT" add_liquidity --from "$DEPLOYER" \
  --amounts "[\"$CIRC_NARROW\",\"$CIRC_NARROW\"]" --lower 40 --upper 50 \
  --min_liquidity 0 --deadline "$MAXU64" >>"$LOG" 2>&1
note "pool B: narrow    [40,50] $(( CIRC_NARROW / M )) + $(( CIRC_NARROW / M ))"

# -------------------------------------------------------------- 8 simulations
say "8/9  Simulations: $SWAPS randomised swaps (max $SWAP_MAX per swap)"
ok=0; fail=0
for i in $(seq 1 "$SWAPS"); do
  if (( RANDOM % 3 == 0 )); then
    PID="$CIRCLE"; LBL="circle"
    if (( RANDOM % 2 == 0 )); then TIN=$SHARED; TOUT=$CIRC_B; else TIN=$CIRC_B; TOUT=$SHARED; fi
  else
    PID="$SUPER";  LBL="super "
    # pool A holds only the first four assets
    a=$(( RANDOM % 4 )); b=$(( RANDOM % 4 )); while (( b == a )); do b=$(( RANDOM % 4 )); done
    TIN="${IDS[$a]}"; TOUT="${IDS[$b]}"
  fi
  AMT=$(( (RANDOM % SWAP_MAX + 1) * M ))
  if inv "$PID" "$IDENT" swap --from "$DEPLOYER" --token_in "$TIN" \
       --amount_in "$AMT" --token_out "$TOUT" --min_out 0 --deadline "$MAXU64" >>"$LOG" 2>&1; then
    ok=$(( ok + 1 )); printf '   swap %2d/%s  %s  %8s  ok\n' "$i" "$SWAPS" "$LBL" "$(( AMT / M ))" | tee -a "$LOG"
  else
    fail=$(( fail + 1 )); printf '   swap %2d/%s  %s  %8s  skipped\n' "$i" "$SWAPS" "$LBL" "$(( AMT / M ))" | tee -a "$LOG"
  fi
done
note "landed $ok/$SWAPS  (skipped $fail)"

say "Final state"
note "pool A reserves: $(inv "$SUPER" "$IDENT" get_reserves 2>/dev/null | tr -d '\n')"
note "pool B reserves: $(inv "$CIRCLE" "$IDENT" get_reserves 2>/dev/null | tr -d '\n')"
note "pool B spot:     $(inv "$CIRCLE" "$IDENT" get_spot_price 2>/dev/null | tr -d '\n')"

# ------------------------------------------------------------------ 9 record
say "9/9  Record"
TOKENS_JSON=""
for i in "${!CODES[@]}"; do TOKENS_JSON+="\"${CODES[$i]}\": \"${IDS[$i]}\", "; done
TOKENS_JSON="${TOKENS_JSON%, }"
cat > "$OUT" <<JSON
{
  "network": "$NET",
  "deployer": "$DEPLOYER",
  "pool_wasm_hash": "$POOL_HASH",
  "factory": "$FACTORY",
  "router": "$ROUTER",
  "super_pool": "$SUPER",
  "circle_pool": "$CIRCLE",
  "fee_bps": $FEE_BPS,
  "swaps_landed": $ok,
  "super_tokens": ["${SUPER_CODES[0]}", "${SUPER_CODES[1]}", "${SUPER_CODES[2]}", "${SUPER_CODES[3]}"],
  "circle_tokens": ["$SHARED_CODE", "$CIRC_CODE"],
  "shared_token": "$SHARED_CODE",
  "tokens": { $TOKENS_JSON }
}
JSON
note "wrote $OUT"
cat "$OUT"
