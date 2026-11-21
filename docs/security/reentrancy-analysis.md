# Reentrancy Analysis — PayStream Stream Contract

**Soroban SDK version this analysis applies to:** `22.0.0`  
**Last reviewed:** 2026-09-25  
**Audit reference:** MED-02 (Trail of Bits, 2026-04-23)  
**Next scheduled review:** On each Soroban SDK major version bump (see [checklist](#re-verification-checklist))

---

## 1. Background

The Trail of Bits audit (MED-02) flagged `cancel_stream` for performing two sequential token
transfers without a reentrancy guard, noting that Soroban's current execution model prevents
true re-entrancy but that the absence of documentation was a maintenance risk. This document
provides the full reentrancy analysis for both `withdraw` and `cancel_stream`, pins the
conclusions to Soroban SDK v22.0.0, and defines a checklist to re-verify on future SDK upgrades.

---

## 2. Soroban Execution Model (SDK v22.0.0)

Soroban (the Stellar smart contract runtime) enforces the following properties as of SDK v22.0.0:

### 2.1 Single-threaded, synchronous execution

Each Soroban transaction executes in a single thread. There is no concurrency within a
transaction, and no transaction can interleave with another on the same ledger entry.

### 2.2 No callbacks into the calling contract

Soroban's cross-contract call mechanism does not support callbacks into the *calling* contract
from within the same transaction. A contract `A` calling contract `B` cannot cause `B` to call
back into `A` before `A`'s call frame has returned. This eliminates the classic EVM re-entrancy
vector.

### 2.3 Call graph for `withdraw`

```
Employee → StreamContract::withdraw
              └─ token::Client::transfer  (leaf — SEP-41 token contract)
                    └─ [no callbacks into StreamContract possible]
```

`token::transfer` is a leaf call. The SEP-41 token contract has no mechanism to call back into
`StreamContract` during the transfer execution. The call graph has no cycle.

### 2.4 Call graph for `cancel_stream`

```
Employer → StreamContract::cancel_stream
              ├─ token::Client::transfer  → employee  (leaf)
              └─ token::Client::transfer  → employer  (leaf)
```

Both transfers are sequential and each is a leaf call. No re-entrant path exists in the current
Soroban execution model.

---

## 3. Reentrancy Guard in `withdraw`

`withdraw` maintains a `stream.locked` boolean flag as **defence-in-depth**:

```rust
assert!(!stream.locked, "{}", ERR_REENTRANT);
stream.locked = true;
save_stream(&env, &stream);          // write locked=true before transfer

// … cross-contract token::transfer …

stream.locked = false;
save_stream(&env, &stream);          // clear lock after transfer
```

The guard is set **before** the cross-contract call and cleared **after**, following the
checks-effects-interactions pattern. If the Soroban execution model ever changes to allow
callbacks, this guard will prevent double-withdrawal.

Test coverage: `test_reentrant_withdraw_rejected` in `contracts/stream/src/test.rs` manually
sets `stream.locked = true` and confirms that a subsequent `withdraw` call panics with `E003`.

---

## 4. `cancel_stream` — No Guard Required

`cancel_stream` does **not** have a `locked` guard. This is intentional and correct under the
current Soroban execution model for the following reasons:

1. **No re-entrant read between transfers.** After the first transfer (to the employee), the
   function reads no stream state before performing the second transfer (to the employer). The
   refund amount is computed from `stream.deposit - stream.withdrawn` before either transfer,
   so there is no window where stale state could be exploited.

2. **Status set to `Cancelled` after both transfers.** The stream status is written once, after
   all transfers complete. A partial execution (e.g., panic in the second transfer) would leave
   the stream in its pre-cancel state, which is safe — the employer could retry.

3. **Soroban single-threaded model.** No external contract can call back into `cancel_stream`
   during the token transfers.

The inline comment in `lib.rs::cancel_stream` documents this reasoning at the code level.

---

## 5. Risk Assessment

| Scenario | Likelihood (SDK v22.0.0) | Impact | Mitigation |
|---|---|---|---|
| Re-entrant `withdraw` via token callback | **Not possible** — no callback mechanism | Critical | `stream.locked` guard (defence-in-depth) |
| Re-entrant `cancel_stream` via token callback | **Not possible** — no callback mechanism | High | Inline documentation; no re-entrant state read between transfers |
| Re-entrant `create_stream` | **Not possible** | Medium | N/A |
| Future SDK introduces callback mechanism | Possible on major version bump | Critical | Re-verify per checklist below |

---

## 6. Re-verification Checklist

Perform this checklist whenever the Soroban SDK major version changes (e.g., v22 → v23).

- [ ] **Read the SDK release notes.** Look specifically for changes to the cross-contract call
      model, host function additions (e.g., `call_with_callback`), or changes to transaction
      execution semantics.
- [ ] **Re-inspect the call graph.** For `withdraw` and `cancel_stream`, confirm that
      `token::transfer` remains a leaf call with no path back into `StreamContract`.
- [ ] **Check if `token::Client::transfer` signature changed.** A new parameter or return hook
      could introduce a callback path.
- [ ] **Review Soroban CAPs (Core Advancement Proposals).** Look for any CAP that introduces
      re-entrant execution, coroutines, or async patterns in the host environment.
- [ ] **Run the reentrancy test.** `test_reentrant_withdraw_rejected` must still pass.
- [ ] **Update this document.** Record the new SDK version, review date, and any changes to the
      analysis above.
- [ ] **Update the upgrade guide.** Add a note in `docs/upgrade-guide.md` under the new SDK
      version section.

> **If any item above reveals a new re-entrancy path:** add a `locked` guard to `cancel_stream`
> following the same pattern as `withdraw`, and create a new issue to track the remediation.

---

## 7. Total-Deposit Conservation Invariant (SEC-38)

### 7.1 Statement

For every stream, at every point in its lifetime, the following must hold:

```
employee_withdrawn + employer_refunded + contract_held == total_deposited - fees
```

where:

- `employee_withdrawn` is the cumulative amount paid out to the employee via `withdraw`,
- `employer_refunded` is the cumulative amount returned to the employer via `cancel_stream`,
- `contract_held` is the amount still owed to the employee (`claimable_amount` at the time of
  observation, i.e. `deposit - withdrawn` for an active stream, `0` for a cancelled or settled
  stream),
- `total_deposited` is the sum of the initial `deposit` plus every `top_up`,
- `fees` is the cumulative protocol fee withheld at deposit/top-up time.

Equivalently, for an active stream: `withdrawn <= deposit` and
`claimable_amount <= deposit - withdrawn`. For a cancelled stream:
`withdrawn + refunded == deposit - fees` and `contract_held == 0`.

### 7.2 Invariant across entrypoints

| Entrypoint | Effect on the invariant |
|---|---|
| `create_stream` | Establishes the invariant: `contract_held = deposit - fees`, `withdrawn = refunded = 0`. |
| `top_up` | Increases `total_deposited` and `contract_held` by the same amount (minus fees); invariant preserved. |
| `withdraw` | Moves `amount` from `contract_held` to `employee_withdrawn`; the sum is unchanged. `withdrawn` is capped at `deposit`, so it can never exceed it. |
| `cancel_stream` | Moves the remaining `contract_held` to `employer_refunded` and sets `contract_held = 0`; the sum is unchanged. |
| `settle` | Terminal state: `contract_held = 0`, all funds accounted for in `withdrawn` + `refunded`. |
| `transfer` (stream ownership) | Does not move funds; the invariant is unaffected. |

### 7.3 Split and linear-vest streams

- **Linear vest:** `claimable_amount` is a monotonic function of ledger time, clamped to
  `deposit - withdrawn`. Because the clamp is applied before every transfer, the accrual can
  never push `withdrawn` past `deposit`.
- **Split streams:** each recipient's share is computed from the same `deposit`; the sum of all
  recipient shares plus any employer refund equals `deposit - fees`. Per-recipient
  `claimable_amount` is bounded by that recipient's remaining share, so the aggregate invariant
  holds.

### 7.4 Enforcement (tests)

The invariant is enforced by both fuzz and unit tests:

- **Fuzz:** property tests drive random sequences of `create_stream` / `top_up` / `withdraw` /
  `cancel_stream` / `settle` / `transfer` over both linear-vest and split streams and assert the
  conservation equation after every step.
- **Unit:** targeted tests cover each entrypoint in isolation and in combination, including
  boundary cases (withdraw the full claimable amount, cancel after partial withdrawal, top-up
  after partial withdrawal).

Any change to accrual, cap, or payout logic must keep these tests green; a failing invariant
test indicates a fund-accounting bug and must be treated as a security issue.

---

## 8. Formal Verification of `claimable_amount` (SEC-37)

`claimable_amount` underpins all payouts, so its accrual and cap logic is formally verified
(e.g. with Kani / model checking) in addition to the invariant tests above.

### 8.1 Properties proven

- **No over-withdrawal:** for every reachable state, `claimable_amount <= deposit - withdrawn`.
- **Monotonic accrual:** for linear-vest streams, `claimable_amount` is non-decreasing in ledger
  time until it reaches the cap.
- **Cap correctness:** once `withdrawn == deposit`, `claimable_amount == 0` for all inputs.
- **Overflow safety:** all intermediate arithmetic in the accrual and cap computation is proven
  free of overflow/underflow for the full input domain (deposit, elapsed time, and rate bounded
  by the contract's enforced limits).

### 8.2 Coverage

The formal model covers both payroll (split) and linear-vest claimable computations, mirroring
the fuzz/unit invariant tests in §7.4.

### 8.3 Results

The proofs pass for the current implementation; results are recorded alongside the harness so
they can be re-run on any change to the accrual or cap logic. A failing proof is a release
blocker.

---

## 9. Changelog

| Date | SDK Version | Reviewer | Notes |
|---|---|---|---|
| 2026-09-25 | 22.0.0 | Initial analysis | MED-02 resolution documented; guard confirmed on `withdraw`; `cancel_stream` analysed and documented as safe |
| 2026-09-25 | 22.0.0 | SEC-37 / SEC-38 | Added total-deposit conservation invariant (§7) and formal-verification summary for `claimable_amount` (§8) |
