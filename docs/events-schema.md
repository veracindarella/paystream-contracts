# PayStream Event Schema

This document describes the structured event schema for PayStream smart contracts. Off-chain indexers can use this schema to parse Soroban events correctly and detect schema changes across contract upgrades.

## Changelog

Indexer-facing record of event schema changes. **Process:** any PR that adds, renames or removes an event, topic or data field must add an entry here (and bump the schema version for breaking changes).

Each entry lists the event, the change (added field / renamed topic / removed field) and migration steps for indexers.

### v1.2 (2026-10-11) — delegate withdrawals, schedules and immutable streams

| Event | Change | Migration |
|-------|--------|-----------|
| `withdraw_to` | Added event `["v1", "withdraw_to", <stream_id>]` with data `[employee, to, amount]` | Indexers tracking withdrawals should also handle `withdraw_to`; the holder's claimable decrements exactly as with `withdraw` |
| `rate_schedule_set` | Added event `["v1", "rate_sched", <stream_id>]` with data `[schedule]` (`Vec<(u64, i128)>`) | Indexers may ignore this event; the active rate still advances via `rate_upd` at each scheduled timestamp |
| `milestone_schedule_set` | Added event `["v1", "ms_sched", <stream_id>]` with data `[schedule]` (`Vec<(u64, u32)>`, fractions in basis points) | Indexers may ignore this event; claimable amounts are derived from the schedule |
| `stream_immutable` | Added event `["v1", "immutable", <stream_id>]` with data `[immutable]` (`bool`) | Indexers should treat streams with `immutable == true` as non-cancellable and non-pausable |

### v1.1 (2026-10-04) — stream metadata and split vest streams

| Event | Change | Migration |
|-------|--------|-----------|
| `cancelled` | Added optional `reason` (Symbol) at data[2], carrying a standard `CancelReason` value. Advisory only — not enforced on-chain | Treat missing `reason` (events before v1.1) as `"other"` |
| `metadata_updated` | Added event `["v1", "meta_upd", <stream_id>]` with data `[metadata]` (`Map<Symbol, Symbol>`, bounded size) | Indexers that do not need metadata may ignore this event; treat streams without it as having empty metadata |
| `split_vest_created` | Added event `["v1", "split_vest", <stream_id>]` with data `[employer, employee, rate, beneficiary, beneficiary_bps]` | Indexers that only handle `created` should also handle `split_vest` to track linear-vest split streams |

### v1.0 (2026-09-27) — baseline

| Event | Change | Migration |
|-------|--------|-----------|
| `init`, `created`, `withdraw`, `status`, `cancelled`, `topup`, `paused`, `rate_upd`, `stop_upd`, `near_exp`, `exp_unres`, `upg_prop`, `upg_exec`, `upg_cncl`, `at_set`, `at_up`, `thresh_up`, `split`, `p_stats`, `cliff` | Baseline: topic prefixed with `"v1"` | None. Match on `topic[0] == "v1"`, then `topic[1]` |
| `receipt`, `archived`, `expired`, `xfr_strm`, `low_bal` | Baseline: emitted **without** a version prefix | Match on `topic[0]` directly. A future version will add the `"v1"` prefix and list it here |
| `transfer`, `approve`, `burn`, `mint` (token) | Baseline: token contract events, emitted **without** a version prefix | Match on `topic[0]` directly |

### v1.1 (2026-10-04) — batch threshold setter & cliff parity

| Event | Change | Migration |
|-------|--------|-----------|
| `thresh_up` | Clarified: emitted once per stream by both `set_low_balance_threshold` and the new `set_low_balance_threshold_batch` (1..=50 streams, all-or-nothing) | No topic/data change. Indexers may see multiple `thresh_up` events per transaction; key on `topic[2]` (stream_id) |
| `cliff` | Clarified: `LinearVest` streams with a cliff emit `cliff_reached` once at the cliff boundary and accrue from the cliff, matching payroll streams | No topic/data change. No migration required |

### v1.1 (2026-10-04) — batch expire summary

| Event | Change | Migration |
|-------|--------|-----------|
| `exp_batch` | Added event `streams_expired_batch` (topic `["v1", "exp_batch"]`, data `[expired: u32, skipped: u32]`) | Additive: no existing event, topic or data field changed. Indexers that do not handle `exp_batch` can ignore it; per-stream `expired` events are unchanged |

<!-- Template for new entries:
### vX.Y (YYYY-MM-DD)
| Event | Change | Migration |
|-------|--------|-----------|
| `name` | Added field `foo` (type) at data[n] | Treat missing `foo` as default for events before vX.Y |
-->

## Schema Version

Current schema version: **v1**

Stream-contract event topics include the schema version as the first component to enable version detection and compatibility handling. A small set of legacy stream events and all token-contract events are emitted **without** a version prefix (see the changelog above).

### Contract Event Version (instance value)

In addition to the per-topic prefix, the contract stores a **contract event version** as an instance value under the key `contract_event_version` (`u32`). It is set on `initialize` and re-stamped on `migrate`, so indexers can read the current schema version directly from contract state without replaying history or resyncing.

- **Key**: `contract_event_version` (instance storage)
- **Type**: `u32`
- **Current value**: `1`
- **Written by**: `initialize`, `migrate`
- **Read by indexers**: via the `get_contract_event_version` view, or by reading the instance ledger entry
- **Legacy contracts**: if the value is absent (deployed before this stamp), assume `1`

When the event schema changes in a breaking way, this value is bumped together with the topic prefix (e.g. `"v2"`), and the change is recorded in the changelog above.

## Event Format

Soroban events have two parts:
- **Topic**: An array that identifies the event type and associated keys
- **Data**: The event payload values

Versioned stream events follow this pattern:
```
[<schema_version>, <event_name>, ...event_specific_keys]
```

Unversioned events (legacy stream events and token events) follow:
```
[<event_name>, ...event_specific_keys]
```

## Completeness

This document is the source of truth for the event surface. Every event emitted in
`contracts/stream/src/events.rs` and `contracts/token/src/events.rs` must have an entry
below. Each entry is keyed by the **exact topic name** (the string literal used in code)
so a CI lint (BE-15) can extract emitted topics from the source and diff them against the
`### <topic>` headings in this file.

Rules for the lint:
- One `### <topic>` heading per emitted topic name.
- The heading text must equal the topic string literal in code (e.g. `### rate_upd`).
- Each entry must declare `**Topic**` and `**Data**` lines.

## Events

### init

Emitted when the contract is initialized with an admin address.

**Topic**: `["v1", "init"]`

**Data**: 
- `admin` (Address): The contract admin address

**Example**:
```json
{
  "topic": ["v1", "init"],
  "data": ["GABCD..."]
}
```

---

### created

Emitted when a new salary stream is created.

**Topic**: `["v1", "created", <stream_id>]`

**Data**:
- `employer` (Address): The employer address
- `employee` (Address): The employee address
- `rate` (i128): Tokens streamed per second

**Example**:
```json
{
  "topic": ["v1", "created", 1],
  "data": ["GABCD...", "GEFGH...", 1000000]
}
```

---

### withdraw

Emitted when an employee withdraws tokens from a stream.

**Topic**: `["v1", "withdraw", <stream_id>]`

**Data**:
- `employee` (Address): The employee address
- `amount` (i128): Amount withdrawn

**Example**:
```json
{
  "topic": ["v1", "withdraw", 1],
  "data": ["GEFGH...", 500000]
}
```

---

### withdraw_to

Emitted when the receipt holder withdraws tokens directly to a different
recipient address (#448). The holder's claimable decrements exactly as with
`withdraw`; the tokens are transferred to `to`.

**Topic**: `["v1", "withdraw_to", <stream_id>]`

**Data**:
- `employee` (Address): The receipt holder authorizing the withdrawal
- `to` (Address): The recipient of the withdrawn tokens
- `amount` (i128): Amount withdrawn

**Example**:
```json
{
  "topic": ["v1", "withdraw_to", 1],
  "data": ["GEFGH...", "GSVGS...", 500000]
}
```

---

### rate_schedule_set

Emitted when a forward rate schedule is set on a stream via
`create_stream` / `update_rate_schedule` (#447). The schedule is sorted by
timestamp and validated for gaps/overlaps.

**Topic**: `["v1", "rate_sched", <stream_id>]`

**Data**:
- `schedule` (Vec<(u64, i128)>): Sorted `(timestamp, rate_per_second)` pairs

**Example**:
```json
{
  "topic": ["v1", "rate_sched", 1],
  "data": [[1767225600, 1000000], [1769904000, 2000000]]
}
```

---

### milestone_schedule_set

Emitted when a milestone vesting schedule is set on a stream (#446). Each
entry vests a fraction of the total at the given timestamp.

**Topic**: `["v1", "ms_sched", <stream_id>]`

**Data**:
- `schedule` (Vec<(u64, u32)>): Sorted `(timestamp, fraction_bps)` pairs;
  fractions are in basis points (0–10000) and sum to at most 10000

**Example**:
```json
{
  "topic": ["v1", "ms_sched", 1],
  "data": [[1767225600, 2500], [1782864000, 5000]]
}
```

---

### stream_immutable

Emitted when a stream is created with the immutable flag (#445). Immutable
streams cannot be cancelled or paused by the employer; funds remain
withdrawable by the employee and top-ups are still allowed.

**Topic**: `["v1", "immutable", <stream_id>]`

**Data**:
- `immutable` (bool): `true` when the stream is locked against cancel/pause

**Example**:
```json
{
  "topic": ["v1", "immutable", 1],
  "data": true
}
```

---

### stream_status_changed

Emitted when a stream's status changes (Active, Paused, Cancelled, Exhausted).

**Topic**: `["v1", "status", <stream_id>]`

**Data**:
- `status` (StreamStatus enum): The new status
  - `0`: Active
  - `1`: Paused
  - `2`: Cancelled
  - `3`: Exhausted

**Example**:
```json
{
  "topic": ["v1", "status", 1],
  "data": [1]
}
```

---

### stream_metadata_updated

Emitted when a stream's structured metadata map is set or replaced via
`update_stream_metadata` (#467). The map is capped in size by the contract;
events are only emitted for accepted (within-cap) updates.

**Topic**: `["v1", "meta_upd", <stream_id>]`

**Data**:
- `metadata` (Map<Symbol, Symbol>): The full metadata map after the update
  (e.g. `department`, `cost_center`). An empty map clears the metadata.

**Example**:
```json
{
  "topic": ["v1", "meta_upd", 1],
  "data": [{"department": "eng", "cost_center": "cc-42"}]
}
```

---

### split_vest_created

Emitted when a linear-vest stream is created with a split beneficiary
(#482). Claimable amounts are split between the employee and the
beneficiary according to `beneficiary_bps`.

**Topic**: `["v1", "split_vest", <stream_id>]`

**Data**:
- `employer` (Address): The employer address
- `employee` (Address): The employee address
- `rate` (i128): Tokens vested per second
- `beneficiary` (Address): The split beneficiary address
- `beneficiary_bps` (u32): Beneficiary share in basis points (0–10000)

**Example**:
```json
{
  "topic": ["v1", "split_vest", 1],
  "data": ["GABCD...", "GEFGH...", 1000000, "GIJKL...", 2500]
}
```

---

### stream_cancelled

Emitted when a stream is cancelled, recording the exact cash-flow split.

**Topic**: `["v1", "cancelled", <stream_id>]`

**Data**:
- `claimable_amount` (i128): Tokens paid to the employee at cancellation time
- `refund_amount` (i128): Tokens returned to the employer (unstreamed portion)
- `fee_refund` (i128): Protocol fee refunded to the employer, proportional to the
  unstreamed deposit (SC-68). `0` when no fee was charged or nothing was unstreamed.

**Example**:
```json
{
  "topic": ["v1", "cancelled", 1],
  "data": [300000, 700000, 3500]
}
```

#### CancelReason (standard set)

`cancel_stream` accepts an optional `Symbol` reason for reporting consistency. These values are a documented convention only; the contract stores and emits whatever symbol is passed (or none) and performs **no on-chain enforcement**.

| Symbol | Meaning |
|--------|---------|
| `role_ended` | Employment/engagement ended normally |
| `dispute` | Cancelled due to a disagreement between parties |
| `error` | Cancelled to correct a mistaken stream configuration |
| `other` | Any other reason (also the assumed value when no reason is emitted) |

Indexers should map unknown symbols to `other` rather than dropping the event.

---

### topup

Emitted when an employer adds more funds to an existing stream.

**Topic**: `["v1", "topup", <stream_id>]`

**Data**:
- `employer` (Address): The employer address
- `amount` (i128): Amount added to the stream

**Example**:
```json
{
  "topic": ["v1", "topup", 1],
  "data": ["GABCD...", 200000]
}
```

---

### at_set

Emitted by `set_auto_topup` (`enabled = true`) and `cancel_auto_topup` (`enabled = false`).

**Topic**: `["v1", "at_set", <stream_id>]`

**Data**:
- `enabled` (bool): `true` when configured, `false` when cancelled

**Example**:
```json
{
  "topic": ["v1", "at_set", 1],
  "data": true
}
```

---

### at_up

Emitted after a `withdraw` / `withdraw_all` when an auto top-up successfully pulls funds from the employer.

**Topic**: `["v1", "at_up", <stream_id>]`

**Data**:
- `amount` (i128): Amount added to the stream by this top-up
- `total_topped_up` (i128): Cumulative auto top-up total after this top-up

**Example**:
```json
{
  "topic": ["v1", "at_up", 1],
  "data": [500000, 500000]
}
```

---

### paused

Emitted when the contract-wide pause flag is toggled.

**Topic**: `["v1", "paused"]`

**Data**:
- `paused` (bool): Whether the contract is paused

**Example**:
```json
{
  "topic": ["v1", "paused"],
  "data": [true]
}
```

---

### rate_upd

Emitted when a stream's rate_per_second is changed.

**Topic**: `["v1", "rate_upd", <stream_id>]`

**Data**:
- `old_rate` (i128): Previous rate per second
- `new_rate` (i128): New rate per second

**Example**:
```json
{
  "topic": ["v1", "rate_upd", 1],
  "data": [1000000, 2000000]
}
```

---

### stop_time_updated

Emitted when a stream's stop time is changed via `update_stop_time` or
`update_stop_time_batch` (SC-63). The batch entrypoint emits one event per
stream in the batch; the batch is all-or-nothing, so either every stream in
the batch emits this event or none do.

**Topic**: `["v1", "stop_upd", <stream_id>]`

**Data**:
- `stop_time` (u64): The new stop time (0 clears the stop time)

**Example**:
```json
{
  "topic": ["v1", "stop_upd", 1],
  "data": [1767225600]
}
```

---

### cliff_reached

Emitted once per stream the first time it is touched (`withdraw`,
`withdraw_all`, `cancel_stream`, `settle_stream`) at or after its cliff
(PROD-17). Streams without a cliff never emit it.

This applies to **both** payroll streams and `LinearVest` streams: a
`LinearVest` schedule configured with a cliff emits `cliff_reached` exactly
once, when the cliff boundary is reached, and accrual starts from the cliff
timestamp (nothing accrues before the cliff). This matches payroll stream
behaviour — there is no difference between the two stream kinds.

**Topic**: `["v1", "cliff", <stream_id>]`

**Data**:
- `cliff_time` (u64): The stream's cliff timestamp

**Example**:
```json
{
  "topic": ["v1", "cliff", 7],
  "data": 1767225600
}
```

---

### low_balance_threshold_updated

Emitted once per stream whenever its low-balance alert threshold is changed,
by either `set_low_balance_threshold` (single stream) or
`set_low_balance_threshold_batch` (1..=50 streams in one transaction). The
batch entrypoint is all-or-nothing: if any stream fails validation (e.g. the
stream is terminal or not owned by the employer) no event is emitted for any
stream. On success, one event is emitted per stream, in input order.

**Topic**: `["v1", "thresh_up", <stream_id>]`

**Data**:
- `threshold` (i128): The new low-balance threshold for the stream

**Example**:
```json
{
  "topic": ["v1", "thresh_up", 1],
  "data": [500000]
}
```

---

### upgrade_proposed

Emitted when a contract upgrade is proposed.

**Topic**: `["v1", "upg_prop"]`

**Data**:
- `wasm_hash` (BytesN<32>): The hash of the new WASM blob
- `unlock_time` (u64): Timestamp when the upgrade can be executed

**Example**:
```json
{
  "topic": ["v1", "upg_prop"],
  "data": ["abcd1234...", 1234567890]
}
```

---

### upg_exec

Emitted when a contract upgrade is executed.

**Topic**: `["v1", "upg_exec"]`

**Data**:
- `wasm_hash` (BytesN<32>): The hash of the executed WASM blob

**Example**:
```json
{
  "topic": ["v1", "upg_exec"],
  "data": ["abcd1234..."]
}
```

---

### upg_cncl

Emitted when a pending contract upgrade is cancelled.

**Topic**: `["v1", "upg_cncl"]`

**Data**: (empty)

**Example**:
```json
{
  "topic": ["v1", "upg_cncl"],
  "data": []
}
```

---

### event_version_stamped

Emitted when the contract event version is stamped on `initialize` or re-stamped on `migrate`.

**Topic**: `["v1", "evt_ver"]`

**Data**:
- `version` (u32): The contract event version now stored in instance storage

**Example**:
```json
{
  "topic": ["v1", "evt_ver"],
  "data": [1]
}
```

---

### migrate_progress

Emitted by `migrate` while backfilling indexes for legacy streams. Bounded per call; emitted once per invocation with the progress after that call.

**Topic**: `["v1", "mig_prog"]`

**Data**:
- `done` (u32): Number of legacy streams backfilled so far
- `total` (u32): Total number of legacy streams to backfill

**Example**:
```json
{
  "topic": ["v1", "mig_prog"],
  "data": [50, 200]
}
```

## Type Reference

| Type | Description | Example |
|------|-------------|---------|
| Address | Stellar address (32 bytes) | `GABCD...` |
| i128 | Signed 128-bit integer | `1000000` |
| u64 | Unsigned 64-bit integer | `1234567890` |
| u32 | Unsigned 32-bit integer | `123456` |
| bool | Boolean | `true` or `false` |
| BytesN<32> | Fixed 32-byte array | `abcd1234...` |
| StreamStatus enum | Stream status (0-3) | `0`, `1`, `2`, or `3` |
| Vec<(u64, i128)> | Sorted timestamp/rate pairs | `[[1767225600, 1000000]]` |
| Vec<(u64, u32)> | Sorted timestamp/fraction (bps) pairs | `[[1767225600, 2500]]` |

## Indexer Compatibility Guide

### Detecting Schema Version

To detect the schema version from an event topic:

1. Parse the first element of the topic array
2. If it matches `"v1"`, use the v1 schema
3. If it's an unknown version, either:
   - Fail gracefully and log a warning
   - Implement a migration path if you have the schema documentation for that version

### Handling Schema Changes

When a new schema version is released:

1. Update your indexer to support the new version
2. Keep support for old versions to parse historical events
3. Use the version prefix to route events to the correct parser
4. Monitor for unknown versions and alert for schema updates

### Example Pseudocode

```javascript
function parseEvent(topic, data) {
  const version = topic[0];
  
  switch (version) {
    case "v1":
      return parseV1Event(topic, data);
    case "v2":
      return parseV2Event(topic, data);
    default:
      console.warn(`Unknown schema version: ${version}`);
      return null;
  }
}

function parseV1Event(topic, data) {
  const eventName = topic[1];
  
  switch (eventName) {
    case "created":
      return {
        type: "created",
        streamId: topic[2],
        employer: data[0],
        employee: data[1],
        rate: data[2]
      };
    // ... other events
  }
}

function parseUnversionedEvent(topic, data) {
  const eventName = topic[0];

  switch (eventName) {
    case "receipt":
      return { type: "receipt", streamId: topic[1], employee: data[0], amount: data[1] };
    case "transfer":
      return { type: "transfer", from: topic[1], to: topic[2], amount: data[0] };
    // ... other unversioned events
  }
}
```

## Indexer Implementation

The off-chain indexer lives in `backend/indexer` (see its `README.md`). It
polls the Soroban RPC `getEvents` endpoint (falling back to Horizon
`/events?type=contract`) for the configured stream contract, starting at
`START_LEDGER`, and persists decoded events to Postgres idempotently on
`(contract_id, ledger_sequence, event_index)`.

### Event topics decoded by the indexer

The indexer decodes every topic documented above. Versioned events are matched
on `topic[0] == "v1"` then `topic[1]`; unversioned events are matched on
`topic[0]` directly.

| Topic key | Event | Notes |
|-----------|-------|-------|
| `init` | `contract_initialized` | versioned |
| `created` | `stream_created` | versioned |
| `withdraw` | `withdrawn` | versioned |
| `status` | `stream_status_changed` | versioned |
| `meta_upd` | `stream_metadata_updated` | versioned (v1.1) |
| `split_vest` | `split_vest_created` | versioned (v1.1) |
| `cancelled` | `stream_cancelled` | versioned |
| `topup` | `topped_up` | versioned |
| `at_set` | `auto_topup_set` | versioned |
| `at_up` | `auto_topped_up` | versioned |
| `paused` | `contract_paused` | versioned |
| `rate_upd` | `rate_updated` | versioned |
| `cliff` | `cliff_reached` | versioned |
| `upg_prop` | `upgrade_proposed` | versioned |
| `upg_exec` | `upgrade_executed` | versioned |
| `upg_cncl` | `upgrade_cancelled` | versioned |
| `receipt` | `receipt` | unversioned |
| `archived` | `archived` | unversioned |
| `expired` | `expired` | unversioned |
| `xfr_strm` | `xfr_strm` | unversioned |
| `low_bal` | `low_bal` | unversioned |

Unknown topics (including unknown schema versions) are logged and skipped
without failing ingestion.

## Schema Migration History

| Version | Date | Changes |
|---------|------|---------|
| v1.1 | 2026-10-04 | Added `metadata_updated`, `split_vest_created` |
| v1.2 | 2026-10-11 | Added `withdraw_to`, `rate_schedule_set`, `milestone_schedule_set`, `stream_immutable` |
| v1 | 2026-09-27 | Initial schema version |
| v1.1 | 2026-10-01 | Added `contract_event_version` instance value (stamped on `initialize`/`migrate`), `evt_ver` and `mig_prog` events. No change to existing event topics or data |

## Testing

Event schemas are validated in the contract test suite. See `contracts/stream/src/test.rs` for tests that verify event structure matches this documentation.

---

## Notification Service (BE-12)

The notification service consumes the `low_bal` and `stream_near_expiry` events and turns them into per-recipient alerts delivered over email, push, and in-app channels according to each recipient's preferences. This section is the design contract for the notifier implementation.

### Subscribed Events

| Event | Topic match | Stream id | Payload fields used |
|-------|-------------|-----------|---------------------|
| `low_bal` | `topic[0] == "low_bal"` | `topic[1]` | `remaining`, `threshold` |
| `stream_near_expiry` | `topic[0] == "v1"` and `topic[1] == "near_exp"` | `topic[2]` | `end_time`, `seconds_remaining` |

The notifier subscribes to both events and normalizes them into a single internal `NotificationEvent` record:

```json
{
  "event_type": "low_balance" | "stream_near_expiry",
  "stream_id": 1,
  "remaining": 50000,          // low_balance only
  "threshold": 100000,         // low_balance only
  "end_time": 1767225600,      // stream_near_expiry only
  "seconds_remaining": 86400,  // stream_near_expiry only
  "ledger": 1234567,
  "tx_hash": "abcd...",
  "observed_at": "2026-09-27T12:00:00Z"
}
```

### Recipients

Each event resolves to one or more recipients based on the stream's on-chain parties:

- `low_balance`: employer (primary), employee (optional, opt-in).
- `stream_near_expiry`: employer and employee.

Recipient addresses are resolved to notification profiles via the user preferences store. Addresses without a profile receive no notifications (implicit opt-out) but the resolution attempt is still recorded in the audit log.

### Per-Recipient Preference Model

Preferences are stored per recipient address and per event type. Each channel can be independently enabled or disabled.

```json
{
  "address": "GABCD...",
  "event_type": "low_balance",
  "channels": {
    "email": { "enabled": true, "destination": "user@example.com" },
    "push":  { "enabled": false, "destination": null },
    "in_app": { "enabled": true, "destination": null }
  },
  "quiet_hours": { "start": "22:00", "end": "07:00", "timezone": "UTC" },
  "updated_at": "2026-09-27T12:00:00Z"
}
```

Rules:

- **Default preferences** (when a profile exists but no explicit preference for an event type): `in_app` enabled, `email` and `push` disabled.
- **Channel independence**: enabling/disabling one channel never affects the others.
- **Destination required**: `email` and `push` require a non-null destination; if missing, the channel is treated as disabled and the skip is audited.
- **Quiet hours**: non-`in_app` channels are deferred (not dropped) during quiet hours; `in_app` is always delivered immediately.
- **Opt-out**: a recipient may opt out of an event type entirely (`channels` all disabled) or globally. Opt-outs are recorded in the audit log (see below).

### Throttling and Deduplication

To avoid duplicate alerts per stream, the notifier applies throttling keyed by `(recipient_address, event_type, stream_id)`:

- **Deduplication window**: after a successful delivery to a recipient for a given key, subsequent events with the same key are suppressed for a configurable window (default: 24 hours for `low_balance`, 72 hours for `stream_near_expiry`).
- **Per-channel dedup**: the dedup key includes the channel, so a suppressed email does not suppress an in-app alert.
- **State transitions**: a `low_balance` alert is re-armed once the stream's remaining balance rises back above the threshold (observed via a subsequent `topup`/`at_up` event or a fresh `low_bal` with `remaining > threshold`). A `stream_near_expiry` alert is re-armed if `end_time` changes.
- **Idempotency**: events are deduplicated by `(tx_hash, event_index)` before throttling so replayed ledgers never produce duplicate deliveries.

### Audit Logging

Every delivery decision is appended to an append-only audit log. Each entry records:

```json
{
  "id": "uuid",
  "recipient": "GABCD...",
  "event_type": "low_balance",
  "stream_id": 1,
  "channel": "email",
  "action": "delivered" | "suppressed" | "opted_out" | "skipped" | "failed",
  "reason": "throttled" | "quiet_hours" | "channel_disabled" | "no_destination" | "no_profile" | null,
  "source": { "tx_hash": "abcd...", "ledger": 1234567 },
  "attempted_at": "2026-09-27T12:00:00Z",
  "delivered_at": "2026-09-27T12:00:01Z"
}
```

Audit requirements:

- **Deliveries**: every successful send is logged with channel, recipient, and source event reference.
- **Suppressions**: throttled and deduplicated events are logged with `action: "suppressed"` and the matching `reason`.
- **Opt-outs**: preference changes that disable a channel or event type are logged with `action: "opted_out"`, including the previous and new preference state.
- **Failures**: delivery errors are logged with `action: "failed"` and the error detail; retries are logged as separate entries.
- **Immutability**: audit entries are append-only and retained for the configured retention period; they are queryable by recipient, stream, and event type.
