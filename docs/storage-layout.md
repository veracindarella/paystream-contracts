# Storage Layout

Reference for off-chain indexers that read PayStream **stream contract** ledger state directly.
Source of truth: `DataKey` in `contracts/stream/src/types.rs` and the accessors in
`contracts/stream/src/storage.rs`.

---

## Key encoding (XDR)

`DataKey` is a `#[contracttype]` enum, so each key is encoded as an `ScVal::Vec`:

- **Unit variant** (e.g. `Admin`) → `Vec[ Symbol("Admin") ]`
- **Tuple variant** (e.g. `Stream(u64)`) → `Vec[ Symbol("Stream"), <payload> ]`

`u64` payloads encode as `ScVal::U64`; `Address` payloads encode as `ScVal::Address`.

Example: the key for stream `7` is `Vec[ Symbol("Stream"), U64(7) ]`.

---

## Keys

Every `DataKey` variant in `contracts/stream/src/types.rs`, in declaration order.

| Key | Value Type | Storage | TTL | Set by | Read by |
|---|---|---|---|---|---|
| `Stream(u64)` | `Stream` | Persistent (Temporary once archived) | Extended on read/write (1y → 2y) | `create_stream`, `withdraw`, `top_up`, `pause_stream`, `cancel_stream`, …; moved to temporary by `archive_stream` | `get_stream`, `claimable`, all stream entry points |
| `StreamCount` | `u64` | Instance | Instance | `create_stream` / batch create (`next_id`) | `stream_count`, `next_id` |
| `Admin` | `Address` | Instance | Instance | `initialize`, `accept_admin` | Every admin entry point |
| `AutoTopup(u64)` | `AutoTopupConfig` | Persistent | Extended on read/write (1y → 2y) | `set_auto_topup`; removed on disable | `get_auto_topup`, top-up trigger |
| `MinDeposit` | `i128` | Instance | Instance | `set_min_deposit` | `create_stream` validation (absent = `10_000`) |
| `ProtocolFee` | `ProtocolFeeConfig` | Instance | Instance | `set_protocol_fee` | `create_stream` / `top_up` fee calculation (absent = 0 bps) |
| `AdminNonce` | `u64` | Instance | Instance | Every nonce-protected admin call | `admin_nonce`, `consume_admin_nonce` |
| `Paused` | `bool` | Instance | Instance | `pause_contract` / `unpause_contract` | All state-changing entry points (absent = `false`) |
| `PendingAdmin` | `Address` | Instance | Instance | `propose_admin`; removed by `accept_admin` | `accept_admin`, `get_pending_admin` |
| `PendingAdminNonce` | `u64` | Instance | Instance | `propose_admin`; removed by `accept_admin` | `accept_admin` |
| `EmployerStreams(Address)` | `Vec<u64>` | Persistent | Extended on read/write (1y → 2y) | `create_stream`, `transfer_stream` | `get_employer_streams` |
| `EmployeeStreams(Address)` | `Vec<u64>` | Persistent | Extended on read/write (1y → 2y) | `create_stream`, `transfer_receipt` | `get_employee_streams` |
| `Version` | `u32` | Instance | Instance | `migrate` | `version` |
| `PendingDrain` | `(Address, u64)` | Instance | Instance | `propose_emergency_drain` (reserved, SEC-03) | `emergency_drain` |
| `StreamsByTimestamp(u64)` | `Vec<u64>` | Persistent | Extended on read/write (1y → 2y) | `create_stream` (bucket = `timestamp / 86_400`) | Time-range queries |
| `TokenStreams(Address)` | `Vec<u64>` | Persistent | Extended on read/write (1y → 2y) | `create_stream` | Token-filtered queries |
| `ReceiptOwner(u64)` | `Address` | Persistent | Extended on read/write (1y → 2y) on write | `transfer_receipt` | `receipt_owner` (absent = `stream.employee`) |
| `TemplateIds(Address)` | `Vec<u32>` | Persistent | Extended on read/write (1y → 2y) | `save_template`, `delete_template` | `list_templates` |
| `Template(Address, u32)` | `StreamParams` | Persistent | Extended on read/write (1y → 2y) | `save_template` | `create_stream_from_template`, `get_template` |
| `StreamCliff(u64)` | `u64` | Persistent | Extended on read/write (1y → 2y) | `create_stream` with cliff (PROD-17) | `claimable`, `withdraw` |
| `CliffReached(u64)` | `bool` | Persistent | Extended on read/write (1y → 2y) | First withdraw/claim after cliff | `mark_cliff_reached` (emit `cliff_reached` once) |
| `StreamType(u64)` | `StreamType` | Persistent | Extended on read/write (1y → 2y) (LinearVest only) | Linear-vest stream creation | `get_stream_type` (absent = `Payroll`) |
| `LinearVestSchedule(u64)` | `LinearVestSchedule` | Persistent | Extended on read/write (1y → 2y) | Linear-vest stream creation | `linear_vest_claimable_amount` |
| `PendingUpgrade` | `PendingUpgrade` | Instance | Instance | `propose_upgrade`; removed by `execute_upgrade` / `cancel_upgrade` | `execute_upgrade`, `cancel_upgrade` |
| `StreamEndedAt(u64)` | `u64` | Persistent | Not extended (default persistent TTL) | Transition to Cancelled / Exhausted | `archive_stream` (30-day wait) |
| `MaxCreatesPerLedger` | `u32` | Instance | Instance | `set_max_creates_per_ledger` | Rate limiter (absent = 50) |
| `CreateCountThisLedger(Address)` | `u32` | Temporary | 2 ledgers | `check_and_increment_create_rate` | Rate limiter |
| `LastCreateLedger(Address)` | `u32` | Temporary | 2 ledgers | `check_and_increment_create_rate` | Rate limiter |
| `SplitBeneficiaries(u64)` | `Vec<SplitBeneficiary>` | Persistent | Extended on read/write (1y → 2y) | Split stream creation (PROD-22) | Split withdrawals |

> **Keep this table current.** Any PR that adds, removes, or changes a `DataKey` variant must update this table in the same PR (reviewers: check this).

### `Stream` value

`Stream` is a `#[contracttype]` struct, encoded as an `ScVal::Map` keyed by field name
(`Symbol`), sorted alphabetically:

| Field | Type | Meaning |
|---|---|---|
| `deposit` | `i128` | Total deposited amount |
| `employee` | `Address` | Recipient |
| `employer` | `Address` | Funder |
| `id` | `u64` | Stream ID |
| `last_withdraw_time` | `u64` | Ledger timestamp of the last withdrawal |
| `locked` | `bool` | Reentrancy guard (normally `false` between transactions) |
| `rate_per_second` | `i128` | Tokens streamed per second |
| `start_time` | `u64` | Ledger timestamp the stream started |
| `status` | `StreamStatus` | `Vec[Symbol("Active" \| "Paused" \| "Cancelled" \| "Exhausted")]` |
| `stop_time` | `u64` | Hard stop timestamp (`0` = no end) |
| `token` | `Address` | Token contract address |
| `withdrawn` | `i128` | Total already withdrawn |

---

## TTL thresholds

Persistent keys are extended on every read/write through `storage.rs` (except
`StreamEndedAt`, see table above). Temporary rate-limit keys live for 2 ledgers.

| Constant | Ledgers | Approx. time (5 s/ledger) |
|---|---|---|
| `TTL_THRESHOLD` | `6_307_200` | ~1 year |
| `TTL_EXTEND_TO` | `12_614_400` | ~2 years |

When a persistent entry's remaining TTL drops below `TTL_THRESHOLD`, it is extended to
`TTL_EXTEND_TO`. Streams that are never touched for longer than their remaining TTL are
archived and must be restored before they can be read on-chain; indexers should rely on
their own copy of historical data rather than live state for archived streams.

Instance keys share the TTL of the contract instance entry.

---

## Keys affected by contract upgrades

`upgrade` / `execute_upgrade` replace the WASM only; all storage is preserved.

- `Version` — written by `migrate` after an upgrade; indexers can use it to detect schema changes.
- `AdminNonce` — incremented by every admin call, including upgrades.
- `PendingUpgrade` — set by `propose_upgrade`, cleared by `execute_upgrade` / `cancel_upgrade`.
- `Stream`, `EmployerStreams`, `EmployeeStreams` — unchanged by upgrades. If a future version
  changes the `Stream` struct layout, `Version` will be bumped and the change noted in
  `CHANGELOG.md` and [upgrade-guide.md](upgrade-guide.md).

---

## Off-chain indexer schema

The indexer keeps its own persistent store (SQLite/Postgres) built from contract events and
ledger state. It is versioned independently of the contract `Version` key so the indexer can
evolve without a resync. Migrations live in `indexer/migrations/` as numbered, forward-only
SQL files (`0001_init.sql`, `0002_admin_audit.sql`, …).

### `schema_version`

Single-row table tracking the applied migration.

| Column | Type | Notes |
|---|---|---|
| `version` | `INTEGER` | Highest applied migration number |
| `applied_at` | `TIMESTAMP` | When the migration ran |

On startup the indexer reads `version`, applies every migration with a higher number in
ascending order inside a transaction, and bumps `version`. Migrations are **forward-only**:
there are no down migrations; a bad migration is fixed by adding a new numbered one. A
migration that has already been applied is never edited.

### Tables

| Table | Purpose | Key columns |
|---|---|---|
| `streams` | One row per stream, mirroring the `Stream` struct | `id` (PK), `employer`, `employee`, `token`, `deposit`, `withdrawn`, `rate_per_second`, `start_time`, `stop_time`, `last_withdraw_time`, `status`, `stream_type`, `cliff_time`, `ended_at`, `created_ledger`, `updated_ledger` |
| `beneficiaries` | Split beneficiaries per stream | `stream_id` (FK → `streams.id`), `address`, `bps`; PK `(stream_id, address)` |
| `events` | Raw decoded contract events, append-only | `id` (PK), `contract_id`, `ledger`, `tx_hash`, `event_index`, `topic`, `stream_id`, `actor`, `amount`, `payload` (JSON), `ingested_at`; unique `(tx_hash, event_index)` |
| `aggregates` | Precomputed rollups for dashboards | `bucket` (e.g. `day`), `bucket_start`, `employer`, `employee`, `token`, `stream_count`, `deposited`, `withdrawn`, `fees`; PK `(bucket, bucket_start, employer, employee, token)` |
| `admin_audit` | Governance/admin actions (BE-11) | `id` (PK), `ledger`, `tx_hash`, `event_index`, `action`, `admin`, `target`, `nonce`, `payload` (JSON), `ingested_at`; unique `(tx_hash, event_index)` |

`events` and `admin_audit` are append-only; reorg handling deletes rows above the fork ledger
and re-ingests. `streams` and `aggregates` are derived and can be rebuilt from `events`.

### Indexes (FE query patterns)

| Index | Backs |
|---|---|
| `idx_streams_employer (employer, status)` | Employer dashboard: streams by employer, filtered by status |
| `idx_streams_employee (employee, status)` | Employee dashboard: streams by employee, filtered by status |
| `idx_streams_token (token, status)` | Token-filtered queries |
| `idx_streams_status (status)` | Global status filters / counts |
| `idx_streams_employer_created (employer, created_ledger)` | Time-ordered employer lists |
| `idx_events_stream (stream_id, ledger)` | Per-stream event history |
| `idx_events_topic (topic, ledger)` | Event-type scans |
| `idx_aggregates_lookup (bucket, bucket_start, employer, employee, token)` | Aggregate lookups |
| `idx_admin_audit_admin (admin, ledger)` | Audit trail by admin address (BE-11) |
| `idx_admin_audit_action (action, ledger)` | Audit trail by action type |

### Admin / governance events (BE-11)

The following events are decoded into `admin_audit` with `action` set to the event name:

- `propose_admin`, `accept_admin` (`target` = new admin)
- `pause_contract`, `unpause_contract`
- `set_min_deposit`, `set_protocol_fee` (`payload` holds old/new values)
- `propose_upgrade`, `execute_upgrade`, `cancel_upgrade` (`payload` holds WASM hash / target)

**Admin nonce sequence.** The contract stores `AdminNonce` as a counter, but the indexer
reconstructs the sequence from events: each nonce-protected admin event carries its nonce in
`payload`, and `admin_audit.nonce` stores it. The current nonce for an admin is
`MAX(nonce)` over `admin_audit` for that `admin`; gaps or out-of-order nonces indicate a
missed or replayed event and are flagged.

**Alerts.** The indexer raises an alert when an admin action is unexpected: an `accept_admin`
or `execute_upgrade` with no matching prior `propose_*`, a nonce gap, or a pause/unpause or
fee change outside the configured expected window. Alerts are written to the `events` table
with `topic = "alert"` and surfaced to governance tooling.
