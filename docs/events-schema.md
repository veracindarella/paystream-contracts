# PayStream Event Schema

This document describes the structured event schema for PayStream smart contracts. Off-chain indexers can use this schema to parse Soroban events correctly and detect schema changes across contract upgrades.

## Changelog

Indexer-facing record of event schema changes. **Process:** any PR that adds, renames or removes an event, topic or data field must add an entry here (and bump the schema version for breaking changes).

Each entry lists the event, the change (added field / renamed topic / removed field) and migration steps for indexers.

### v1.0 (2026-09-27) — baseline

| Event | Change | Migration |
|-------|--------|-----------|
| `init`, `created`, `withdraw`, `status`, `cancelled`, `topup`, `paused`, `rate_upd`, `stop_upd`, `near_exp`, `exp_unres`, `upg_prop`, `upg_exec`, `upg_cncl`, `at_set`, `at_up`, `thresh_up`, `split`, `p_stats`, `cliff` | Baseline: topic prefixed with `"v1"` | None. Match on `topic[0] == "v1"`, then `topic[1]` |
| `receipt`, `archived`, `expired`, `xfr_strm`, `low_bal` | Baseline: emitted **without** a version prefix | Match on `topic[0]` directly. A future version will add the `"v1"` prefix and list it here |
| `transfer`, `approve`, `burn`, `mint` (token) | Baseline: token contract events, emitted **without** a version prefix | Match on `topic[0]` directly |

<!-- Template for new entries:
### vX.Y (YYYY-MM-DD)
| Event | Change | Migration |
|-------|--------|-----------|
| `name` | Added field `foo` (type) at data[n] | Treat missing `foo` as default for events before vX.Y |
-->

## Schema Version

Current schema version: **v1**

Stream-contract event topics include the schema version as the first component to enable version detection and compatibility handling. A small set of legacy stream events and all token-contract events are emitted **without** a version prefix (see the changelog above).

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

### status

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

### cancelled

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

### cliff

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

### upg_prop

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

### stop_upd

Emitted when a stream's stop time (end time) is updated.

**Topic**: `["v1", "stop_upd", <stream_id>]`

**Data**:
- `old_stop_time` (u64): Previous stop timestamp
- `new_stop_time` (u64): New stop timestamp

**Example**:
```json
{
  "topic": ["v1", "stop_upd", 1],
  "data": [1767225600, 1769904000]
}
```

---

### near_exp

Emitted when a stream is approaching its stop time (near-expiry warning).

**Topic**: `["v1", "near_exp", <stream_id>]`

**Data**:
- `stop_time` (u64): The stream's stop timestamp
- `remaining` (i128): Remaining streamed balance at emission time

**Example**:
```json
{
  "topic": ["v1", "near_exp", 1],
  "data": [1767225600, 500000]
}
```

---

### exp_unres

Emitted when a stream expires with an unresolved balance that could not be settled.

**Topic**: `["v1", "exp_unres", <stream_id>]`

**Data**:
- `unresolved_amount` (i128): Balance left unresolved at expiry

**Example**:
```json
{
  "topic": ["v1", "exp_unres", 1],
  "data": [250000]
}
```

---

### thresh_up

Emitted when the auto top-up threshold for a stream is updated.

**Topic**: `["v1", "thresh_up", <stream_id>]`

**Data**:
- `old_threshold` (i128): Previous threshold
- `new_threshold` (i128): New threshold

**Example**:
```json
{
  "topic": ["v1", "thresh_up", 1],
  "data": [100000, 200000]
}
```

---

### split

Emitted when a stream's accrual is split between recipients.

**Topic**: `["v1", "split", <stream_id>]`

**Data**:
- `employee_amount` (i128): Amount allocated to the employee
- `employer_amount` (i128): Amount allocated to the employer

**Example**:
```json
{
  "topic": ["v1", "split", 1],
  "data": [600000, 400000]
}
```

---

### p_stats

Emitted when protocol-level statistics are updated.

**Topic**: `["v1", "p_stats"]`

**Data**:
- `total_streams` (u64): Total number of streams created
- `total_streamed` (i128): Cumulative amount streamed

**Example**:
```json
{
  "topic": ["v1", "p_stats"],
  "data": [42, 100000000]
}
```

---

### receipt

Emitted when a withdrawal receipt is issued. **Unversioned** (no `"v1"` prefix).

**Topic**: `["receipt", <stream_id>]`

**Data**:
- `employee` (Address): The employee address
- `amount` (i128): Amount withdrawn on this receipt

**Example**:
```json
{
  "topic": ["receipt", 1],
  "data": ["GEFGH...", 500000]
}
```

---

### archived

Emitted when a stream is archived. **Unversioned** (no `"v1"` prefix).

**Topic**: `["archived", <stream_id>]`

**Data**:
- `archived_at` (u64): Timestamp at which the stream was archived

**Example**:
```json
{
  "topic": ["archived", 1],
  "data": [1767225600]
}
```

---

### expired

Emitted when a stream reaches its stop time and expires. **Unversioned** (no `"v1"` prefix).

**Topic**: `["expired", <stream_id>]`

**Data**:
- `stop_time` (u64): The stream's stop timestamp

**Example**:
```json
{
  "topic": ["expired", 1],
  "data": [1767225600]
}
```

---

### xfr_strm

Emitted when a stream is transferred to a new employer/owner. **Unversioned** (no `"v1"` prefix).

**Topic**: `["xfr_strm", <stream_id>]`

**Data**:
- `from` (Address): Previous owner
- `to` (Address): New owner

**Example**:
```json
{
  "topic": ["xfr_strm", 1],
  "data": ["GABCD...", "GEFGH..."]
}
```

---

### low_bal

Emitted when a stream's balance falls below the low-balance threshold. **Unversioned** (no `"v1"` prefix).

**Topic**: `["low_bal", <stream_id>]`

**Data**:
- `balance` (i128): Current stream balance
- `threshold` (i128): Low-balance threshold

**Example**:
```json
{
  "topic": ["low_bal", 1],
  "data": [50000, 100000]
}
```

## Token Contract Events

Events emitted by `contracts/token/src/events.rs`. All token events are **unversioned**
(no `"v1"` prefix).

### transfer

Emitted when tokens are transferred between accounts.

**Topic**: `["transfer", <from>, <to>]`

**Data**:
- `amount` (i128): Amount transferred

**Example**:
```json
{
  "topic": ["transfer", "GABCD...", "GEFGH..."],
  "data": [1000000]
}
```

---

### approve

Emitted when an allowance is set for a spender.

**Topic**: `["approve", <from>, <spender>]`

**Data**:
- `amount` (i128): Approved allowance
- `expiration_ledger` (u32): Ledger at which the allowance expires

**Example**:
```json
{
  "topic": ["approve", "GABCD...", "GEFGH..."],
  "data": [1000000, 123456]
}
```

---

### burn

Emitted when tokens are burned.

**Topic**: `["burn", <from>]`

**Data**:
- `amount` (i128): Amount burned

**Example**:
```json
{
  "topic": ["burn", "GABCD..."],
  "data": [500000]
}
```

---

### mint

Emitted when tokens are minted.

**Topic**: `["mint", <to>]`

**Data**:
- `amount` (i128): Amount minted

**Example**:
```json
{
  "topic": ["mint", "GEFGH..."],
  "data": [500000]
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

## Schema Migration History

| Version | Date | Changes |
|---------|------|---------|
| v1 | 2026-09-27 | Initial schema version |

## Testing

Event schemas are validated in the contract test suite. See `contracts/stream/src/test.rs` for tests that verify event structure matches this documentation.

A CI lint (BE-15) extracts every emitted topic from `contracts/stream/src/events.rs` and
`contracts/token/src/events.rs` and asserts that each has a matching `### <topic>` entry
in this document, and that no documented entry is stale.
