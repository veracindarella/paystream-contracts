#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# deploy-mainnet.sh — Deploy PayStream contracts to Stellar Mainnet.
#
# USAGE:
#   ./scripts/deploy-mainnet.sh --confirm
#
# The --confirm flag is required to prevent accidental execution.
# Review docs/mainnet-deployment.md for the full pre-flight checklist
# before running this script.
#
# REQUIRED ENVIRONMENT VARIABLES:
#   STELLAR_SOURCE_ACCOUNT  — funded mainnet account key (name in stellar CLI config)
#
# OPTIONAL ENVIRONMENT VARIABLES:
#   STELLAR_RPC_URL         — override the default mainnet RPC endpoint
#   STELLAR_NETWORK_PASSPHRASE — override the default mainnet passphrase

set -euo pipefail

# ---------------------------------------------------------------------------
# Guard: --confirm flag must be passed explicitly
# ---------------------------------------------------------------------------
CONFIRMED=false
for arg in "$@"; do
  if [[ "$arg" == "--confirm" ]]; then
    CONFIRMED=true
  fi
done

if [[ "$CONFIRMED" != "true" ]]; then
  echo "ERROR: Mainnet deployment requires the --confirm flag."
  echo "       Review docs/mainnet-deployment.md before proceeding."
  echo ""
  echo "Usage: $0 --confirm"
  exit 1
fi

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
NETWORK="mainnet"
SOURCE="${STELLAR_SOURCE_ACCOUNT:-}"
RPC_URL="${STELLAR_RPC_URL:-https://soroban-rpc.mainnet.stellar.gateway.fm}"
NETWORK_PASSPHRASE="${STELLAR_NETWORK_PASSPHRASE:-Public Global Stellar Network ; September 2015}"

TOKEN_WASM="target/wasm32v1-none/release/paystream_token.wasm"
STREAM_WASM="target/wasm32v1-none/release/paystream_stream.wasm"

if [[ -z "$SOURCE" ]]; then
  echo "ERROR: STELLAR_SOURCE_ACCOUNT environment variable must be set."
  echo "       Set it to the name of your funded mainnet account in the Stellar CLI config."
  exit 1
fi

echo "============================================================"
echo "  PayStream Mainnet Deployment"
echo "============================================================"
echo "  Network:  $NETWORK"
echo "  RPC URL:  $RPC_URL"
echo "  Source:   $SOURCE"
echo "============================================================"
echo ""
echo "WARNING: You are about to deploy to MAINNET. This action is irreversible."
echo "         Ensure you have completed all items in docs/mainnet-deployment.md"
echo ""

# ---------------------------------------------------------------------------
# Step 1: Build contracts
# ---------------------------------------------------------------------------
echo "[1/5] Building contracts..."
stellar contract build
echo "      Build complete."
echo ""

# Verify WASM artifacts exist
if [[ ! -f "$TOKEN_WASM" ]]; then
  echo "ERROR: Token WASM not found at $TOKEN_WASM"
  exit 1
fi
if [[ ! -f "$STREAM_WASM" ]]; then
  echo "ERROR: Stream WASM not found at $STREAM_WASM"
  exit 1
fi

# ---------------------------------------------------------------------------
# Step 2: Upload WASMs (get hashes for verification)
# ---------------------------------------------------------------------------
echo "[2/5] Uploading token WASM..."
TOKEN_WASM_HASH=$(stellar contract upload \
  --wasm "$TOKEN_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "      Token WASM hash: $TOKEN_WASM_HASH"
echo ""

echo "[2/5] Uploading stream WASM..."
STREAM_WASM_HASH=$(stellar contract upload \
  --wasm "$STREAM_WASM" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "      Stream WASM hash: $STREAM_WASM_HASH"
echo ""

# ---------------------------------------------------------------------------
# Step 3: Deploy contracts
# ---------------------------------------------------------------------------
echo "[3/5] Deploying token contract..."
TOKEN_ID=$(stellar contract deploy \
  --wasm-hash "$TOKEN_WASM_HASH" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "      Token contract ID: $TOKEN_ID"
echo ""

echo "[3/5] Deploying stream contract..."
STREAM_ID=$(stellar contract deploy \
  --wasm-hash "$STREAM_WASM_HASH" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE")
echo "      Stream contract ID: $STREAM_ID"
echo ""

# ---------------------------------------------------------------------------
# Step 4: Verify deployments (read contract via RPC)
# ---------------------------------------------------------------------------
echo "[4/5] Verifying token contract deployment..."
stellar contract invoke \
  --id "$TOKEN_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- total_supply > /dev/null 2>&1 && echo "      Token contract: OK" || {
    echo "ERROR: Token contract verification failed."
    exit 1
  }

echo "[4/5] Verifying stream contract deployment..."
stellar contract invoke \
  --id "$STREAM_ID" \
  --source "$SOURCE" \
  --network "$NETWORK" \
  --rpc-url "$RPC_URL" \
  --network-passphrase "$NETWORK_PASSPHRASE" \
  -- stream_count > /dev/null 2>&1 && echo "      Stream contract: OK" || {
    echo "ERROR: Stream contract verification failed."
    exit 1
  }
echo ""

# ---------------------------------------------------------------------------
# Step 5: Output deployment summary
# ---------------------------------------------------------------------------
echo "[5/5] Deployment complete."
echo ""
echo "============================================================"
echo "  Deployment Summary"
echo "============================================================"
echo "  Token WASM hash:    $TOKEN_WASM_HASH"
echo "  Stream WASM hash:   $STREAM_WASM_HASH"
echo "  Token contract ID:  $TOKEN_ID"
echo "  Stream contract ID: $STREAM_ID"
echo "============================================================"
echo ""
echo "Next steps (see docs/mainnet-deployment.md):"
echo "  1. Initialize contracts with admin multisig key"
echo "  2. Set minimum deposit: stellar contract invoke --id \$STREAM_ID -- set_min_deposit ..."
echo "  3. Verify LOW-02 fix is included in the deployed WASM"
echo "  4. Transfer admin to multisig wallet"
echo ""
echo "Save these values:"
echo "TOKEN_CONTRACT_ID=$TOKEN_ID"
echo "STREAM_CONTRACT_ID=$STREAM_ID"
