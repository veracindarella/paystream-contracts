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

### v1.1 (2026-10-04) — admin/governance indexing (#412)

| Event | Change | Migration |
|-------|--------|-----------|
| `adm_prop`, `adm_acc`, `adm_cncl`, `paused`, `min_dep`, `fee_set`, `upg_prop`, `upg_exec`, `upg_cncl` | Indexer now consumes these events to build the admin audit trail and reconstruct the admin nonce sequence (see [Admin & Governance Indexing](#admin--governance-indexing)). No topic or data changes | No parser changes required. Start persisting the events listed below if you want an audit trail |

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

## Admin & Governance Indexing

Admin and governance actions are indexed so governance tooling can audit admin
activity (#412). Every event below is emitted by an admin-gated entry point and
carries the acting admin address in its data so the audit trail can be keyed by
admin.

### Event mapping

| Contract action | Event name | Topic | Data |
|-----------------|------------|-------|------|
| `propose_admin` | `adm_prop` | `["v1", "adm_prop"]` | `admin` (Address), `new_admin` (Address), `nonce` (u64) |
| `accept_admin` | `adm_acc` | `["v1", "adm_acc"]` | `admin` (Address), `nonce` (u64) |
| `cancel_admin_proposal` | `adm_cncl` | `["v1", "adm_cncl"]` | `admin` (Address), `nonce` (u64) |
| `pause` / `unpause` | `paused` | `["v1", "paused"]` | `paused` (bool), `admin` (Address), `nonce` (u64) |
| `set_min_deposit` | `min_dep` | `["v1", "min_dep"]` | `admin` (Address), `old_min` (i128), `new_min` (i128), `nonce` (u64) |
| `set_protocol_fee` | `fee_set` | `["v1", "fee_set"]` | `admin` (Address), `old_fee` (u32), `new_fee` (u32), `nonce` (u64) |
| `propose_upgrade` | `upg_prop` | `["v1", "upg_prop"]` | `admin` (Address), `wasm_hash` (BytesN<32>), `unlock_time` (u64), `nonce` (u64) |
| `execute_upgrade` | `upg_exec` | `["v1", "upg_exec"]` | `admin` (Address), `wasm_hash` (BytesN<32>), `nonce` (u64) |
| `cancel_upgrade` | `upg_cncl` | `["v1", "upg_cncl"]` | `admin` (Address), `nonce` (u64) |

Notes:

- `paused` is emitted for both pause and unpause; distinguish them with the
  `paused` boolean in data.
- `upg_prop` / `upg_exec` / `upg_cncl` extend the v1 payloads documented above
  with `admin` and `nonce`; indexers that only read the leading fields keep
  working.
- `nonce` is the contract-wide admin nonce **after** the action was applied
  (monotonically increasing, starting at `1` for the first admin action).

### Reconstructing the admin nonce sequence

The admin nonce is a strictly increasing counter incremented by every
admin-gated action. To rebuild it from events:

1. Filter events whose `topic[1]` is one of the admin/governance event names
   listed above.
2. Order them by `(ledger_sequence, transaction_index, event_index)` — the
   canonical Soroban ordering — **not** by the `nonce` field.
3. Read `nonce` from each event's data. The sequence must be contiguous:
   `1, 2, 3, ...` with no gaps and no duplicates.
4. A gap or duplicate indicates a missed event or a reorg; re-sync the affected
   ledger range before trusting the audit trail.
5. The current admin nonce is the `nonce` of the last event in canonical order
   (or `0` if no admin events exist yet).

### Audit trail keyed by admin address

Each admin/governance event includes the acting `admin` address in data. Build
the audit trail by indexing on `(admin, nonce)`:

- **Primary key**: `admin` address.
- **Sort key**: `nonce` (equivalently canonical event order).
- **Record**: `{ admin, nonce, action, ledger, tx_hash, event_index, payload }`
  where `payload` holds the action-specific fields (`new_admin`, `paused`,
  `old_min`/`new_min`, `old_fee`/`new_fee`, `wasm_hash`/`unlock_time`).

This makes queries such as "all actions by admin X", "the action at nonce N",
and "the full admin history in order" cheap to serve.

### Alerts on unexpected admin actions

Indexers should raise an alert (not just log) when any of the following is
observed, since they indicate either a compromise or an indexer bug:

- An admin/governance event whose `admin` is not the currently known admin.
- A nonce gap, duplicate nonce, or out-of-order nonce relative to canonical
  event order.
- `adm_prop` followed by `adm_acc` from a `new_admin` that was never proposed.
- `upg_exec` without a matching prior `upg_prop` for the same `wasm_hash`.
- `upg_exec` before the proposal's `unlock_time`.
- More than one `paused` toggle within a short window (possible flapping).
- `fee_set` / `min_dep` changes outside the configured allowed bounds.

---

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
| v1.1 | 2026-10-04 | Indexer consumes admin/governance events (`adm_prop`, `adm_acc`, `adm_cncl`, `paused`, `min_dep`, `fee_set`, `upg_prop`, `upg_exec`, `upg_cncl`) for the admin audit trail and nonce reconstruction (#412) |

## Testing

Event schemas are validated in the contract test suite. See `contracts/stream/src/test.rs` for tests that verify event structure matches this documentation.
