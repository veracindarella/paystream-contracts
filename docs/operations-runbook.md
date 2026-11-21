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

## 6. Operations endpoints (health, readiness, metrics)

The indexer/API service exposes three unauthenticated HTTP endpoints for monitoring. They are served by the same process as the API; no admin credentials are required.

| Endpoint | Purpose | Success | Failure |
|---|---|---|---|
| `GET /healthz` | Process liveness | `200` while the process is up | No response / connection refused |
| `GET /readyz` | Readiness: DB connectivity **and** indexer lag | `200` when DB reachable and lag ≤ threshold | `503` with a JSON body naming the failing check |
| `GET /metrics` | Prometheus exposition format | `200` | — |

### 6.1 `/healthz` — liveness

Reports only that the process is running and its event loop is responsive. It does **not** touch the database or chain, so a `200` here means "do not restart the pod", not "safe to send traffic".

```bash
curl -fsS http://<indexer-host>:<port>/healthz
```

Wire this to the orchestrator liveness probe (restart on repeated failure).

### 6.2 `/readyz` — readiness

Runs two checks and returns `503` if either fails:

1. **DB connectivity** — a `SELECT 1` against the indexer database.
2. **Indexer lag** — `latest_chain_ledger - last_indexed_ledger` must be ≤ `INDEXER_MAX_LAG_LEDGERS` (default `50`). Lag is computed from the ledger cursor persisted by the indexer.

```bash
curl -fsS http://<indexer-host>:<port>/readyz
# 200 {"status":"ok","db":"ok","lag_ledgers":3}
# 503 {"status":"degraded","db":"ok","lag_ledgers":412,"max_lag_ledgers":50}
```

Wire this to the readiness probe so traffic is drained while the indexer catches up. A `503` here is expected during backfills and after a chain restart — do not page on readiness alone; page on the lag alert below.

### 6.3 `/metrics` — Prometheus

Scrape `/metrics` from the OPS Prometheus instance. Exported metrics relevant to the indexer:

| Metric | Type | Description |
|---|---|---|
| `paystream_indexer_chain_lag_ledgers` | gauge | Ledgers between the chain head and the last indexed ledger |
| `paystream_indexer_events_total` | counter | Events processed, labelled by `contract` and `event_type` |
| `paystream_indexer_event_errors_total` | counter | Events that failed to decode/apply, labelled by `reason` |
| `paystream_indexer_event_processing_seconds` | histogram | Per-batch event processing latency |
| `paystream_indexer_db_up` | gauge | `1` if the DB check passes, `0` otherwise |

Event throughput is `rate(paystream_indexer_events_total[5m])`; error rate is `rate(paystream_indexer_event_errors_total[5m]) / rate(paystream_indexer_events_total[5m])`.

### 6.4 Lag alert (wired to OPS monitoring)

The lag alert is defined in the OPS Prometheus rules and routed to the OPS on-call channel. It fires when the indexer falls behind the chain head for a sustained period:

```yaml
groups:
  - name: paystream-indexer
    rules:
      - alert: PayStreamIndexerLagging
        expr: paystream_indexer_chain_lag_ledgers > 50
        for: 5m
        labels:
          severity: warning
          team: ops
        annotations:
          summary: "Indexer is {{ $value }} ledgers behind chain head"
          runbook: "docs/operations-runbook.md#64-lag-alert-wired-to-ops-monitoring"
      - alert: PayStreamIndexerDown
        expr: up{job="paystream-indexer"} == 0
        for: 2m
        labels:
          severity: critical
          team: ops
```

**Wiring:** the rule file lives in the OPS monitoring repo and is deployed with the Prometheus config; `team: ops` routes to the OPS on-call rotation via Alertmanager. Keep the threshold in sync with `INDEXER_MAX_LAG_LEDGERS` — if you change one, change the other in the same PR.

**Response:** on `PayStreamIndexerLagging`, check `/readyz` and the `paystream_indexer_event_errors_total` rate. A rising error rate usually means a contract event shape changed or a decode bug — see the error-rate panel before restarting. On `PayStreamIndexerDown`, check `/healthz` and the process logs.

### 6.5 Alerting on unexpected admin actions

Admin/governance activity (admin nonce changes, `propose_admin`/`accept_admin`, pause/unpause, `set_min_deposit`, `set_protocol_fee`, and upgrade propose/execute/cancel) is indexed into the `admin_audit` table. Each row records the admin address, action, nonce, ledger, and tx hash, so the full audit trail is queryable by admin address:

```sql
SELECT ledger, action, nonce, tx_hash
FROM admin_audit
WHERE admin = '<ADMIN_ADDRESS>'
ORDER BY ledger DESC;
```

Alert on admin actions that were not expected. The OPS monitoring job polls `admin_audit` for rows newer than the last check and raises an alert for any action outside an approved change window:

- Any `pause_contract` / `unpause_contract` outside a declared incident window
- Any `propose_admin` / `accept_admin` not matching a scheduled handover
- Any `propose_upgrade` / `execute_upgrade` without a matching approved release tag
- Any `set_protocol_fee` / `set_min_deposit` change

Route these to the security on-call channel with `severity: critical`. On alert, confirm the action against the change log and the tx hash in `admin_audit`; if unplanned, follow the incident procedure and, for a rogue admin transfer, re-propose the correct admin immediately (see [Admin transfer](#4-admin-transfer)).

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
