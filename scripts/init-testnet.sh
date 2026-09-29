#!/usr/bin/env bash
set -euo pipefail
NETWORK="testnet"
SOURCE="${STELLAR_SOURCE_ACCOUNT:-default}"
# STELLAR_ADMIN_ADDRESS can be any Stellar account address, including a native
# multisig account.  For mainnet, set this to a 2-of-3 (or stronger) multisig
# account.  See docs/security/admin-multisig.md for setup instructions (SEC-01).
ADMIN="${STELLAR_ADMIN_ADDRESS:?Set STELLAR_ADMIN_ADDRESS}"
TOKEN_ID="${TOKEN_CONTRACT_ID:?Set TOKEN_CONTRACT_ID}"
STREAM_ID="${STREAM_CONTRACT_ID:?Set STREAM_CONTRACT_ID}"
INITIAL_SUPPLY="${INITIAL_SUPPLY:-1000000000}"

echo "Initialising PayStream contracts on testnet..."

stellar contract invoke --id "$TOKEN_ID" --source "$SOURCE" --network "$NETWORK" \
  -- initialize --admin "$ADMIN" --initial_supply "$INITIAL_SUPPLY"
echo "Token initialised."

stellar contract invoke --id "$STREAM_ID" --source "$SOURCE" --network "$NETWORK" \
  -- initialize --admin "$ADMIN"
echo "Stream contract initialised."

echo "Init complete."
