#!/usr/bin/env bash
set -euo pipefail
NETWORK="local"
SOURCE="default"

# When deploying against the Docker sandbox started via `docker compose up sandbox`,
# override the RPC URL and network passphrase:
#   RPC_URL=http://localhost:8000/soroban/rpc \
#   NETWORK_PASSPHRASE="Standalone Network ; February 2017" \
#   ./scripts/deploy-local.sh
RPC_ARGS=()
if [[ -n "${RPC_URL:-}" ]]; then
  RPC_ARGS+=(--rpc-url "$RPC_URL")
fi
if [[ -n "${NETWORK_PASSPHRASE:-}" ]]; then
  RPC_ARGS+=(--network-passphrase "$NETWORK_PASSPHRASE")
fi

echo "Deploying PayStream contracts to local network..."

TOKEN_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/paystream_token.wasm \
  --source "$SOURCE" --network "$NETWORK" \
  "${RPC_ARGS[@]}")
echo "Token:  $TOKEN_ID"

STREAM_ID=$(stellar contract deploy \
  --wasm target/wasm32v1-none/release/paystream_stream.wasm \
  --source "$SOURCE" --network "$NETWORK" \
  "${RPC_ARGS[@]}")
echo "Stream: $STREAM_ID"

echo "TOKEN_CONTRACT_ID=$TOKEN_ID"
echo "STREAM_CONTRACT_ID=$STREAM_ID"
