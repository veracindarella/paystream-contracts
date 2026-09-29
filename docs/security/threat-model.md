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

## 4. Residual Risks

| ID | Risk | Likelihood | Impact | Notes |
|---|---|---|---|---|
| RR-01 | Admin key compromise | Low | Critical | Mitigated by nonce (replay protection) but not by multi-sig; **for mainnet, use a Stellar native multisig or deployed multisig contract as the admin** — see `docs/security/admin-multisig.md` (SEC-01 / #30) |
| RR-02 | Malicious token contract with re-entrancy hook | Very Low | High | Soroban host prevents true re-entrancy today; `locked` flag is defence-in-depth |
| RR-03 | Stellar network halt causes stream data to expire before TTL extension | Very Low | Medium | TTL set to ~2 years; network halts of that duration are not credible |
| RR-04 | `stop_time` manipulation by employer (set far future) | Low | Low | Employee can still withdraw at any time; no harm from a far-future stop_time |

---

## 5. Security Controls Summary

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

---

## 6. Out of Scope

- Stellar validator security and consensus
- Key management practices for the admin keypair
- Front-end / off-chain components
- Token contract internals (treated as a trusted SEP-41 implementation)
