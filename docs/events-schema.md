# PayStream Event Schema

This document describes the structured event schema for PayStream smart contracts. Off-chain indexers can use this schema to parse Soroban events correctly and detect schema changes across contract upgrades.

## Changelog

Indexer-facing record of event schema changes. **Process:** any PR that adds, renames or removes an event, topic or data field must add an entry here (and bump the schema version for breaking changes).

Each entry lists the event, the change (added field / renamed topic / removed field) and migration steps for indexers.

### v1.0 (2026-09-27) — baseline

| Event | Change | Migration |
|-------|--------|-----------|
| `init`, `created`, `withdraw`, `status`, `cancelled`, `topup`, `paused`, `rate_upd`, `stop_upd`, `near_exp`, `exp_unres`, `upg_prop`, `upg_exec`, `upg_cncl`, `at_set`, `at_up`, `thresh_up`, `split`, `p_stats` | Baseline: topic prefixed with `"v1"` | None. Match on `topic[0] == "v1"`, then `topic[1]` |
| `receipt`, `archived`, `expired`, `xfr_strm`, `low_bal` | Baseline: emitted **without** a version prefix | Match on `topic[0]` directly. A future version will add the `"v1"` prefix and list it here |

<!-- Template for new entries:
### vX.Y (YYYY-MM-DD)
| Event | Change | Migration |
|-------|--------|-----------|
| `name` | Added field `foo` (type) at data[n] | Treat missing `foo` as default for events before vX.Y |
-->

## Schema Version

Current schema version: **v1**

All event topics include the schema version as the first component to enable version detection and compatibility handling.

## Event Format

Soroban events have two parts:
- **Topic**: An array that identifies the event type and associated keys
- **Data**: The event payload values

Topics always follow this pattern:
```
[<schema_version>, <event_name>, ...event_specific_keys]
```

## Events

### contract_initialized

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

### stream_created

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

### withdrawn

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

### stream_cancelled

Emitted when a stream is cancelled, recording the exact cash-flow split.

**Topic**: `["v1", "cancelled", <stream_id>]`

**Data**:
- `claimable_amount` (i128): Tokens paid to the employee at cancellation time
- `refund_amount` (i128): Tokens returned to the employer (unstreamed portion)

**Example**:
```json
{
  "topic": ["v1", "cancelled", 1],
  "data": [300000, 700000]
}
```

---

### topped_up

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

### auto_topup_set

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

### auto_topped_up

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

### contract_paused

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

### rate_updated

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

### cliff_reached

Emitted once per stream the first time it is touched (`withdraw`,
`withdraw_all`, `cancel_stream`, `settle_stream`) at or after its cliff
(PROD-17). Streams without a cliff never emit it.

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

### upgrade_executed

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

### upgrade_cancelled

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

## Type Reference

| Type | Description | Example |
|------|-------------|---------|
| Address | Stellar address (32 bytes) | `GABCD...` |
| i128 | Signed 128-bit integer | `1000000` |
| u64 | Unsigned 64-bit integer | `1234567890` |
| bool | Boolean | `true` or `false` |
| BytesN<32> | Fixed 32-byte array | `abcd1234...` |
| StreamStatus enum | Stream status (0-3) | `0`, `1`, `2`, or `3` |

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
        type: "stream_created",
        streamId: topic[2],
        employer: data[0],
        employee: data[1],
        rate: data[2]
      };
    // ... other events
  }
}
```

## Indexer Ingestion Contract

Off-chain indexers that consume these events must support three ingestion modes. This section defines the contract between the contracts (event producers) and indexers (event consumers) so that replay, resume and live processing never duplicate or skip events.

### Modes

| Mode | Purpose | Start point |
|------|---------|-------------|
| **Backfill** | Populate a fresh indexer (new deployment) or repair a gap | A configurable start ledger (default: genesis / contract deployment ledger) |
| **Catch-up** | Resume after downtime | The persisted high-water-mark cursor |
| **Live** | Follow the chain head | The persisted high-water-mark cursor, then stream new ledgers |

Backfill and live modes **must not overlap**: an indexer runs exactly one mode at a time. When backfill or catch-up reaches the chain head, it transitions to live mode from the same cursor (see *Mode transitions* below).

### High-Water-Mark Cursor

The cursor is the indexer's durable progress marker. It is a single record persisted in the indexer's own storage (database, KV store, etc.) — never in the contract.

```
cursor = {
  ledger: u32,        // last fully processed ledger sequence
  eventIndex: u32,    // last processed event index within that ledger
  schemaVersion: str  // schema version the cursor was written under, e.g. "v1"
}
```

Semantics:

- The cursor is **inclusive**: `(ledger, eventIndex)` identifies the last event that was fully applied.
- The next event to process is the first event strictly after `(ledger, eventIndex)` in ledger order, then event order within the ledger.
- The cursor is **monotonic**: it only ever moves forward. A reorg or replay that would move it backwards must be treated as an error and surfaced, not silently applied.
- The cursor is written **only after** the effects of the event are committed (see *Idempotency and ordering*).

### Backfill

Backfill replays events from a configurable start ledger:

1. Read the configured start ledger `L_start` (default: the contract deployment ledger, i.e. genesis for this contract).
2. Initialize the cursor to `{ ledger: L_start - 1, eventIndex: MAX, schemaVersion: <current> }` so the first event at `L_start` is included.
3. Fetch ledgers in ascending order, parse events using the schema version routing described in *Indexer Compatibility Guide*, and apply them.
4. After each ledger's events are committed, advance the cursor to the last processed event of that ledger.
5. When the cursor reaches the chain head, switch to **live** mode from the same cursor.

Backfill is **resumable**: if the process stops, restarting backfill with the same start ledger resumes from the persisted cursor rather than from `L_start`. To force a full replay, explicitly reset the cursor first.

### Catch-up

Catch-up resumes from the persisted high-water mark:

1. Load the cursor. If none exists, fall back to backfill from the configured start ledger.
2. Fetch ledgers strictly after `cursor.ledger`, starting at `cursor.eventIndex + 1` within `cursor.ledger`.
3. Apply and commit events, advancing the cursor as in backfill.
4. When the cursor reaches the chain head, switch to **live** mode from the same cursor.

Catch-up and backfill share the same apply/commit/cursor-advance loop; they differ only in where the cursor is initialized.

### Live

Live mode follows the chain head:

1. Start from the persisted cursor (after catch-up or backfill has reached the head).
2. Poll or subscribe for new ledgers; apply events in ledger order.
3. Advance the cursor after each committed ledger.

Because live mode starts strictly after the cursor, events already applied during backfill/catch-up are never re-applied.

### Mode Transitions

```
backfill ──(cursor == head)──▶ live
catch-up ──(cursor == head)──▶ live
live ──(gap detected / restart)──▶ catch-up
```

- Only one mode is active at a time; the mode is recorded alongside the cursor so a restart resumes the correct mode.
- A detected gap (cursor older than the retention window of the event source) triggers catch-up, and if the gap predates the cursor's recoverable range, backfill from the configured start ledger.

### Idempotency and Ordering

To guarantee backfill, catch-up and live modes never duplicate processing:

- **Idempotent event keys.** Derive a deterministic key for every event, e.g. `(ledger, eventIndex)` or `(txHash, eventIndex)`. Applying an event whose key already exists is a no-op. This makes replays safe even if the cursor is lost or reset.
- **Cursor advanced after commit.** Apply the event's effects and commit them in the same transaction that advances the cursor. If the process crashes between apply and commit, the event is replayed on restart and deduplicated by its key.
- **Ordered application.** Events are applied in `(ledger, eventIndex)` order. Do not apply events out of order or in parallel within a ledger.
- **At-least-once delivery.** The pipeline is at-least-once; exactly-once semantics come from the idempotent keys plus the post-commit cursor.

### Replay Across Upgrades

Backfill may replay events emitted under older schema versions. Use the version routing in *Indexer Compatibility Guide*:

- Route each event by `topic[0]` (`"v1"`, `"v2"`, …) to the matching parser; keep parsers for all historical versions.
- Events emitted without a version prefix (see the v1.0 changelog entry) are matched on `topic[0]` directly.
- Record the schema version in the cursor so a resume knows which parsers were in use; if the indexer is upgraded to support a newer schema, it may continue from the existing cursor without re-backfilling.
- Unknown versions must fail gracefully (log and skip or halt) rather than corrupt the cursor.

### Observability

Expose at minimum:

- Current mode (`backfill` / `catch-up` / `live`)
- Cursor `(ledger, eventIndex, schemaVersion)`
- Target head ledger and lag (`head - cursor.ledger`)
- Counters for applied, deduplicated and failed events

These make progress observable and let operators confirm a backfill or catch-up is advancing and resumable.

## Schema Migration History

| Version | Date | Changes |
|---------|------|---------|
| v1 | 2026-09-27 | Initial schema version |

## Testing

Event schemas are validated in the contract test suite. See `contracts/stream/src/test.rs` for tests that verify event structure matches this documentation.
