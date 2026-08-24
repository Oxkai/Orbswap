#!/usr/bin/env bash
# Keeper for a rate-aware Orbswap pool. Each tick:
#   1. poke_rate                    — pull the feed, accept or trip the breaker
#   2. re_anchor (if needed)        — reopen the pool on the new curve
#   3. append a row to the SPREAD LOG
#
# The spread log is the point. It records, every tick, what the pool would quote
# against the oracle mid — which is the artifact an anchor is shown to answer
# "would you route conversion through this?" (todo.md §10).
#
#   bash scripts/keeper.sh                  # loop forever at KEEPER_INTERVAL
#   bash scripts/keeper.sh --once           # single tick (cron / CI)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[ -f .env ] && set -a && . ./.env && set +a

NET="${STELLAR_NETWORK:-testnet}"
IDENT="${DEPLOYER_IDENTITY:-orbswap}"
DEPLOY_FILE="${DEPLOY_FILE:-deployments/${NET}_rates.json}"
INTERVAL="${KEEPER_INTERVAL:-600}"          # 10 min, matching Lightecho's cadence
LOG="${SPREAD_LOG:-deployments/spread_log.csv}"
MAXU64=18446744073709551615
M=10000000
PROBE="${PROBE_AMOUNT:-$M}"                 # 1.0 numeraire unit
ONCE=0
[ "${1:-}" = "--once" ] && ONCE=1

[ -f "$DEPLOY_FILE" ] || { echo "no $DEPLOY_FILE — run scripts/deploy_rate_pool.sh first" >&2; exit 1; }
jqr() { python3 -c "import json,sys;print(json.load(open('$DEPLOY_FILE'))$1)"; }
POOL=$(jqr "['pool']")
FEED=$(jqr "['feed']")
FEED_DEC=$(jqr "['feed_decimals']")
# The deploy script writes `tokens` in {numeraire, quote} order.
NUM=$(python3 -c "import json;t=json.load(open('$DEPLOY_FILE'))['tokens'];print(list(t.values())[0])")
QUOTE=$(python3 -c "import json;t=json.load(open('$DEPLOY_FILE'))['tokens'];print(list(t.values())[1])")

inv()  { stellar contract invoke --id "$1" --source "$IDENT" --network "$NET" -- "${@:2}" 2>/dev/null; }
invq() { stellar contract invoke --id "$1" --source "$IDENT" --network "$NET" -- "${@:2}" >/dev/null 2>&1; }
strip() { tr -d '"' ; }

if [ ! -f "$LOG" ]; then
  echo "utc,rate_wad,oracle_out,pool_out,spread_bps,fresh,breaker,reanchored" > "$LOG"
fi

tick() {
  local ts rate fresh breaker status pool_out oracle_out spread reanchored=0

  # 1. Pull the feed. A deviation does NOT error — it latches the breaker and
  #    returns the old rate (Soroban rolls back state on Err), so the outcome is
  #    read from rate_status, not from this call.
  invq "$POOL" poke_rate || true

  # 2. Reopen the pool if the accepted rate moved it off-curve.
  if [ "$(inv "$POOL" needs_reanchor | strip)" = "true" ]; then
    if invq "$POOL" re_anchor --deadline "$MAXU64"; then
      reanchored=1
    else
      echo "  !! re_anchor FAILED — pool stays closed" >&2
    fi
  fi

  # 3. Log the spread: what the pool quotes vs what the oracle rate implies.
  status=$(inv "$POOL" rate_status)
  rate=$(echo "$status" | python3 -c "import sys,json;print(json.load(sys.stdin)[0])" 2>/dev/null || echo 0)
  fresh=$(echo "$status" | python3 -c "import sys,json;print(json.load(sys.stdin)[2])" 2>/dev/null || echo false)
  breaker=$(echo "$status" | python3 -c "import sys,json;print(json.load(sys.stdin)[3])" 2>/dev/null || echo false)

  pool_out=$(inv "$POOL" quote --token_in "$NUM" --amount_in "$PROBE" --token_out "$QUOTE" | strip || echo 0)
  # Frictionless output at the oracle rate: probe / rate, in quote-token units.
  oracle_out=$(python3 -c "
r=int('${rate:-0}' or 0)
print(int($PROBE * 10**18 // r) if r > 0 else 0)")
  spread=$(python3 -c "
o=int('${oracle_out:-0}' or 0); p=int('${pool_out:-0}' or 0)
print(f'{(o-p)*10000/o:.2f}' if o > 0 else '')")

  ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  echo "$ts,$rate,$oracle_out,$pool_out,$spread,$fresh,$breaker,$reanchored" >> "$LOG"
  printf '%s  rate=%s  spread=%s bps  fresh=%s  breaker=%s  reanchored=%s\n' \
    "$ts" "$rate" "${spread:-n/a}" "$fresh" "$breaker" "$reanchored"

  [ "$breaker" = "true" ] && echo "  !! BREAKER LATCHED — trading halted, withdrawals open. Clear with: set_breaker false" >&2
  return 0
}

echo "keeper: pool=$POOL feed=$FEED (decimals $FEED_DEC) log=$LOG"
if [ "$ONCE" = "1" ]; then tick; exit 0; fi
while true; do tick || true; sleep "$INTERVAL"; done
