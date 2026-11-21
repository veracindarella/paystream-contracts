# PayStream Threat Model

**Version:** 1.0  
**Date:** 2026-04-24  
**Methodology:** STRIDE  
**Scope:** `paystream-stream` Soroban smart contract

---

## 1. System Overview

PayStream streams salary from an employer to an employee in real-time on the Stellar blockchain. The core contract (`contracts/stream`) holds deposited tokens in escrow and releases them per-second to the employee.

### Actors

| Actor | Trust Level | Description |
|---|---|---|
| Admin | High (privileged) | Deploys and configures the contract; can pause/unpause and upgrade |
| Employer | Medium | Creates streams, deposits funds, can pause/resume/cancel their own streams |
| Employee | Low | Withdraws earned tokens from streams assigned to them |
| External attacker | None | Any party with no legitimate role |
| Token contract | External | SEP-41 fungible token; assumed honest but treated as untrusted for callbacks |

### Trust Boundaries

```
[Stellar Network / Ledger]
        │
        ▼
[PayStream Stream Contract]  ←── Admin (privileged ops)
        │                    ←── Employer (stream lifecycle)
        │                    ←── Employee (withdraw)
        ▼
[SEP-41 Token Contract]      ←── External (transfer calls)
```

---

## 2. Assets

| Asset | Sensitivity | Notes |
|---|---|---|
| Deposited tokens | Critical | Locked in contract escrow; loss = direct financial harm |
| Admin key | Critical | Controls pause, upgrade, min-deposit; compromise = full contract control |
| Stream state | High | Manipulation could allow over-withdrawal or fund lock |
| Admin nonce | Medium | Replay protection counter; must be monotonically increasing |
| Contract WASM | High | Upgrade replaces all logic; malicious WASM = total compromise |

---

## 3. STRIDE Threat Analysis

### 3.1 Spoofing

| ID | Threat | Affected Component | Mitigation |
|---|---|---|---|
| S-01 | Attacker impersonates employer to cancel/pause a stream | `cancel_stream`, `pause_stream` | `employer.require_auth()` + `assert_eq!(stream.employer, employer)` |
| S-02 | Attacker impersonates employee to withdraw funds | `withdraw` | `employee.require_auth()` + `assert_eq!(stream.employee, employee)` |
| S-03 | Attacker impersonates admin to pause contract or upgrade WASM | `pause_contract`, `upgrade` | `admin.require_auth()` + stored admin address check |

### 3.2 Tampering

| ID | Threat | Affected Component | Mitigation |
|---|---|---|---|
| T-01 | Attacker manipulates stream state (e.g., sets `withdrawn = 0`) | Persistent storage | Soroban storage is only writable by the owning contract; no external write path |
| T-02 | Integer overflow in `claimable_amount` produces wrong payout | `storage::claimable_amount` | `checked_mul` panics on overflow (E004); `saturating_sub` for elapsed time |
| T-03 | Deposit underflow allows over-withdrawal | `withdraw`, `top_up` | `checked_add` on `withdrawn`; `min(earned, remaining)` caps payout |
| T-04 | Malicious token contract returns false balance to bypass SEP-41 probe | `create_stream` | `token_client.balance()` call panics if token is non-conformant |

### 3.3 Repudiation

| ID | Threat | Affected Component | Mitigation |
|---|---|---|---|
| R-01 | Employer denies creating a stream | `create_stream` | `stream_created` event published on-chain; immutable ledger record |
| R-02 | Employee denies withdrawing funds | `withdraw` | `withdrawn` event published on-chain |
| R-03 | Admin denies pausing or upgrading | `pause_contract`, `upgrade` | `contract_paused` event; upgrade recorded in ledger history |

### 3.4 Information Disclosure

| ID | Threat | Affected Component | Mitigation |
|---|---|---|---|
| I-01 | Stream balances and rates are visible to all | All read functions | Accepted: Stellar is a public ledger; no private data stored |
| I-02 | Admin address is discoverable | `DataKey::Admin` in instance storage | Accepted: admin address is not secret; auth is enforced cryptographically |

### 3.5 Denial of Service

| ID | Threat | Affected Component | Mitigation |
|---|---|---|---|
| D-01 | Attacker creates millions of streams to exhaust stream index | `create_stream` | Min-deposit requirement (E007) raises cost per stream; index is per-employer/employee |
| D-02 | Stream data expires from Soroban persistent storage | All stream operations | TTL extended to ~2 years on every `save_stream` / `load_stream` call (issue #73) |
| D-03 | Admin key compromise allows permanent contract pause | `pause_contract` | Nonce prevents replay of a captured pause transaction (issue #70); key rotation via `upgrade` |
| D-04 | Employer creates a stream with `rate = 0`, locking deposit forever | `create_stream` | `validate_create_stream` rejects `rate_per_second = 0` (E001) |
| D-05 | Employer creates a stream with `deposit = 0` | `create_stream` | `validate_create_stream` rejects `deposit ≤ 0` (E002) and `deposit < min_deposit` (E007) |

### 3.6 Elevation of Privilege

| ID | Threat | Affected Component | Mitigation |
|---|---|---|---|
| E-01 | Employee calls `cancel_stream` to reclaim employer funds | `cancel_stream` | `assert_eq!(stream.employer, employer)` — only employer can cancel |
| E-02 | Employer calls `withdraw` to drain employee earnings | `withdraw` | `assert_eq!(stream.employee, employee)` — only employee can withdraw |
| E-03 | Non-admin calls `upgrade` to replace contract WASM | `upgrade` | `admin.require_auth()` + stored admin check |
| E-04 | Reentrancy via malicious token callback drains contract | `withdraw` | `stream.locked` guard set before cross-contract call; released after (E003) |
| E-05 | Replay of a captured admin transaction (e.g., pause) | `pause_contract`, `unpause_contract`, `set_min_deposit`, `upgrade` | Monotonic nonce consumed atomically on each admin call (E009) |
| E-06 | Employer == employee allows self-streaming to game accounting | `create_stream` | `validate_create_stream` rejects `employer == employee` |

---

## 4. Auto Top-Up Threat Analysis (SEC-26 / issue #264)

The auto top-up feature (`set_auto_topup`, `cancel_auto_topup`, `maybe_auto_topup`) introduces
a new trust surface: the employer pre-approves the stream contract as a `transfer_from` spender
for up to `max_total` tokens. After each `withdraw`, the contract autonomously pulls funds from
the employer's token account when the remaining balance falls below `trigger_threshold`.

### 4.1 New Attack Surface

```
Employer account  ──(pre-approved allowance)──▶  StreamContract
                                                      │
                                                      │ transfer_from on withdraw
                                                      ▼
                                                Employee account
```

Any actor that can manipulate the contract's auto top-up state (trigger, amount, cap) could
cause the contract to drain more from the employer than intended, or cause it to never top up
(locking employee earnings).

### 4.2 STRIDE Threats — Auto Top-Up

#### Spoofing

| ID | Threat | Affected Function | Mitigation |
|---|---|---|---|
| AT-S-01 | Attacker impersonates employer to configure or cancel auto top-up | `set_auto_topup`, `cancel_auto_topup` | `employer.require_auth()` + `assert_eq!(stream.employer, employer)` — only the stream's actual employer can set or revoke config |

#### Tampering

| ID | Threat | Affected Function | Mitigation |
|---|---|---|---|
| AT-T-01 | **Malicious cap manipulation** — attacker raises `max_total` to drain employer allowance beyond the intended cap | `set_auto_topup` | Config is only writable by the authenticated employer; `max_total` is stored per-stream and capped to the employer's own intent. Employers should set token allowance ≤ `max_total`. |
| AT-T-02 | **Allowance drain** — employer's token approval is higher than `max_total`; contract pulls more than intended if `total_topped_up` accounting is corrupted | `maybe_auto_topup` | `amount = topup_amount.min(max_total - total_topped_up)` — running total is tracked and the delta is capped. `total_topped_up` is only written inside `set_auto_topup` and `maybe_auto_topup`; both require the stream to be loaded and saved atomically. |
| AT-T-03 | Replay of a cancelled auto top-up configuration | `cancel_auto_topup`, `set_auto_topup` | `cancel_auto_topup` calls `remove_auto_topup` which deletes the key; re-enabling requires a fresh `set_auto_topup` call from the employer |

#### Repudiation

| ID | Threat | Affected Function | Mitigation |
|---|---|---|---|
| AT-R-01 | Employer denies enabling auto top-up | `set_auto_topup` | `auto_topup_set(stream_id, true)` event emitted on-chain |
| AT-R-02 | Employer denies a top-up transfer occurred | `maybe_auto_topup` | `auto_topped_up(stream_id, amount, total_topped_up)` event emitted after each successful pull |

#### Information Disclosure

| ID | Threat | Affected Function | Mitigation |
|---|---|---|---|
| AT-I-01 | Auto top-up config (trigger, amount, cap) is visible to all | `get_auto_topup_config` | Accepted: Stellar is a public ledger. Employers should treat their top-up strategy as public. |

#### Denial of Service

| ID | Threat | Affected Function | Mitigation |
|---|---|---|---|
| AT-D-01 | **`stop_time` bypass** — auto top-up fires after `stop_time` has passed, extending a stream that should have ended | `maybe_auto_topup` | `if stream.stop_time > 0 && now >= stream.stop_time { return; }` — top-up is skipped when stop_time is reached |
| AT-D-02 | Attacker cancels employer's auto top-up to starve the stream | `cancel_auto_topup` | Only the authenticated employer can cancel; employee has no access |
| AT-D-03 | Insufficient employer token balance causes `transfer_from` to fail and silently skip the top-up | `maybe_auto_topup` | `try_transfer_from` is used; failure returns early without reverting the withdraw so the employee still receives their earned funds |

#### Elevation of Privilege

| ID | Threat | Affected Function | Mitigation |
|---|---|---|---|
| AT-E-01 | Employee triggers auto top-up to exceed `max_total` cap | `maybe_auto_topup` (called from `withdraw`) | Cap is enforced: `amount = topup_amount.min(max_total - total_topped_up)`; once `total_topped_up >= max_total` the amount is 0 and the top-up is skipped |
| AT-E-02 | Recursive top-up: `maybe_auto_topup` calls `transfer_from` which re-enters `withdraw` | `maybe_auto_topup` | `stream.locked = true` set before `token::transfer`; a re-entrant `withdraw` call panics with E003. Additionally, `maybe_auto_topup` does not call `withdraw` — it calls `transfer_from` directly. |

### 4.3 Mitigations Summary

| Mitigation | Where Enforced |
|---|---|
| **Cap enforcement** (`max_total`) | `maybe_auto_topup`: `amount = topup_amount.min(max_total - total_topped_up)` |
| **`stop_time` check** | `maybe_auto_topup`: early return if `now >= stream.stop_time` |
| **Allowance check** (non-reverting) | `maybe_auto_topup`: `try_transfer_from` returns without panic if allowance/balance is insufficient |
| **Auth on config writes** | `set_auto_topup` / `cancel_auto_topup`: `employer.require_auth()` + stream ownership check |
| **Reentrancy guard** | `maybe_auto_topup` is called while `stream.locked = true` from `withdraw` / `withdraw_all` |
| **Event trail** | `auto_topup_set`, `auto_topped_up` events for auditability |

### 4.4 Residual Risks — Auto Top-Up

| ID | Risk | Likelihood | Impact | Notes |
|---|---|---|---|---|
| AT-RR-01 | Employer sets token allowance >> `max_total` | Medium | Medium | Not a contract bug — employers must set allowance ≤ `max_total`. Documented in API reference. |
| AT-RR-02 | Employer's account is compromised; attacker replaces auto top-up config with a large `max_total` | Low | High | Mitigated by the same admin key rotation procedure; see `docs/security/key-rotation.md` |
| AT-RR-03 | `trigger_threshold` set very high causes constant top-ups, draining `max_total` rapidly | Low | Low | Employer controls all three parameters; no third party can change them |

---

## 5. Front-Running Analysis — Admin Nonce & Two-Step Transfers (SEC-40 / #586)

Admin operations are authorised by `admin.require_auth()` **and** bound to a monotonically
increasing nonce (`DataKey::AdminNonce`, consumed via `consume_admin_nonce` in `storage.rs`).
This section analyses whether an attacker who observes a pending admin transaction in the
mempool can front-run it, replay it, or substitute their own operation.

### 5.1 Operations in Scope

| Operation | Nonce binding | Notes |
|---|---|---|
| `pause_contract` / `unpause_contract` | Nonce parameter, consumed atomically | Replay of a captured pause is rejected (E009) |
| `set_min_deposit` | Nonce parameter, consumed atomically | Same protection as pause |
| `upgrade` (timelock) | Nonce parameter, consumed atomically | Timelock delay applies *in addition* to the nonce |
| `propose_admin` | Nonce parameter, consumed atomically | Stores the **next** nonce expected by `accept_admin` |
| `accept_admin` | Nonce must equal the nonce stored at proposal time | Two-step handover; see 5.3 |

### 5.2 Nonce / Replay Protection

- The nonce is stored in instance storage and incremented **inside the same invocation** that
  performs the privileged action. Soroban executes a contract invocation atomically, so either
  the state change and the nonce increment both commit or neither does.
- A captured (signed) admin transaction cannot be re-submitted: the second submission presents
  a nonce that is now stale and is rejected with **E009** (`InvalidAdminNonce`).
- A captured transaction cannot be *reordered* ahead of the legitimate one to change its effect:
  the operation is fully specified by the transaction payload (function, args, nonce), so
  front-running it with the same payload simply consumes the nonce — the legitimate submission
  then fails with E009 and the admin re-issues with the next nonce. No unauthorised state change
  occurs because the front-runner cannot produce `admin.require_auth()`.
- Because authorisation is cryptographic (`require_auth`), an attacker cannot substitute a
  *different* operation under the same nonce: any altered payload fails auth.

### 5.3 Two-Step Admin Handover (`propose_admin` / `accept_admin`)

```
Admin A ──propose_admin(new_admin = B, nonce = n)──▶ Contract
                                                     │ stores PendingAdmin = B
                                                     │ stores PendingAdminNonce = n + 1
                                                     ▼
Admin B ──accept_admin(nonce = n + 1)──────────────▶ Contract
                                                     │ B.require_auth()
                                                     │ nonce must == PendingAdminNonce
                                                     │ Admin := B, pending cleared
```

- **`propose_admin`** requires `A.require_auth()` and consumes nonce `n`. It records the
  proposed admin **and** the nonce that `accept_admin` must present (`n + 1`).
- **`accept_admin`** requires `B.require_auth()` and must present exactly the nonce stored at
  proposal time. A mismatched nonce is rejected with **E024** (`InvalidPendingAdminNonce`).
- **Front-running `accept_admin`:** an attacker who sees B's pending `accept_admin` cannot
  replay it (nonce consumed on first execution) and cannot execute it themselves (they cannot
  satisfy `B.require_auth()`). Executing it *early* on B's behalf is harmless — the effect is
  exactly the handover B intended.
- **Front-running `propose_admin`:** an attacker cannot forge a competing proposal (needs
  `A.require_auth()`). If the attacker replays A's captured proposal, the nonce is consumed and
  the legitimate proposal fails with E009; A re-issues with the next nonce. The pending admin
  recorded is still A's intended choice.
- **Stale proposal:** if A proposes B and later changes their mind, A can re-propose C with a
  fresh nonce; the stored `PendingAdminNonce` is overwritten, so B's previously observed
  `accept_admin` nonce becomes stale and is rejected with E024.

### 5.4 Upgrade Timelock

- `upgrade` is bound to the admin nonce (E009 on replay) **and** gated by the timelock: the
  WASM hash must have been scheduled at least `timelock_delay` ledgers earlier.
- Front-running an `upgrade` cannot shorten the delay: the scheduled ledger is written when the
  upgrade is queued, and the delay check is evaluated against the current ledger at execution.
- An attacker replaying a captured `upgrade` is rejected by the nonce; an attacker attempting to
  queue a *different* WASM hash fails `admin.require_auth()`.
- The timelock gives users an observation window; a malicious queued upgrade is visible on-chain
  before it can execute, allowing exit or governance response.

### 5.5 Edge Cases — Stale Nonce

| ID | Edge case | Behaviour |
|---|---|---|
| FR-01 | Two admin transactions signed with the same nonce (e.g., duplicated submission) | First commits; second rejected with **E009** |
| FR-02 | Admin signs nonce `n` but a prior transaction already advanced the counter to `n + 1` | Rejected with **E009**; admin must re-read the current nonce and re-sign |
| FR-03 | `accept_admin` presented with a nonce other than the one stored at proposal time | Rejected with **E024** |
| FR-04 | `accept_admin` presented after the proposal was superseded by a newer `propose_admin` | Rejected with **E024** (stored nonce was overwritten) |
| FR-05 | `accept_admin` presented with no pending proposal | Rejected (no `PendingAdmin` / `PendingAdminNonce` in storage) |
| FR-06 | Nonce gap (admin skips a value) | Rejected — nonce must equal the stored value exactly; no gaps permitted |
| FR-07 | Concurrent proposals from the same admin in the same ledger | Only one can commit; the other fails E009 and must be re-issued |

### 5.6 Supporting Evidence

- Unit tests in `contracts/stream/src/test.rs` cover replay rejection (E009) and mismatched
  pending-admin nonce (E024).
- The `fuzz_admin_nonce` target (`contracts/stream/fuzz/`) exercises arbitrary nonce sequences
  against `pause_contract`, `propose_admin`, and `accept_admin`, asserting that no sequence of
  replayed or out-of-order nonces can produce an unauthorised state change.

### 5.7 Conclusion

The admin nonce binds every privileged operation to a single, ordered execution and provides
replay protection; the two-step handover additionally binds `accept_admin` to the nonce recorded
at proposal time (E024). Combined with `require_auth()` and the upgrade timelock, front-running
of admin operations yields at most a denial of the *specific* pending transaction (recoverable by
re-issuing with the next nonce), never an unauthorised state change.

---

## 6. Residual Risks
=======

| ID | Risk | Likelihood | Impact | Notes |
|---|---|---|---|---|
| RR-01 | Admin key compromise | Low | Critical | Mitigated by nonce (replay protection) but not by multi-sig; **for mainnet, use a Stellar native multisig or deployed multisig contract as the admin** — see `docs/security/admin-multisig.md` (SEC-01 / #30) |
| RR-02 | Malicious token contract with re-entrancy hook | Very Low | High | Soroban host prevents true re-entrancy today; `locked` flag is defence-in-depth |
| RR-03 | Stellar network halt causes stream data to expire before TTL extension | Very Low | Medium | TTL set to ~2 years; network halts of that duration are not credible |
| RR-04 | `stop_time` manipulation by employer (set far future) | Low | Low | Employee can still withdraw at any time; no harm from a far-future stop_time |

---

## 7. Security Controls Summary

| Control | Issue | Implementation |
|---|---|---|
| Admin nonce (replay protection) | #70 | `consume_admin_nonce` in `storage.rs`; nonce param on `pause_contract`, `unpause_contract`, `set_min_deposit`, `upgrade` |
| Centralized input validation | #72 | `validate.rs` — `validate_create_stream`, `validate_top_up`; wired into all entry points |
| Persistent storage TTL hardening | #73 | `extend_ttl` on every `save_stream`, `load_stream`, and index write; threshold ~1 yr, extend to ~2 yr |
| Reentrancy guard | Pre-existing | `stream.locked` flag set before `token::transfer`, released after |
| Overflow protection | Pre-existing | `checked_mul` / `checked_add` throughout; `saturating_sub` for elapsed time |
| Auth enforcement | Pre-existing | `require_auth()` on every state-mutating entry point |
| On-chain event log | Pre-existing | All state changes emit events for auditability |
| Emergency drain (planned) | #32 (SEC-03) | `propose_emergency_drain` + `emergency_drain` two-step; requires contract paused; see `docs/security/emergency-drain.md` |
| Auto top-up cap + stop_time guard | #264 (SEC-26) | `maybe_auto_topup` enforces `max_total` cap and skips after `stop_time`; see Section 4 |
| Rate-limit on stream creation | #262 (SEC-24) | `check_and_increment_create_rate` in `lib.rs`; `set_max_creates_per_ledger` admin function |
| Two-step admin handover with nonce binding | #586 (SEC-40) | `propose_admin` stores `PendingAdminNonce`; `accept_admin` requires it (E024); see Section 5 |

---

## 8. Out of Scope

- Stellar validator security and consensus
- Key management practices for the admin keypair
- Front-end / off-chain components
- Token contract internals (treated as a trusted SEP-41 implementation)
