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

## 5. Suggested Indexer Schema

```text
streams(id PK, employer, employee, token, deposit, withdrawn, rate_per_second,
        start_time, stop_time, status, low_balance_threshold,
        created_ledger, updated_ledger)

stream_events(id PK, stream_id FK, kind, ledger, timestamp, tx_hash,
              amount, refund_amount, old_rate, new_rate, status, remaining, threshold)

employer_daily(employer, token, day, deposited, withdrawn, refunded,
               active_streams, burn_rate, PK(employer, token, day))
```

`streams` is refreshed via `get_streams_batch` on each event touching a stream; `stream_events` is an append-only log of the events above; `employer_daily` is a rollup materialised from `stream_events`.
