#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# run.sh — Integration test against a local Soroban sandbox (stellar/quickstart)
#
# Deploys the real WASM of both contracts and exercises:
#   initialize → create_stream → advance time → withdraw → verify balance
#
# Usage:
#   make integration-test          # starts the sandbox, then runs this script
#   ./tests/integration/run.sh     # expects a sandbox at $STELLAR_RPC_URL
#
# Exits non-zero on any failure.

set -euo pipefail

RPC_URL="${STELLAR_RPC_URL:-http://localhost:8000/rpc}"
PASSPHRASE="${STELLAR_NETWORK_PASSPHRASE:-Standalone Network ; February 2017}"
NETWORK="it-local"
DEPOSIT=100000
RATE_PER_SECOND=10
WAIT_SECONDS=10

log() { echo "[integration] $*"; }
fail() { echo "[integration] FAIL: $*" >&2; exit 1; }
invoke() { local id="$1" src="$2"; shift 2
  stellar contract invoke --id "$id" --source "$src" --network "$NETWORK" -- "$@"; }
strip() { tr -d '"\n'; }

# ---------------------------------------------------------------------------
# Network + accounts
# ---------------------------------------------------------------------------
stellar network add "$NETWORK" --rpc-url "$RPC_URL" --network-passphrase "$PASSPHRASE"

log "Waiting for sandbox RPC at $RPC_URL..."
for _ in $(seq 1 60); do
  curl -sf -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "$RPC_URL" | grep -q healthy && break
  sleep 5
done || true
curl -sf -X POST -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' "$RPC_URL" | grep -q healthy \
  || fail "Sandbox RPC not healthy"

for key in it-admin it-employer it-employee; do
  stellar keys generate "$key" --network "$NETWORK" --fund --overwrite >/dev/null 2>&1 \
    || stellar keys fund "$key" --network "$NETWORK"
done
ADMIN=$(stellar keys address it-admin)
EMPLOYER=$(stellar keys address it-employer)
EMPLOYEE=$(stellar keys address it-employee)

# ---------------------------------------------------------------------------
# Build + deploy
# ---------------------------------------------------------------------------
log "Building contracts..."
stellar contract build
WASM_DIR=target/wasm32v1-none/release
[[ -d "$WASM_DIR" ]] || WASM_DIR=target/wasm32-unknown-unknown/release

TOKEN_ID=$(stellar contract deploy --wasm "$WASM_DIR/paystream_token.wasm" \
  --source it-admin --network "$NETWORK")
STREAM_ID=$(stellar contract deploy --wasm "$WASM_DIR/paystream_stream.wasm" \
  --source it-admin --network "$NETWORK")
[[ -n "$TOKEN_ID" && -n "$STREAM_ID" ]] || fail "Deploy returned empty contract ID"
log "Token: $TOKEN_ID  Stream: $STREAM_ID"

# ---------------------------------------------------------------------------
# initialize → mint → create_stream
# ---------------------------------------------------------------------------
invoke "$TOKEN_ID" it-admin initialize --admin "$ADMIN" --initial_supply 1000000000
invoke "$STREAM_ID" it-admin initialize --admin "$ADMIN"
invoke "$TOKEN_ID" it-admin mint --admin "$ADMIN" --to "$EMPLOYER" --amount "$DEPOSIT"

STREAM_NUM=$(invoke "$STREAM_ID" it-employer create_stream \
  --employer "$EMPLOYER" --employee "$EMPLOYEE" --token_address "$TOKEN_ID" \
  --deposit "$DEPOSIT" --rate_per_second "$RATE_PER_SECOND" --stop_time 0 | strip)
log "Stream created: $STREAM_NUM"

# ---------------------------------------------------------------------------
# Advance time (sandbox ledgers close in real time)
# ---------------------------------------------------------------------------
log "Advancing time ${WAIT_SECONDS}s..."
sleep "$WAIT_SECONDS"
CLAIMABLE=$(invoke "$STREAM_ID" it-admin claimable --stream_id "$STREAM_NUM" | strip)
log "Claimable: $CLAIMABLE"
[[ "$CLAIMABLE" -gt 0 ]] || fail "Nothing claimable after ${WAIT_SECONDS}s"

# ---------------------------------------------------------------------------
# withdraw → verify balance
# ---------------------------------------------------------------------------
BEFORE=$(invoke "$TOKEN_ID" it-admin balance --owner "$EMPLOYEE" | strip)
WITHDRAWN=$(invoke "$STREAM_ID" it-employee withdraw \
  --employee "$EMPLOYEE" --stream_id "$STREAM_NUM" | strip)
AFTER=$(invoke "$TOKEN_ID" it-admin balance --owner "$EMPLOYEE" | strip)
log "Withdrawn: $WITHDRAWN  Employee balance: $BEFORE → $AFTER"
[[ "$WITHDRAWN" -gt 0 ]] || fail "withdraw returned 0"
[[ $((AFTER - BEFORE)) -eq "$WITHDRAWN" ]] || fail "Balance delta != withdrawn amount"

# ---------------------------------------------------------------------------
# Scenario A: batch create + batch cancel
# ---------------------------------------------------------------------------
log "Scenario A: batch create/cancel"
BATCH_EMPLOYEE2=$(stellar keys generate it-employee2 --network "$NETWORK" --fund --overwrite >/dev/null 2>&1 \
  && stellar keys address it-employee2 || stellar keys address it-employee2)
invoke "$TOKEN_ID" it-admin mint --admin "$ADMIN" --to "$EMPLOYER" --amount "$DEPOSIT"

BATCH_IDS=$(invoke "$STREAM_ID" it-employer create_streams_batch \
  --employer "$EMPLOYER" \
  --employees "[\"$EMPLOYEE\",\"$BATCH_EMPLOYEE2\"]" \
  --token_address "$TOKEN_ID" \
  --deposits "[$DEPOSIT,$DEPOSIT]" \
  --rates_per_second "[$RATE_PER_SECOND,$RATE_PER_SECOND]" \
  --stop_times "[0,0]" | strip)
log "Batch stream IDs: $BATCH_IDS"
[[ -n "$BATCH_IDS" ]] || fail "create_streams_batch returned empty"

BATCH_COUNT=$(invoke "$STREAM_ID" it-admin stream_count | strip)
[[ "$BATCH_COUNT" -ge 3 ]] || fail "Expected >=3 streams after batch, got $BATCH_COUNT"

BATCH_FIRST=$(echo "$BATCH_IDS" | tr -d '[]' | cut -d',' -f1 | tr -d ' ')
BATCH_REFUND_BEFORE=$(invoke "$TOKEN_ID" it-admin balance --owner "$EMPLOYER" | strip)
invoke "$STREAM_ID" it-employer cancel --caller "$EMPLOYER" --stream_id "$BATCH_FIRST" >/dev/null
BATCH_REFUND_AFTER=$(invoke "$TOKEN_ID" it-admin balance --owner "$EMPLOYER" | strip)
log "Batch cancel refund: $BATCH_REFUND_BEFORE → $BATCH_REFUND_AFTER"
[[ $((BATCH_REFUND_AFTER - BATCH_REFUND_BEFORE)) -gt 0 ]] || fail "Batch cancel produced no refund"

# ---------------------------------------------------------------------------
# Scenario B: split stream + withdraw
# ---------------------------------------------------------------------------
log "Scenario B: split stream"
SPLIT_BENEFICIARY=$(stellar keys generate it-split --network "$NETWORK" --fund --overwrite >/dev/null 2>&1 \
  && stellar keys address it-split || stellar keys address it-split)
invoke "$TOKEN_ID" it-admin mint --admin "$ADMIN" --to "$EMPLOYER" --amount "$DEPOSIT"

SPLIT_ID=$(invoke "$STREAM_ID" it-employer create_split_stream \
  --employer "$EMPLOYER" --employee "$EMPLOYEE" --beneficiary "$SPLIT_BENEFICIARY" \
  --token_address "$TOKEN_ID" --deposit "$DEPOSIT" \
  --rate_per_second "$RATE_PER_SECOND" --stop_time 0 | strip)
log "Split stream created: $SPLIT_ID"
[[ -n "$SPLIT_ID" ]] || fail "create_split_stream returned empty"

sleep "$WAIT_SECONDS"
SPLIT_BEFORE=$(invoke "$TOKEN_ID" it-admin balance --owner "$SPLIT_BENEFICIARY" | strip)
SPLIT_WITHDRAWN=$(invoke "$STREAM_ID" it-employee withdraw \
  --employee "$EMPLOYEE" --stream_id "$SPLIT_ID" | strip)
SPLIT_AFTER=$(invoke "$TOKEN_ID" it-admin balance --owner "$SPLIT_BENEFICIARY" | strip)
log "Split withdrawn: $SPLIT_WITHDRAWN  Beneficiary balance: $SPLIT_BEFORE → $SPLIT_AFTER"
[[ "$SPLIT_WITHDRAWN" -gt 0 ]] || fail "Split withdraw returned 0"
[[ $((SPLIT_AFTER - SPLIT_BEFORE)) -gt 0 ]] || fail "Split beneficiary was not credited"

# ---------------------------------------------------------------------------
# Scenario C: linear vest stream + claimable/withdraw
# ---------------------------------------------------------------------------
log "Scenario C: linear vest stream"
VEST_EMPLOYEE=$(stellar keys generate it-vest --network "$NETWORK" --fund --overwrite >/dev/null 2>&1 \
  && stellar keys address it-vest || stellar keys address it-vest)
invoke "$TOKEN_ID" it-admin mint --admin "$ADMIN" --to "$EMPLOYER" --amount "$DEPOSIT"

VEST_ID=$(invoke "$STREAM_ID" it-employer create_vesting_stream \
  --employer "$EMPLOYER" --employee "$VEST_EMPLOYEE" --token_address "$TOKEN_ID" \
  --deposit "$DEPOSIT" --rate_per_second "$RATE_PER_SECOND" --stop_time 0 | strip)
log "Vest stream created: $VEST_ID"
[[ -n "$VEST_ID" ]] || fail "create_vesting_stream returned empty"

sleep "$WAIT_SECONDS"
VEST_CLAIMABLE=$(invoke "$STREAM_ID" it-admin claimable --stream_id "$VEST_ID" | strip)
log "Vest claimable: $VEST_CLAIMABLE"
[[ "$VEST_CLAIMABLE" -gt 0 ]] || fail "Vest stream has nothing claimable"

VEST_BEFORE=$(invoke "$TOKEN_ID" it-admin balance --owner "$VEST_EMPLOYEE" | strip)
VEST_WITHDRAWN=$(invoke "$STREAM_ID" it-employee withdraw \
  --employee "$VEST_EMPLOYEE" --stream_id "$VEST_ID" | strip)
VEST_AFTER=$(invoke "$TOKEN_ID" it-admin balance --owner "$VEST_EMPLOYEE" | strip)
log "Vest withdrawn: $VEST_WITHDRAWN  Balance: $VEST_BEFORE → $VEST_AFTER"
[[ "$VEST_WITHDRAWN" -gt 0 ]] || fail "Vest withdraw returned 0"
[[ $((VEST_AFTER - VEST_BEFORE)) -eq "$VEST_WITHDRAWN" ]] || fail "Vest balance delta != withdrawn"

# ---------------------------------------------------------------------------
# Scenario D: settle at stop_time
# ---------------------------------------------------------------------------
log "Scenario D: settle at stop_time"
SETTLE_EMPLOYEE=$(stellar keys generate it-settle --network "$NETWORK" --fund --overwrite >/dev/null 2>&1 \
  && stellar keys address it-settle || stellar keys address it-settle)
invoke "$TOKEN_ID" it-admin mint --admin "$ADMIN" --to "$EMPLOYER" --amount "$DEPOSIT"

NOW=$(date +%s)
STOP_TIME=$((NOW + WAIT_SECONDS))
SETTLE_ID=$(invoke "$STREAM_ID" it-employer create_stream \
  --employer "$EMPLOYER" --employee "$SETTLE_EMPLOYEE" --token_address "$TOKEN_ID" \
  --deposit "$DEPOSIT" --rate_per_second "$RATE_PER_SECOND" --stop_time "$STOP_TIME" | strip)
log "Settle stream created: $SETTLE_ID (stop_time=$STOP_TIME)"

log "Waiting past stop_time..."
sleep $((WAIT_SECONDS + 5))
SETTLE_WITHDRAWN=$(invoke "$STREAM_ID" it-employee withdraw \
  --employee "$SETTLE_EMPLOYEE" --stream_id "$SETTLE_ID" | strip)
log "Settled withdrawn: $SETTLE_WITHDRAWN"
[[ "$SETTLE_WITHDRAWN" -gt 0 ]] || fail "Settle withdraw returned 0"

SETTLE_STATE=$(invoke "$STREAM_ID" it-admin get_stream --stream_id "$SETTLE_ID" | strip)
log "Settle stream state: $SETTLE_STATE"
echo "$SETTLE_STATE" | grep -qi exhausted || fail "Stream did not transition to Exhausted after stop_time"

SETTLE_CLAIMABLE=$(invoke "$STREAM_ID" it-admin claimable --stream_id "$SETTLE_ID" | strip)
log "Post-settle claimable: $SETTLE_CLAIMABLE"
[[ "$SETTLE_CLAIMABLE" -eq 0 ]] || fail "Accrual continued after settle (claimable=$SETTLE_CLAIMABLE)"

log "✅ Integration test PASSED"
