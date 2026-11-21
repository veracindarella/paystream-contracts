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
| `StreamMetadata(u64)` | `Map<Symbol, String>` | Persistent | Extended on read/write (1y → 2y) | `create_stream` / `update_stream_metadata` (bounded, cap enforced) | `get_stream_metadata` |
| `MaxBatchSize` | `u32` | Instance | Instance | `set_max_batch_size` (bounds `1..=200`) | All batch entry points (absent = 50) |

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
| `metadata` | `Map<Symbol, String>` | Optional bounded metadata map (e.g. department, cost center); empty when unset |
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
