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
| `total_claimable[token]` | `i128` | Σ currently claimable over Active streams | `total_claimable_by_employee(employee)` per employee, or `claimable(id)` per Active stream | Direct |
| `total_refunded[token]` | `i128` | Σ refunds returned on cancellation | `cancelled` events → Σ `refund_amount` | Indexed |
| `total_locked[token]` | `i128` | Σ (`deposit` − `withdrawn`) over the employer's Active streams | `total_locked_by_employer(employer)` | Direct |
| `total_remaining[token]` | `i128` | Unstreamed escrow still locked: Σ (`deposit` − `withdrawn` − `claimable`) over Active/Paused streams | Derived from the rows above | Derived |
| `burn_rate[token]` | `i128` / s | Σ `rate_per_second` over Active streams | `get_streams_batch` → Σ `rate_per_second` | Derived |
| `low_balance_alerts` | `Vec<u64>` | Streams that emitted `low_bal` and have not been topped up since | `low_bal` events minus later `topup` events | Indexed |

> `get_streams_batch` is capped at 50 IDs per call — page through `stream_ids` in chunks of 50.
>
> The aggregate queries `total_claimable_by_employee` and `total_locked_by_employer` are read-only and iterate the address's streams on-chain, so they are bounded: they error once an address exceeds the contract's iteration limit. For addresses past that limit, fall back to paging `stream_ids` and summing client-side (or use an indexer).

### Paginated stream listing

For employers with many streams, prefer the paginated query over fetching the full ID list:

```text
streams_by_employee_paginated(employee: Address, start_after: Option<u64>, limit: u32) -> Vec<u64>
```

- Returns up to `limit` stream IDs owned by `employee`, ordered by ascending stream ID (stable ordering — IDs are never reused, so pages cannot shift).
- `start_after` is exclusive: pass the last ID of the previous page (or `None` / `0` for the first page).
- `limit` is capped at 50; larger values are clamped to 50.
- The dashboard hydration loop should paginate until a page returns fewer than `limit` items (i.e. until exhausted), then hydrate each page with `get_streams_batch` in chunks of 50.

```text
let mut cursor = None;
loop {
    let page = streams_by_employee_paginated(employer, cursor, 50);
    if page.is_empty() { break; }
    hydrate(get_streams_batch(page));
    cursor = page.last();
    if page.len() < 50 { break; }
}
```

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
| `claimable` | `i128` | Earned but not yet withdrawn | `claimable(id)` or `stream_summary(id).claimable` | Direct |
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

**Polling read path:** for indexers and dashboards polling many streams, prefer `stream_summary(id)` over `get_stream(id)`. It returns a small struct with only `id`, `status`, `claimable`, `remaining`, and `rate` — cheaper to simulate and decode than the full `Stream`. Use `get_stream` only when fields outside the summary (e.g. `employee`, `token`, `start_time`) are needed.

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
| `allowlist` | `("allowlist",)` | `(token, allowed)` | `set_token_allowlist`, `set_token_allowed` |

### Token allowlist

Admins can restrict which SEP-41 token addresses may be used as the stream token, preventing malicious tokens (e.g. ones that revert on transfer) from griefing the contract or trapping deposits.

| Function | Description |
|---|---|
| `set_token_allowlist(admin, nonce, tokens: Vec<Address>)` | Replaces the allowlist with `tokens` |
| `set_token_allowed(admin, nonce, token: Address, allowed: bool)` | Toggles a single token in the allowlist |
| `is_token_allowed(token: Address) -> bool` | Read-only check |

- **Default is permissive:** an empty allowlist means any token is accepted. Enforcement only kicks in once at least one token is allowlisted.
- `create_stream` and `create_linear_vest` (and batch variants) reject disallowed tokens when the allowlist is enabled.
- The dashboard should surface the allowlist state so employers know which tokens they can stream; changes emit the `allowlist` event above.

---

## 4. Direct Query vs Off-Chain Aggregation

| Data | Direct contract query | Off-chain aggregation required | Why |
|---|---|---|---|
| Stream list for an employer | ✅ `stream_ids_by_employer` | | On-chain index exists |
| Current state of each stream | ✅ `get_stream` / `get_streams_batch` | | Stored on-chain |
| Lightweight per-stream polling (id, status, claimable, remaining, rate) | ✅ `stream_summary` | | Small struct, cheaper than `get_stream` |
| Current / future claimable | ✅ `claimable`, `claimable_at` | | Computed on-chain |
| Employee claimable total | ✅ `total_claimable_by_employee` | | On-chain aggregate; bounded iteration, errors past the limit |
| Employer locked total (deposit − withdrawn, Active) | ✅ `total_locked_by_employer` | | On-chain aggregate; bounded iteration, errors past the limit |
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

employer_daily(employer, token, day, deposited, withdrawn, refunded,
               active_streams, burn_rate, PK(employer, token, day))

ingest_cursor(id PK, mode, start_ledger, last_processed_ledger,
              high_water_mark, status, updated_at)
```

`streams` is refreshed via `get_streams_batch` on each event touching a stream; `stream_events` is an append-only log of the events above; `employer_daily` is a rollup materialised from `stream_events`.

### 5.1 Ingestion Cursor and High-Water Mark

A single `ingest_cursor` row (one per indexer instance / contract) tracks how far event ingestion has progressed. It is the source of truth for resumability.

| Field | Type | Definition |
|---|---|---|
| `id` | `PK` | Indexer instance / contract identifier |
| `mode` | `enum` | `backfill` or `live` (see §5.2) |
| `start_ledger` | `u32` | Configured first ledger to replay (genesis or an operator-chosen ledger) |
| `last_processed_ledger` | `u32` | Highest ledger fully ingested and committed; the resume point |
| `high_water_mark` | `u32` | Latest ledger known to be finalised on-chain; catch-up target |
| `status` | `enum` | `idle`, `running`, `paused`, `failed`, `caught_up` |
| `updated_at` | `u64` | Timestamp of the last cursor update |

The cursor is advanced **only after** the events for a ledger range are written and the derived rollups (`employer_daily`) are committed, so a crash mid-batch leaves `last_processed_ledger` at the last consistent boundary. On restart the indexer reads the cursor and resumes from `last_processed_ledger + 1` — no gaps, no duplicates (writes are keyed by `(tx_hash, event index)` and are idempotent).

### 5.2 Backfill vs Catch-Up Modes

- **Backfill** — replays events from `start_ledger` (genesis, or a configured ledger such as the contract's deployment ledger) up to the current high-water mark. Used for new deployments and for rebuilding history after schema changes. This is what populates all `Indexed` fields in §1–§3 (refund totals, creation ledger, low-balance inbox, and every time series).
- **Catch-up** — resumes from the persisted `last_processed_ledger` and advances toward `high_water_mark`. Used after downtime; it is the same replay loop as backfill but starts at the cursor rather than `start_ledger`.
- **Live** — follows new ledgers as they finalise once `last_processed_ledger == high_water_mark` (status `caught_up`).

**Backfill and live ingestion are mutually exclusive.** Only one mode runs at a time per cursor; the indexer acquires a lock on the `ingest_cursor` row before switching modes. Live ingestion does not start until backfill/catch-up has reached the high-water mark, so the same ledger range is never processed by two modes concurrently and events are not duplicated.

### 5.3 Progress and Resumability

- Progress is observable: `(last_processed_ledger − start_ledger) / (high_water_mark − start_ledger)` gives a completion ratio, and `status` / `updated_at` expose liveness. These can back a dashboard progress bar or health check.
- Progress is resumable: stopping the indexer at any point and restarting it continues from `last_processed_ledger + 1` in the recorded `mode`. An operator can also reset `start_ledger` and re-run backfill to rebuild history from scratch.
- Because `stream_events` is append-only and keyed idempotently, replaying an already-seen ledger range is safe and produces no duplicate rows or double-counted rollups.
