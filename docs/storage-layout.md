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

| Variant | Key XDR | Value type | Storage tier | Notes |
|---|---|---|---|---|
| `Stream(u64)` | `Vec[Symbol("Stream"), U64(id)]` | `Stream` struct | Persistent | One entry per stream |
| `EmployerStreams(Address)` | `Vec[Symbol("EmployerStreams"), Address]` | `Vec<u64>` | Persistent | Stream IDs created by the employer, append-only |
| `EmployeeStreams(Address)` | `Vec[Symbol("EmployeeStreams"), Address]` | `Vec<u64>` | Persistent | Stream IDs owned by the employee, append-only |
| `StreamCount` | `Vec[Symbol("StreamCount")]` | `u64` | Instance | Last issued stream ID (absent = 0) |
| `Admin` | `Vec[Symbol("Admin")]` | `Address` | Instance | Contract admin |
| `PendingAdmin` | `Vec[Symbol("PendingAdmin")]` | `Address` | Instance | Set by `propose_admin`, removed by `accept_admin` |
| `PendingAdminNonce` | `Vec[Symbol("PendingAdminNonce")]` | `u64` | Instance | Nonce bound to the pending admin proposal |
| `AdminNonce` | `Vec[Symbol("AdminNonce")]` | `u64` | Instance | Replay-protection counter (absent = 0) |
| `MinDeposit` | `Vec[Symbol("MinDeposit")]` | `i128` | Instance | Absent = default `10_000` |
| `Paused` | `Vec[Symbol("Paused")]` | `bool` | Instance | Global circuit breaker (absent = `false`) |
| `Version` | `Vec[Symbol("Version")]` | `u32` | Instance | Written by `migrate` |
| `PendingDrain` | `Vec[Symbol("PendingDrain")]` | — | Instance | Reserved for the emergency-drain proposal flow |

The upgrade timelock (`propose_upgrade` / `execute_upgrade`) additionally stores a
`PendingUpgrade` value in instance storage under `DataKey::PendingUpgrade`.

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

Persistent keys (`Stream`, `EmployerStreams`, `EmployeeStreams`) are extended on every
read/write through `storage.rs`:

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
