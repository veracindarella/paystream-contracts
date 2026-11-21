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

---

### low_balance

Emitted when a stream's remaining balance drops below its configured low-balance threshold. Consumed by the notification service (BE-12) to alert the employer (and optionally the employee).

**Topic**: `["low_bal", <stream_id>]`

> Note: `low_bal` is currently emitted **without** a version prefix (see v1.0 baseline). Match on `topic[0] == "low_bal"` and read the stream id from `topic[1]`.

**Data**:
- `remaining` (i128): Remaining stream balance at the time of emission
- `threshold` (i128): The low-balance threshold that was crossed

**Example**:
```json
{
  "topic": ["low_bal", 1],
  "data": [50000, 100000]
}
```

---

### stream_near_expiry

Emitted when a stream is approaching its end time (within the configured near-expiry window). Consumed by the notification service (BE-12) to alert the employer and employee.

**Topic**: `["v1", "near_exp", <stream_id>]`

**Data**:
- `end_time` (u64): The stream's scheduled end timestamp
- `seconds_remaining` (u64): Seconds remaining until `end_time` at emission time

**Example**:
```json
{
  "topic": ["v1", "near_exp", 1],
  "data": [1767225600, 86400]
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

## Schema Migration History

| Version | Date | Changes |
|---------|------|---------|
| v1 | 2026-09-27 | Initial schema version |

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