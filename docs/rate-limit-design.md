# Rate-Limit Stream Creation per Ledger (SEC-05)

**Issue:** #34  
**Status:** Design  
**Priority:** Medium

---

## Problem

An attacker with sufficient tokens can spam `create_stream` (or `create_streams_batch`) to:

1. Bloat the employer's `EmployerStreams` index, increasing persistent storage write costs
2. Consume a large fraction of Soroban's per-ledger write quota, degrading service for
   legitimate users
3. Force off-chain indexers to process an arbitrarily large number of stream events

The existing `min_deposit` guard (E007) raises the *token cost* per stream but does not cap
the *ledger-level* throughput of stream creation.

---

## Proposed Solution

Introduce a per-employer, per-ledger stream creation counter enforced on both
`create_stream` and `create_streams_batch`.

### New storage keys

```rust
// In types.rs DataKey enum:
/// Ledger sequence number when the employer last created a stream.
LastCreateLedger(Address),
/// Number of streams created by this employer in the current ledger.
CreateCountThisLedger(Address),
/// Admin-configurable maximum streams per ledger per employer.
MaxCreatesPerLedger,
```

### New admin function

```rust
pub fn set_max_creates_per_ledger(env: Env, admin: Address, nonce: u64, max: u32)
```

- Default: `50`
- Minimum enforced: `1`
- Emits `max_creates_per_ledger_updated` event

### Rate-limit logic (per `create_stream` call)

```rust
fn check_and_increment_create_rate(env: &Env, employer: &Address) {
    let current_ledger = env.ledger().sequence();
    let max = get_max_creates_per_ledger(env); // default 50

    let last_ledger: u32 = env.storage().temporary()
        .get(&DataKey::LastCreateLedger(employer.clone()))
        .unwrap_or(0);

    let count: u32 = if last_ledger == current_ledger {
        env.storage().temporary()
            .get(&DataKey::CreateCountThisLedger(employer.clone()))
            .unwrap_or(0)
    } else {
        0 // new ledger — reset
    };

    assert!(count < max, "E025: stream creation rate limit exceeded");

    env.storage().temporary()
        .set(&DataKey::LastCreateLedger(employer.clone()), &current_ledger);
    env.storage().temporary()
        .set(&DataKey::CreateCountThisLedger(employer.clone()), &(count + 1));
}
```

Note: temporary storage is used so keys automatically expire and do not accumulate.

### Integration points

- `create_stream`: call `check_and_increment_create_rate` after auth check, before validation
- `create_streams_batch`: call once with `count += params.len()` check to prevent a single
  batch from consuming the entire quota in one call

### New error code

```
E025 | ERR_RATE_LIMIT_EXCEEDED | Stream creation rate limit exceeded for this ledger
```

---

## Acceptance Criteria

- [ ] `DataKey::LastCreateLedger(Address)` and `DataKey::CreateCountThisLedger(Address)` added
- [ ] `DataKey::MaxCreatesPerLedger` added; default value `50`
- [ ] `set_max_creates_per_ledger` admin function implemented with nonce protection
- [x] `create_stream` and `create_streams_batch` enforce the rate limit per employer
- [x] Rate limit resets every ledger (using `env.ledger().sequence()`)
- [x] Tests: at-limit succeeds, over-limit panics with E025
- [ ] API reference updated with new admin function and error code

### Test coverage (SEC-24 / TEST-53)

The SEC-24 rate-limit behaviour is covered by tests in `contracts/stream/src/test.rs`:

- **Limit enforced; excess reverts** — creating up to `MaxCreatesPerLedger` streams succeeds,
  and the next `create_stream` reverts with `E025` / `ERR_RATE_LIMIT_EXCEEDED`.
- **Resets next ledger** — after advancing the ledger sequence (via `env.ledger().sequence()`),
  the employer's counter resets and creation succeeds again.
- **Batch counts all entries** — `create_streams_batch` counts every entry in the batch against
  the quota, so a batch that would exceed the limit reverts with `E025` and a batch within the
  limit increments the counter by `params.len()`.
- **Snapshot committed** — the ledger snapshot is committed after the rate-limit state updates.

---

## Security Considerations

- Using `env.ledger().sequence()` (not timestamp) as the ledger boundary is more reliable
  since each ledger has exactly one sequence number
- Temporary storage keys have a configurable TTL; set to `1` ledger so they expire
  immediately and incur no ongoing storage rent
- The default of `50` is conservative; adjust based on testnet load measurements

---

# API-Layer Rate Limiting and Authentication (BE-07)

**Issue:** #408  
**Status:** Design  
**Priority:** Medium  
**Depends on:** BE-06

---

## Problem

The public API is currently unauthenticated and unthrottled. Anyone can call write and
subscribe endpoints, and anonymous reads are unbounded. This allows:

1. Unauthorized stream creation / subscription spam through the API layer
2. Resource exhaustion of the API service and its backing indexer
3. No accountability for who performed a write operation

The contract-level limit in SEC-05 caps per-ledger stream creation *on-chain*; BE-07 adds
a complementary *off-chain* control at the API boundary.

---

## Authentication Model

### API keys

- Keys are issued by an admin endpoint (`POST /admin/api-keys`) and returned **once** in
  plaintext; only a hash (e.g. SHA-256) is persisted.
- Each key record stores: `id`, `key_hash`, `label`, `scopes`, `created_at`,
  `revoked_at`, `last_used_at`.
- Keys are presented via the `X-API-Key` header.
- Revocation (`DELETE /admin/api-keys/{id}`) sets `revoked_at`; revoked keys are rejected
  with `401` on the next request. Revocation takes effect immediately (no cache TTL, or a
  short negative cache bounded to a few seconds).

### Scopes

| Scope        | Grants                                              |
|--------------|-----------------------------------------------------|
| `read`       | Read endpoints (also available anonymously, throttled) |
| `write`      | `create_stream`, `create_streams_batch`, cancel      |
| `subscribe`  | Event subscription / webhook registration           |

Write and subscribe endpoints **require** a valid key with the matching scope.
Anonymous requests to these endpoints return `401`.

### Anonymous read policy

- Read endpoints remain accessible without a key.
- Anonymous reads are throttled more aggressively than keyed reads (see buckets below).
- Anonymous requests are bucketed by client IP (respecting `X-Forwarded-For` only from
  trusted proxies).

---

## Token-Bucket Rate Limiting

Each bucket is a classic token bucket: capacity `C`, refill rate `R` tokens/second.
A request consumes one token; if the bucket is empty the request is rejected.

### Buckets

| Bucket            | Keyed by        | Capacity | Refill (tokens/s) |
|-------------------|-----------------|----------|-------------------|
| Per-key (write)   | API key id      | 30       | 5                 |
| Per-key (read)    | API key id      | 120      | 20                |
| Per-IP (anon read)| client IP       | 30       | 2                 |
| Per-IP (auth fail)| client IP       | 10       | 0.5               |

- Authenticated requests are limited by **both** the per-key bucket and the per-IP bucket;
  the stricter of the two applies.
- The `auth fail` bucket throttles repeated invalid/revoked key attempts to slow brute force.
- Bucket state is held in a shared store (e.g. Redis) so limits apply across instances.

### Rejection response

When a bucket is exhausted the API returns:

```
HTTP/1.1 429 Too Many Requests
Retry-After: <seconds until next token>
Content-Type: application/json

{ "error": "rate_limited", "bucket": "per-key-write", "retry_after": <seconds> }
```

`Retry-After` is computed as `ceil((1 - tokens) / R)` for the limiting bucket, clamped to
a minimum of `1` second.

---

## Interaction with Key Issuance / Revocation

- Issuing a key creates fresh, empty buckets for that key id.
- Revoking a key deletes its buckets and adds the key id to a short-lived deny list so
  in-flight requests fail fast.
- Invalid or revoked keys consume from the `auth fail` bucket before returning `401`.

---

## Audit Logging

Every authenticated request (and every rejected auth attempt) appends an audit record:

```json
{
  "ts": "2025-01-01T00:00:00Z",
  "key_id": "key_abc123",
  "ip": "203.0.113.7",
  "method": "POST",
  "path": "/streams",
  "status": 201,
  "rate_limited": false
}
```

- Audit records are written asynchronously and never block the request path.
- Key material is never logged; only `key_id`.
- Rate-limited requests are logged with `rate_limited: true` and the limiting bucket name.

---

## Acceptance Criteria (BE-07)

- [ ] API keys can be issued and revoked; revoked keys rejected with `401`
- [ ] Write and subscribe endpoints require a valid key with the matching scope
- [ ] Per-key and per-IP token buckets enforced; exhausted buckets return `429` with `Retry-After`
- [ ] Anonymous reads allowed but throttled by per-IP bucket
- [ ] Audit log records key id, IP, path, status, and rate-limit outcome for key use
- [ ] Bucket state shared across instances so limits are global

---

## Security Considerations (BE-07)

- Store only key hashes; never persist or log plaintext keys
- Trust `X-Forwarded-For` only from known proxies to prevent IP spoofing of buckets
- Use constant-time comparison when validating key hashes
- Keep `Retry-After` values coarse to avoid leaking precise bucket state
