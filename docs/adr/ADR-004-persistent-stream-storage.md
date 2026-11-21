# ADR-004: Persistent storage for streams

- **Status:** Accepted

## Context
The number of streams is unbounded, and each must survive for months or years.

## Decision
Each stream lives in **persistent** storage under `DataKey::Stream(id)`. Its TTL is extended on every access (`TTL_THRESHOLD` ≈ 1 year → `TTL_EXTEND_TO` ≈ 2 years). Instance storage holds only small global config.

### Cold field relocation
To reduce the per-stream rent footprint, rarely used ("cold") fields — `pause_reason` and `low_balance_threshold` — are stored under a separate persistent entry, `DataKey::StreamCold(id)`, rather than inline in the hot `Stream` entry. The hot `Stream` entry retains these fields so that existing entries continue to decode in a backward-compatible way (see [ADR-004](../adr/ADR-004-persistent-stream-storage.md) decode guarantees). Readers must merge the cold data from `DataKey::StreamCold(id)` with the hot `Stream` entry to obtain a complete view of a stream; writers must keep both entries consistent. The cold entry's TTL is extended on every access alongside the hot entry.

The resulting rent savings are documented in [storage-layout.md](../storage-layout.md).

## Alternatives considered
- **Instance storage**: the whole instance is loaded and charged on *every* invocation. Storing streams there would make fees grow with the total number of streams and eventually hit entry size limits.
- **Temporary storage**: deleted on expiry, which is unacceptable for funds. It is used only for archived streams after the 30-day wait, and for rate-limit counters.

## Consequences
- Per-stream cost is constant, and entries are independently archivable/restorable.
- Every persistent accessor must extend the TTL (pattern in `storage.rs`).
- Cold fields live in a separate `DataKey::StreamCold(id)` entry; readers must merge cold data with the hot `Stream` entry, and writers must keep both in sync.
- Indexers should keep their own history, because expired entries are archived (see [storage-layout.md](../storage-layout.md)).