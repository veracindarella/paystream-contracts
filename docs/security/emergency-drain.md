# Emergency Drain Procedure (SEC-03)

**Issue:** #32  
**Status:** Design  
**Priority:** High

---

## Overview

In the event of a critical vulnerability, the admin needs a last-resort mechanism to recover
all tokens held by the contract and distribute them to stream owners. Without this, a hard-paused
contract leaves deposited tokens locked permanently.

The `emergency_drain` function provides this capability with strong safeguards to prevent abuse.

---

## Safeguards

| Safeguard | Rationale |
|-----------|-----------|
| Contract must be hard-paused first | Prevents drain during normal operation |
| Requires two-of-two admin confirmation (propose + execute) | No single transaction can drain; prevents key-compromise drain |
| Emits `emergency_drain` event on-chain | Creates an immutable audit trail |
| Marks all Active/Paused streams as Cancelled | Prevents double-claim after drain |
| Nonce consumed on propose and execute | Replay protection on both steps |

---

## Proposed API

### Step 1 — Propose drain

```rust
pub fn propose_emergency_drain(
    env: Env,
    admin: Address,
    nonce: u64,
    recipient: Address,
)
```

- Admin auth required
- Stores `recipient` and current nonce in instance storage under `DataKey::PendingDrain`
- Emits `emergency_drain_proposed(recipient)` event
- Contract must already be paused

### Step 2 — Execute drain

```rust
pub fn emergency_drain(
    env: Env,
    admin: Address,
    nonce: u64,
)
```

- Admin auth required
- Verifies `DataKey::PendingDrain` exists and nonce matches
- Iterates all streams (via `StreamCount`), marks Active/Paused streams as Cancelled
- Queries contract token balance for every unique token address held
- Transfers full balance of each token to the stored `recipient`
- Emits `emergency_drained(recipient, token, amount)` per token
- Clears `DataKey::PendingDrain`

---

## New Storage Keys

```rust
// In types.rs DataKey enum:
/// Pending emergency drain: stores (recipient, nonce) set by propose_emergency_drain.
PendingDrain,
```

---

## New Event

```
emergency_drain_proposed(recipient: Address)
emergency_drained(recipient: Address, token: Address, amount: i128)
```

---

## Security Considerations

- The two-step pattern means an attacker who compromises the admin key must submit two
  separate transactions; monitoring infrastructure can detect and respond to the proposal
  before the drain executes
- The drain can only proceed when `DataKey::Paused == true`, so it cannot be used as a
  silent rug-pull during normal operation
- Iterating all streams to mark them Cancelled may hit Soroban's per-transaction instruction
  limit for contracts with thousands of streams; a paginated version (drain in batches) may
  be needed for large deployments

---

## Threat Model Update

Once implemented, update `docs/security/threat-model.md` section 4 (Residual Risks) to reflect
that a contract-pause + emergency drain procedure is available to the admin.

---

## Acceptance Criteria

- [ ] `DataKey::PendingDrain` added to `types.rs`
- [ ] `propose_emergency_drain` implemented (requires paused contract + admin auth + nonce)
- [ ] `emergency_drain` implemented (requires pending drain + admin auth + nonce)
- [ ] All Active/Paused streams marked Cancelled on drain
- [ ] `emergency_drain_proposed` and `emergency_drained` events emitted
- [ ] Two-of-two confirmation pattern enforced
- [ ] `docs/security/threat-model.md` updated with drain procedure
- [ ] Tests: drain succeeds when paused, drain rejected when not paused, drain rejected without proposal
