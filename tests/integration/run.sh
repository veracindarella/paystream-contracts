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

log "✅ Integration test PASSED"
