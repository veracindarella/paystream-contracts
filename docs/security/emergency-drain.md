# Emergency Drain Procedure (SEC-03)

**Issue:** #32, #266, #496  
**Status:** Design (stream contract) / Implemented (token contract)  
**Priority:** High

---

## Overview

In the event of a critical vulnerability, the admin needs a last-resort mechanism to recover
all tokens held by the contract and distribute them to stream owners. Without this, a hard-paused
contract leaves deposited tokens locked permanently.

The `emergency_drain` function provides this capability with strong safeguards to prevent abuse.

The same mechanism is mirrored in the **token contract** (TOK-30 / #496) so that tokens held
by the token contract can be recovered under identical safeguards. See
[Token Contract Drain](#token-contract-drain-tok-30-496) below.

---

## Safeguards

| Safeguard | Rationale |
|-----------|-----------|
| Contract must be hard-paused first | Prevents drain during normal operation |
| Requires two-of-two admin confirmation (propose + execute) | No single transaction can drain; prevents key-compromise drain |
| Minimum 24-hour time-lock between propose and execute | Gives the community time to detect and respond to a malicious drain |
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
- Stores `recipient`, current nonce, and the proposal timestamp (`env.ledger().timestamp()`)
  in instance storage under `DataKey::PendingDrain`
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
- Rejects with `ERR_DRAIN_TIMELOCK_ACTIVE` if fewer than 86_400 seconds (24 hours) have
  elapsed since the proposal timestamp
- Iterates all streams (via `StreamCount`), marks Active/Paused streams as Cancelled
- Queries contract token balance for every unique token address held
- Transfers full balance of each token to the stored `recipient`
- Emits `emergency_drained(recipient, token, amount)` per token
- Clears `DataKey::PendingDrain`

---

## Time-Lock

A minimum delay of **86_400 seconds (24 hours)** is enforced between `propose_emergency_drain`
and `emergency_drain`. The proposal timestamp is recorded at propose time and compared against
`env.ledger().timestamp()` at execute time. If the elapsed time is less than the delay, the
drain is rejected with `ERR_DRAIN_TIMELOCK_ACTIVE`.

This window gives monitoring infrastructure and the community time to detect a malicious
proposal and respond (e.g. rotate the admin key, coordinate a counter-proposal) before any
tokens can be moved.

---

## New Storage Keys

```rust
// In types.rs DataKey enum:
/// Pending emergency drain: stores (recipient, nonce, proposed_at) set by propose_emergency_drain.
PendingDrain,
```

---

## New Error

```rust
/// Emergency drain cannot execute until the time-lock has elapsed.
pub const ERR_DRAIN_TIMELOCK_ACTIVE: u32 = ...;
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
- The 24-hour time-lock further widens the response window: even after a malicious proposal,
  no tokens can move until the delay has elapsed, giving responders time to act
- The drain can only proceed when `DataKey::Paused == true`, so it cannot be used as a
  silent rug-pull during normal operation
- Iterating all streams to mark them Cancelled may hit Soroban's per-transaction instruction
  limit for contracts with thousands of streams; a paginated version (drain in batches) may
  be needed for large deployments

---

## Token Contract Drain (TOK-30 / #496)

The token contract implements an emergency drain aligned with the stream contract's SEC-03
procedure. It exists for recovery scenarios where tokens held by the token contract must be
moved to a safe recipient after a critical vulnerability is discovered.

### Safeguards

| Safeguard | Rationale |
|-----------|-----------|
| Contract must be paused first | Prevents drain during normal operation |
| Requires two-of-two admin confirmation (propose + execute) | No single transaction can drain; prevents key-compromise drain |
| Only callable by the admin multi-sig | Ensures the drain requires the multi-sig authority, not a single key |
| Minimum 24-hour time-lock between propose and execute | Gives the community time to detect and respond to a malicious drain |
| Emits events on propose and execute | Creates an immutable audit trail |
| Nonce consumed on propose and execute | Replay protection on both steps |

### API

```rust
pub fn propose_emergency_drain(
    env: Env,
    admin: Address,
    nonce: u64,
    recipient: Address,
)

pub fn emergency_drain(
    env: Env,
    admin: Address,
    nonce: u64,
)
```

- Both functions require admin auth (admin multi-sig)
- `propose_emergency_drain` requires the contract to be paused, stores `recipient`, nonce and
  the proposal timestamp under `DataKey::PendingDrain`, and emits
  `emergency_drain_proposed(recipient)`
- `emergency_drain` verifies the pending proposal and nonce, rejects with
  `ERR_DRAIN_TIMELOCK_ACTIVE` if fewer than 86_400 seconds have elapsed since the proposal,
  transfers the contract's token balance to the stored `recipient`, emits
  `emergency_drained(recipient, token, amount)`, and clears `DataKey::PendingDrain`

### Time-Lock

A minimum delay of **86_400 seconds (24 hours)** is enforced between
`propose_emergency_drain` and `emergency_drain`, identical to the stream contract. The proposal
timestamp is recorded at propose time and compared against `env.ledger().timestamp()` at
execute time.

### New Storage Key

```rust
// In types.rs DataKey enum:
/// Pending emergency drain: stores (recipient, nonce, proposed_at) set by propose_emergency_drain.
PendingDrain,
```

### New Error

```rust
/// Emergency drain cannot execute until the time-lock has elapsed.
pub const ERR_DRAIN_TIMELOCK_ACTIVE: u32 = ...;
```

### New Events

```
emergency_drain_proposed(recipient: Address)
emergency_drained(recipient: Address, token: Address, amount: i128)
```

---

## Threat Model Update

Once implemented, update `docs/security/threat-model.md` section 4 (Residual Risks) to reflect
that a contract-pause + emergency drain procedure is available to the admin.

---

## Acceptance Criteria

### Stream contract (SEC-03)

- [ ] `DataKey::PendingDrain` added to `types.rs`
- [ ] `propose_emergency_drain` implemented (requires paused contract + admin auth + nonce)
- [ ] `propose_emergency_drain` records the proposal timestamp
- [ ] `emergency_drain` implemented (requires pending drain + admin auth + nonce)
- [ ] `emergency_drain` rejects with `ERR_DRAIN_TIMELOCK_ACTIVE` if < 86_400 seconds elapsed
- [ ] `ERR_DRAIN_TIMELOCK_ACTIVE` error constant added
- [ ] All Active/Paused streams marked Cancelled on drain
- [ ] `emergency_drain_proposed` and `emergency_drained` events emitted
- [ ] Two-of-two confirmation pattern enforced
- [ ] `docs/security/threat-model.md` updated with drain procedure
- [ ] Tests: drain succeeds when paused, drain rejected when not paused, drain rejected without proposal
- [ ] Tests: drain before time-lock panics, drain after time-lock succeeds

### Token contract (TOK-30 / #496)

- [x] `DataKey::PendingDrain` added to `types.rs`
- [x] `propose_emergency_drain` implemented (requires paused contract + admin auth + nonce)
- [x] `propose_emergency_drain` records the proposal timestamp
- [x] `emergency_drain` implemented (requires pending drain + admin auth + nonce)
- [x] `emergency_drain` rejects with `ERR_DRAIN_TIMELOCK_ACTIVE` if < 86_400 seconds elapsed
- [x] `ERR_DRAIN_TIMELOCK_ACTIVE` error constant added
- [x] Drain callable only by the admin multi-sig
- [x] `emergency_drain_proposed` and `emergency_drained` events emitted
- [x] Two-of-two confirmation pattern enforced
- [x] Documented in `docs/security/emergency-drain.md`
- [x] Tests: full drain flow (propose → time-lock → execute) covered
