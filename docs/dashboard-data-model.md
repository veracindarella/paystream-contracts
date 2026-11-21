# Employer Dashboard Data Model

This document specifies the data an employer dashboard needs and where each data point comes from, so frontend developers and indexer builders can work in parallel with contract development.

**Legend — Source type**

- **Direct** — a single read-only contract call (simulated, no fee).
- **Derived** — computed client-side from direct reads, no indexer needed.
- **Indexed** — requires an off-chain indexer that ingests contract events (history, time series, cross-stream totals over large sets).

All token amounts are `i128` in the token's smallest unit (e.g. stroops for XLM). Timestamps are ledger seconds (`u64`).

---

## 1. Employer Overview

One card per employer address. Totals are per token — never sum amounts across different `token` addresses.

| Field | Type | Definition | Source | Type of source |
|---|---|---|---|---|
| `employer` | `Address` | Connected wallet | — | — |
| `total_streams` | `u64` | Number of streams created by the employer | `stream_count_by_employer(employer)` | Direct |
| `stream_ids` | `Vec<u64>` | All stream IDs owned by the employer | `stream_ids_by_employer(employer)` | Direct |
| `streams_by_status` | `map<StreamStatus, u64>` | Count of Active / Paused / Cancelled / Exhausted | `get_streams_batch(stream_ids)` → group by `status` | Derived |
| `total_deposited[token]` | `i128` | Σ `deposit` over the employer's streams | `get_streams_batch` → Σ `deposit` | Derived |
| `total_withdrawn[token]` | `i128` | Σ `withdrawn` over the employer's streams | `get_streams_batch` → Σ `withdrawn` | Derived |
| `total_claimable[token]` | `i128` | Σ currently claimable over Active streams | `claimable(id)` per Active stream | Direct (N calls) |
| `total_refunded[token]` | `i128` | Σ refunds returned on cancellation | `cancelled` events → Σ `refund_amount` | Indexed |
| `total_remaining[token]` | `i128` | Unstreamed escrow still locked: Σ (`deposit` − `withdrawn` − `claimable`) over Active/Paused streams | Derived from the rows above | Derived |
| `burn_rate[token]` | `i128` / s | Σ `rate_per_second` over Active streams | `get_streams_batch` → Σ `rate_per_second` | Derived |
| `low_balance_alerts` | `Vec<u64>` | Streams that emitted `low_bal` and have not been topped up since | `low_bal` events minus later `topup` events | Indexed |

> `get_streams_batch` is capped at 50 IDs per call — page through `stream_ids` in chunks of 50.

---

## 2. Per-Stream Summary

One row per stream in the employer's stream table.

| Field | Type | Definition | Source | Type of source |
|---|---|---|---|---|
| `id` | `u64` | Stream ID | `Stream.id` | Direct |
| `employee` | `Address` | Recipient | `Stream.employee` | Direct |
| `token` | `Address` | Token contract (SEP-41 or XLM SAC) | `Stream.token` | Direct |
| `status` | `StreamStatus` | Active / Paused / Cancelled / Exhausted | `Stream.status` or `stream_status(id)` | Direct |
| `deposit` | `i128` | Total deposited incl. top-ups | `Stream.deposit` | Direct |
| `withdrawn` | `i128` | Total paid out to the employee | `Stream.withdrawn` | Direct |
| `claimable` | `i128` | Earned but not yet withdrawn | `claimable(id)` | Direct |
| `remaining` | `i128` | `deposit − withdrawn − claimable` | Derived | Derived |
| `rate_per_second` | `i128` | Current rate | `Stream.rate_per_second` | Direct |
| `start_time` | `u64` | Stream start | `Stream.start_time` | Direct |
| `stop_time` | `u64` | Hard stop (0 = none) | `Stream.stop_time` | Direct |
| `last_withdraw_time` | `u64` | Last withdraw or resume | `Stream.last_withdraw_time` | Direct |
| `low_balance_threshold` | `i128` | Alert threshold (0 = disabled) | `Stream.low_balance_threshold` | Direct |
| `estimated_exhaustion_time` | `u64` | `now + remaining / rate_per_second`, capped by `stop_time` if set; `—` if not Active | Derived | Derived |
| `projected_claimable_at(t)` | `i128` | Claimable at a future timestamp | `claimable_at(id, t)` | Direct |
| `created_at_ledger` | `u32` | Ledger of creation | `created` event | Indexed |
| `last_activity` | `u64` | Most recent event of any kind | Latest event for `id` | Indexed |

**Recommended read path:** `stream_ids_by_employer` → `get_streams_batch` (chunks of 50) → `claimable` for Active streams only (the others are always 0 or fixed).

---

## 3. Time-Series Data

Contract storage keeps only the current state, so all history comes from events and **requires an indexer**.

| Series | Granularity | Built from | Notes |
|---|---|---|---|
| Cumulative deposited per token | per event, bucketed daily | `created` (initial deposit read via `get_stream` at ingest) + `topup` (`amount`) | `created` does not carry the deposit; the indexer reads it once on ingest |
| Cumulative withdrawn per token | per event, bucketed daily | `withdraw` (`amount`) + `cancelled` (`claimable_amount`) | Cancellation pays the employee too |
| Refunds per token | per event | `cancelled` (`refund_amount`) | |
| Active stream count | daily snapshot | `created`, `status` (Paused/Exhausted/Cancelled), `cancelled` | Replay status transitions |
| Burn rate per token | per event | `created` (`rate`), `rate_upd` (`old_rate`, `new_rate`), `status` changes | Only Active streams contribute |
| Per-stream withdrawal history | per event | `withdraw` topics `("withdraw", id)` | Table on the stream detail page |
| Low-balance alerts | per event | `low_bal` (`employer`, `remaining`, `threshold`) | Clear when a `topup` for the same `id` follows |
| Contract pause windows | per event | `paused` (`bool`) | Show a banner; withdrawals blocked while paused |

### Event reference

| Event | Topics | Data | Emitted by |
|---|---|---|---|
| `created` | `("created", id)` | `(employer, employee, rate)` | `create_stream`, `create_streams_batch` |
| `withdraw` | `("withdraw", id)` | `(employee, amount)` | `withdraw`, `withdraw_all` |
| `topup` | `("topup", id)` | `(employer, amount)` | `top_up` |
| `status` | `("status", id)` | `StreamStatus` | pause / resume / cancel / exhaustion / `settle_stream` |
| `cancelled` | `("cancelled", id)` | `(claimable_amount, refund_amount)` | `cancel_stream` |
| `rate_upd` | `("rate_upd", id)` | `(old_rate, new_rate)` | `update_rate` |
| `low_bal` | `("low_bal", id)` | `(employer, remaining, threshold)` | `withdraw` |
| `paused` | `("paused",)` | `bool` | `pause_contract`, `unpause_contract` |

---

## 4. Direct Query vs Off-Chain Aggregation

| Data | Direct contract query | Off-chain aggregation required | Why |
|---|---|---|---|
| Stream list for an employer | ✅ `stream_ids_by_employer` | | On-chain index exists |
| Current state of each stream | ✅ `get_stream` / `get_streams_batch` | | Stored on-chain |
| Current / future claimable | ✅ `claimable`, `claimable_at` | | Computed on-chain |
| Overview totals (deposited, withdrawn, claimable, burn rate) | ✅ via reads + client sum | | Small N; sum client-side. Use an indexer when N is large (≫ 50 streams) |
| Refund totals | | ✅ | Refund amount exists only in `cancelled` events |
| Any historical or time-series chart | | ✅ | Storage keeps only current state |
| Stream creation time / ledger | | ✅ | Only `start_time` is stored; ledger comes from the event |
| Low-balance alert inbox | | ✅ | `low_bal` is event-only |
| Cross-employer / global analytics | | ✅ | No on-chain global aggregates beyond `stream_count` |

---

## 5. Indexer Schema

The indexer persists the data above in a relational store. The schema is defined in the backend crate (`backend/src/db/schema.rs`) and applied through **forward-only, versioned migrations** tracked in the `schema_version` table, so the indexer can evolve without a full resync.

### 5.1 Tables

```text
schema_version(version PK, applied_at)

streams(id PK, employer, employee, token, deposit, withdrawn, rate_per_second,
        start_time, stop_time, status, low_balance_threshold,
        created_ledger, updated_ledger)

beneficiaries(stream_id FK, employee, token, PK(stream_id, employee))

stream_events(id PK, stream_id FK, kind, ledger, timestamp, tx_hash,
              amount, refund_amount, old_rate, new_rate, status, remaining, threshold)

aggregates(bucket PK, employer, employee, token, status, day,
           deposited, withdrawn, refunded, active_streams, burn_rate)

admin_audit(id PK, kind, admin, target_admin, ledger, timestamp, tx_hash,
            nonce, amount, paused, proposal_id)
```

- `streams` is refreshed via `get_streams_batch` on each event touching a stream and holds the current state of each stream.
- `beneficiaries` maps each stream to its recipient so employee-side queries do not need to scan `streams`.
- `stream_events` is an append-only log of the events in §3.
- `aggregates` is a rollup materialised from `stream_events`, keyed by `(employer, employee, token, status, day)`.
- `admin_audit` is an append-only log of governance actions (see §6).

**Claimable is never cached.** `claimable` / `claimable_at` are read from the contract at request time, because they depend on the current ledger. The indexer stores only amounts that are fixed once an event is emitted (`deposit`, `withdrawn`, `refund_amount`, rates).

### 5.2 Indexes and supported query patterns

| Query pattern | Index | Serves |
|---|---|---|
| Streams by employer | `streams(employer)` | Employer overview, stream table |
| Streams by employee | `beneficiaries(employee)`, `streams(employee)` | Employee dashboard |
| Streams by token | `streams(token)` | Per-token totals and charts |
| Streams by status | `streams(status)` | Active / Paused / Cancelled / Exhausted counts |
| Employer + token + status | `streams(employer, token, status)` | Overview cards without a full scan |
| Events for a stream | `stream_events(stream_id, ledger)` | Per-stream history, last activity |
| Events by kind and time | `stream_events(kind, ledger)` | Time-series rollups, alert inbox |
| Aggregates by employer / employee / token / day | `aggregates(employer, token, day)`, `aggregates(employee, token, day)` | Dashboard charts |
| Admin activity by admin address | `admin_audit(admin, ledger)` | Governance audit trail (§6) |
| Admin activity by action kind | `admin_audit(kind, ledger)` | Alerts on unexpected admin actions |

### 5.3 Migrations

Migrations are applied in order at startup; each records its version in `schema_version` and is never rolled back. New columns/tables are added in new migration steps, so an existing indexer database is upgraded in place rather than resynced.

---

## 6. Admin / Governance Audit Trail

Governance tools audit admin activity from the `admin_audit` table, which is populated from the admin events below.

| Event | Topics | Data | Emitted by |
|---|---|---|---|
| `admin_nonce` | `("admin_nonce",)` | `(admin, nonce)` | Any admin action that consumes a nonce |
| `admin_prop` | `("admin_prop",)` | `(admin, proposal_id)` | `propose_admin`, `propose_upgrade` |
| `admin_accept` | `("admin_accept",)` | `(admin, target_admin)` | `accept_admin` |
| `paused` | `("paused",)` | `bool` | `pause_contract`, `unpause_contract` |
| `fee_set` | `("fee_set",)` | `(admin, amount)` | `set_protocol_fee`, `set_min_deposit` |
| `upgrade` | `("upgrade",)` | `(admin, proposal_id)` | Upgrade execution steps |

- **Nonce sequence** is reconstructed by ordering `admin_nonce` rows per `admin` by `ledger`; gaps or out-of-order nonces indicate a missed or replayed action.
- **Audit by admin** is a single indexed lookup: `admin_audit(admin, ledger)` returns the full history for one admin address.
- **Alerts** fire when an admin action occurs that was not preceded by the expected proposal/accept sequence, when a nonce is skipped, or when pause/fee changes happen outside an expected window.
=======
