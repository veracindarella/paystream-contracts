# Operations Runbook — Contract Admin

Step-by-step procedures for day-to-day administration of the PayStream stream contract. Function details are in the [API reference](api-reference.md).

## Environment

Set these once per shell session:

```bash
export NETWORK=testnet                   # or mainnet
export STREAM_ID=<STREAM_CONTRACT_ID>    # deployed stream contract ID
export ADMIN=<ADMIN_IDENTITY>            # stellar keys identity for the current admin
export ADMIN_ADDRESS=$(stellar keys address "$ADMIN")
```

> Every admin call consumes the current admin nonce. **Always read the nonce immediately before each admin call** and never reuse a value — a stale nonce fails with E009.

## 1. Check admin nonce

```bash
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- admin_nonce
```

Store it for the next call:

```bash
export NONCE=$(stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- admin_nonce)
```

## 2. Pause / unpause the contract

Pausing blocks state-changing user operations (`create_stream`, `withdraw`, …) with E013. Read-only queries still work.

```bash
# Check current state
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- is_paused

# Pause
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- pause_contract --nonce "$NONCE"

# Unpause (re-read the nonce first — it was incremented by pause_contract)
export NONCE=$(stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- admin_nonce)
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- unpause_contract --nonce "$NONCE"
```

## 3. Set minimum deposit

```bash
export MIN_DEPOSIT=<AMOUNT>   # in token base units
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- set_min_deposit --admin "$ADMIN_ADDRESS" --nonce "$NONCE" --amount "$MIN_DEPOSIT"
```

Applies only to streams created afterwards; existing streams are unaffected.

## 4. Admin transfer

Two-step handover. The nonce passed to `propose_admin` must also be passed to `accept_admin`.

```bash
export NEW_ADMIN=<NEW_ADMIN_IDENTITY>
export NEW_ADMIN_ADDRESS=$(stellar keys address "$NEW_ADMIN")

# Step 1 — current admin proposes (note the nonce used)
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- propose_admin --new_admin "$NEW_ADMIN_ADDRESS" --nonce "$NONCE"

# Verify the proposal
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- get_pending_admin

# Step 2 — new admin accepts with the SAME nonce used in step 1
stellar contract invoke --id "$STREAM_ID" --source "$NEW_ADMIN" --network "$NETWORK" \
  -- accept_admin --new_admin "$NEW_ADMIN_ADDRESS" --nonce "$NONCE"
```

After acceptance, update `ADMIN` / `ADMIN_ADDRESS` to the new identity.

## 5. Contract upgrade

Upgrades use a 48-hour timelock (`TIMELOCK_DELAY = 172800` seconds). See the [upgrade guide](upgrade-guide.md) for building and verifying the WASM.

```bash
# Upload the new WASM and capture its hash
export WASM_HASH=$(stellar contract upload --source "$ADMIN" --network "$NETWORK" \
  --wasm target/wasm32v1-none/release/paystream_stream.wasm)

# Step 1 — propose (starts the timelock)
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- propose_upgrade --new_wasm_hash "$WASM_HASH" --nonce "$NONCE"

# Step 2 — after 48 hours, re-read the nonce and execute
export NONCE=$(stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- admin_nonce)
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- execute_upgrade --nonce "$NONCE"

# Abort a pending upgrade instead
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- cancel_upgrade --nonce "$NONCE"

# Confirm
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- version
```

## Common errors and recovery

| Error | Cause | Recovery |
|---|---|---|
| `E009: invalid admin nonce` | Stale or reused nonce (another admin call landed first) | Re-run [Check admin nonce](#1-check-admin-nonce) and retry with the fresh value |
| `E010: no pending admin set` | `accept_admin` called without a proposal | Current admin runs `propose_admin` first |
| `E011: not the pending admin` | `accept_admin` signed by a different address | Sign with the proposed `NEW_ADMIN` identity, or re-propose the correct address |
| `E024: invalid pending admin nonce` | `accept_admin` nonce differs from the one used in `propose_admin` | Use the exact nonce from step 1 of the admin transfer |
| `E012: caller is not the contract admin` | Wrong `--source` / `--admin` | Use the current admin identity; check with `get_pending_admin` whether a transfer is in progress |
| `E013: contract is paused` | User operation attempted while paused | Run `unpause_contract` once the incident is resolved |
| `E021: admin has not been initialised` | Contract not initialized | Call `initialize --admin "$ADMIN_ADDRESS"` |
| Timelock not elapsed on `execute_upgrade` | Called before 48 hours passed | Wait until the timelock expires, then retry with a fresh nonce |
| No pending upgrade on `execute_upgrade` / `cancel_upgrade` | Proposal never submitted or already executed/cancelled | Run `propose_upgrade` again if an upgrade is still needed |
| Auth / signature failure | Missing or wrong signing key | Verify with `stellar keys address "$ADMIN"` and that the account is funded |
