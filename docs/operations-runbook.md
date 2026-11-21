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

## 0. Mainnet deployment

Mainnet deploys are **never automatic**. The release workflow only runs a dry-run check against the mainnet RPC on tag push; the real deploy is a separate manual `workflow_dispatch`.

### 0.1 Automatic dry-run (on tag push)

The `mainnet-dry-run` job runs after `release` and executes `stellar contract deploy --dry-run` against the mainnet RPC. It fails the workflow if the dry-run returns a non-zero exit code, catching misconfigured environment variables before any real deploy.

### 0.2 Manual mainnet deploy

Trigger the manual deploy workflow from the Actions tab (or via `gh`):

```bash
gh workflow run deploy-mainnet.yml --ref <TAG>
```

Before dispatching, confirm:

- [ ] The `mainnet-dry-run` job passed on the target tag
- [ ] `MAINNET_RPC_URL`, `MAINNET_NETWORK_PASSPHRASE`, and the deployer identity secrets are set
- [ ] The WASM hash matches the audited build

Then deploy and verify:

```bash
export NETWORK=mainnet
export STREAM_ID=<STREAM_CONTRACT_ID>

stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- version
```

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

## 3. Expire stale streams

Use admin batch expiry when streams have crossed `stop_time + GRACE_PERIOD` and the employee has already exhausted any remaining claimable balance. The admin nonce is consumed even if no streams are eligible.

```bash
# Re-read the nonce before the admin batch
export NONCE=$(stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- admin_nonce)

stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- expire_streams --admin "$ADMIN_ADDRESS" --nonce "$NONCE" --stream_ids '[1,2,3]'
```

Eligible streams are transitioned to `Exhausted`; ineligible IDs are silently skipped. This keeps off-chain indexes accurate without reverting the whole batch.

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

## 6. Indexer ledger catch-up and historical backfill

The indexer runs in exactly one ingestion mode at a time: **live**, **catch-up**, or **backfill**. Modes never overlap, so the same ledger range is never ingested twice.

### 6.1 Configuration

```bash
export NETWORK=testnet                  # selects the network profile (see §7)
export BACKFILL_START_LEDGER=0          # 0 = genesis; set to a ledger number to start later
```

- `BACKFILL_START_LEDGER=0` replays from genesis.
- Any other value replays from that ledger (inclusive).
- The persisted high-water mark (last successfully ingested ledger) is stored by the indexer and survives restarts.

### 6.2 Backfill (from genesis or a configured start ledger)

```bash
paystream-indexer backfill --start-ledger "$BACKFILL_START_LEDGER"
```

Backfill ignores the high-water mark and replays the configured range. Use it for new deployments or after a long downtime where the cursor is missing or known to be behind.

### 6.3 Catch-up (resume from high-water mark)

```bash
paystream-indexer catch-up
```

Catch-up resumes from the persisted high-water mark and replays forward until it reaches the current ledger, then hands off to live mode. If no cursor exists, it falls back to `BACKFILL_START_LEDGER`.

### 6.4 Observing progress

The indexer logs the current mode and cursor on every batch:

```
mode=catch-up cursor=1234567 target=1234999
mode=catch-up cursor=1234999 target=1234999 complete
```

- `cursor` is the last successfully ingested ledger.
- `target` is the current network ledger (or the backfill end).
- `complete` is logged when the range is exhausted and the indexer switches to live mode.

### 6.5 Resuming an interrupted run

If the indexer stops mid-run, restart it with the same command. The cursor is only advanced **after** a batch is successfully ingested, so an interrupted run resumes from the last committed ledger with no gaps and no duplicates. Re-running `backfill` with the same `BACKFILL_START_LEDGER` is safe and idempotent.

### 6.6 Mode exclusivity

Only one ingestion mode runs at a time. Starting `backfill` or `catch-up` while live ingestion is active is refused; stop the live indexer first. This guarantees backfill and live ingestion never overlap or duplicate events.

## 7. Switching networks

The same stack runs against testnet or mainnet with no code changes. Select the network with `NETWORK`; the passphrase, RPC URL, and contract IDs are resolved per network:

```bash
export NETWORK=mainnet
export NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
export RPC_URL=https://mainnet.sorobanrpc.com
export STREAM_CONTRACT_ID=<MAINNET_STREAM_CONTRACT_ID>
```

For testnet:

```bash
export NETWORK=testnet
export NETWORK_PASSPHRASE="Test SDF Network ; September 2015"
export RPC_URL=https://soroban-testnet.stellar.org
export STREAM_CONTRACT_ID=<TESTNET_STREAM_CONTRACT_ID>
```

- Both the indexer and the API **refuse to start** if `NETWORK_PASSPHRASE` does not match the selected `NETWORK` profile.
- Contract IDs are validated against the selected network; an ID that does not exist on that network fails startup.
- Switching networks requires a fresh indexer cursor for that network — run [backfill](#62-backfill-from-genesis-or-a-configured-start-ledger) on first use.

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
