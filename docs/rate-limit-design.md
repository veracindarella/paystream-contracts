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
- [ ] `create_stream` and `create_streams_batch` enforce the rate limit per employer
- [ ] Rate limit resets every ledger (using `env.ledger().sequence()`)
- [ ] Tests: at-limit succeeds, over-limit panics with E025
- [ ] API reference updated with new admin function and error code

---

## Security Considerations

- Using `env.ledger().sequence()` (not timestamp) as the ledger boundary is more reliable
  since each ledger has exactly one sequence number
- Temporary storage keys have a configurable TTL; set to `1` ledger so they expire
  immediately and incur no ongoing storage rent
- The default of `50` is conservative; adjust based on testnet load measurements
