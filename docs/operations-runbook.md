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

### 3.1 Keeper pre-check with `is_stream_expirable`

Before building a batch, query `is_stream_expirable` for each candidate. It is read-only (no nonce, no auth) and returns a tuple of `(eligible: bool, seconds_until_eligible: u64)`. When `eligible` is `true`, `seconds_until_eligible` is `0`; otherwise it is the number of seconds remaining until the stream crosses `stop_time + GRACE_PERIOD`. This lets keepers skip streams that would be silently ignored and avoid wasted admin calls.

```bash
# Read-only: no nonce required
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- is_stream_expirable --stream_id 1
# => [true, 0]        stream is eligible now
# => [false, 3600]    eligible in 3600 seconds
```

Keeper loop:

1. For each tracked stream, call `is_stream_expirable`.
2. Collect IDs where `eligible == true` into a batch.
3. If the batch is empty, do nothing — no nonce is consumed.
4. Otherwise re-read the nonce and run `expire_streams` (below).
5. For ineligible streams, schedule the next check at `now + seconds_until_eligible`.

### 3.2 Batch expiry

```bash
# Re-read the nonce before the admin batch
export NONCE=$(stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" -- admin_nonce)

stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- expire_streams --admin "$ADMIN_ADDRESS" --nonce "$NONCE" --stream_ids '[1,2,3]'
```

Eligible streams are transitioned to `Exhausted`; ineligible IDs are silently skipped. This keeps off-chain indexes accurate without reverting the whole batch.

## 3b. Keeper: find and settle streams nearing expiry

Keepers use the read-only `streams_expiring_before(timestamp)` query to discover streams whose `stop_time` is at or before the given timestamp. It reads from the `StreamsByTimestamp` index, does not mutate state, and returns a **bounded** result set — if more streams are eligible than the cap, only the first batch is returned, so re-run the query after processing to pick up the rest.

```bash
# Unix timestamp (seconds) — e.g. now + 1 hour to catch streams expiring soon
export CUTOFF=$(($(date +%s) + 3600))

stellar contract invoke --id "$STREAM_ID" --source "$KEEPER" --network "$NETWORK" \
  -- streams_expiring_before --timestamp "$CUTOFF"
```

Feed the returned IDs into the appropriate call:

```bash
# Settle a single stream (permissionless once stop_time has passed)
stellar contract invoke --id "$STREAM_ID" --source "$KEEPER" --network "$NETWORK" \
  -- settle_stream --stream_id <STREAM_ID>

# Or batch-expire streams past stop_time + GRACE_PERIOD (admin only — see section 3)
stellar contract invoke --id "$STREAM_ID" --source "$ADMIN" --network "$NETWORK" \
  -- expire_streams --admin "$ADMIN_ADDRESS" --nonce "$NONCE" --stream_ids '[<IDS>]'
```

## 4. Admin transfer
=======

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

## 6. Indexer event ingestion (idempotency & replay protection)

The indexer must never double-count events when a ledger is reprocessed or the process restarts. Ingestion is idempotent and resumable.

### 6.1 Persistence schema

```sql
-- One row per on-chain event. The unique key makes ingestion idempotent.
CREATE TABLE events (
    id               BIGSERIAL PRIMARY KEY,
    contract_id      TEXT    NOT NULL,
    ledger_sequence  BIGINT  NOT NULL,
    event_index      INTEGER NOT NULL,   -- position within the ledger
    payload          JSONB   NOT NULL,
    ingested_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contract_id, ledger_sequence, event_index)
);

-- Monotonic high-water-mark, one row per contract.
CREATE TABLE ingestion_state (
    contract_id           TEXT   PRIMARY KEY,
    last_processed_ledger BIGINT NOT NULL
);
```

Writes use `INSERT ... ON CONFLICT DO NOTHING`, so replaying a ledger is a no-op for events already stored:

```sql
INSERT INTO events (contract_id, ledger_sequence, event_index, payload)
VALUES ($1, $2, $3, $4)
ON CONFLICT (contract_id, ledger_sequence, event_index) DO NOTHING;
```

### 6.2 High-water-mark semantics

- `ingestion_state.last_processed_ledger` is the highest ledger that has been **fully** ingested for the contract.
- It is advanced **monotonically only** — the update never moves it backwards:

  ```sql
  INSERT INTO ingestion_state (contract_id, last_processed_ledger)
  VALUES ($1, $2)
  ON CONFLICT (contract_id) DO UPDATE
    SET last_processed_ledger = GREATEST(ingestion_state.last_processed_ledger, EXCLUDED.last_processed_ledger);
  ```

- The event insert and the high-water-mark update for a ledger run in the **same transaction**, so the mark never claims a ledger whose events were not persisted.

### 6.3 Dedupe buffer (out-of-order delivery)

Before any DB write, events pass through an in-memory buffer keyed by `contract_id:ledger_sequence:event_index` (bounded LRU set). This absorbs duplicate and out-of-order delivery within a batch window. The DB unique constraint remains the source of truth; the buffer is only a fast pre-filter and may be safely dropped on restart.

### 6.4 Restart / resume behavior

On startup the indexer reads the persisted high-water-mark:

- If a row exists for the contract, ingestion resumes at `last_processed_ledger + 1`.
- If no row exists (first run), ingestion starts at the configured start ledger.

Because reprocessing is idempotent, resuming from `last_processed_ledger` (rather than `+ 1`) is also safe; the overlap is deduplicated by the unique constraint.

```bash
# Inspect progress
psql "$DATABASE_URL" -c "SELECT * FROM ingestion_state;"

# Verify no duplicates exist for a ledger
psql "$DATABASE_URL" -c \
  "SELECT ledger_sequence, event_index, count(*) FROM events
   GROUP BY 1,2 HAVING count(*) > 1;"
```

### 6.5 Ingestion tests

The indexer test suite covers:

- **Reprocess same ledger** — ingesting a ledger twice inserts each event once.
- **Restart mid-stream** — a fresh process resumes from the persisted high-water-mark with no loss.
- **Out-of-order delivery** — events delivered out of order are stored once and the high-water-mark advances monotonically.

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
