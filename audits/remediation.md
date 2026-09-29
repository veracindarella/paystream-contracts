# Audit Remediation Summary

**Audit:** Trail of Bits — April 2026  
**Full report:** [2026-04-trail-of-bits.md](2026-04-trail-of-bits.md)  
**Last updated:** 2026-04-24

---

## Status Overview

| ID      | Severity | Title                                                        | Status   |
|---------|----------|--------------------------------------------------------------|----------|
| HIGH-01 | High     | Missing employer identity check in `top_up`                  | ✅ Resolved |
| MED-01  | Medium   | Integer overflow in `claimable_amount`                       | ✅ Resolved |
| MED-02  | Medium   | Dual transfer in `cancel_stream` without reentrancy note     | ✅ Resolved |
| LOW-01  | Low      | `stop_time` does not auto-transition to `Exhausted`          | ✅ Accepted |
| LOW-02  | Low      | `initialize` can be called multiple times                    | 🔲 Open   |
| INFO-01 | Info     | `topped_up` event presence confirmed                         | ✅ N/A    |

---

## Resolved Findings

### HIGH-01 — Missing employer identity check in `top_up`

**Fix:** Added `assert_eq!(stream.employer, employer, "not the employer")` in `lib.rs::top_up`
immediately after loading the stream, before the token transfer.  
**Verified:** Present in current code at `contracts/stream/src/lib.rs`.

---

### MED-01 — Integer overflow in `claimable_amount`

**Fix:** Replaced bare `elapsed * rate_per_second` with
`.checked_mul(...).expect(ERR_OVERFLOW)` in `storage.rs::claimable_amount`. Result is capped
by `.min(remaining)`.  
**Verified:** `test_claimable_overflow_panics` confirms the E004 panic fires on overflow.
`test_claimable_large_elapsed_capped_by_deposit` confirms the cap works correctly.

---

### MED-02 — Dual transfer in `cancel_stream` without reentrancy documentation

**Fix:** Added inline comment in `lib.rs::cancel_stream` documenting Soroban's single-threaded
execution model and confirming no re-entrant path exists.  
**Verified:** Comment present in current code.

---

### LOW-01 — `stop_time` does not auto-transition to `Exhausted`

**Decision:** Accepted as-is. `Exhausted` is only set when `withdrawn >= deposit`, which is
the canonical signal for a fully drained stream. Integrators must use `claimable()` to check
remaining value, not `status`. Documented in README.

---

## Open Finding

### LOW-02 — `initialize` can be called multiple times

**Required fix:**
```rust
pub fn initialize(env: Env, admin: Address) {
    assert!(
        !env.storage().instance().has(&DataKey::Admin),
        "already initialized"
    );
    admin.require_auth();
    set_admin(&env, &admin);
}
```

**Blocking:** Yes — this must be resolved before mainnet deployment.  
**Tracking:** Open hardening task in the project backlog.

---

## Deployment Readiness

| Environment | Ready |
|-------------|-------|
| Testnet     | ✅ Yes |
| Mainnet     | ❌ Pending LOW-02 resolution + delta audit (SEC-07) |

---

## Planned: Delta Audit (SEC-07)

A follow-up audit covering all changes since the 2026-04-23 Trail of Bits engagement is
required before mainnet deployment. See [2026-next-audit-scope.md](2026-next-audit-scope.md)
for the full scope, acceptance criteria, and auditor candidates.

When the audit completes, add a new section here following the format above.
