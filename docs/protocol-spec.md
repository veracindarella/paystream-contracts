# PayStream Protocol Specification

Status: **Draft**. Must be reviewed and signed off before mainnet launch.

This document describes PayStream as an abstract state machine, independent of Soroban or Rust. It is meant for alternative implementations, formal verification and regulatory review. Where this spec and the reference implementation (`contracts/stream`) disagree, file an issue.

## 1. Notation

- `t` — current time (ledger timestamp, seconds). Time never decreases.
- `S` — a stream record. Fields are accessed as `S.field`.
- `min(a, b)`, `max(a, b)` — usual meaning. All amounts are integers in token base units.
- `⊥` — "operation rejected"; a rejected operation changes no state.
- `auth(x)` — the operation is authorised by party `x`.

## 2. State

Global state:

| Symbol | Meaning |
|--------|---------|
| `admin` | Admin party |
| `paused` ∈ {true, false} | Global pause flag |
| `minDeposit` | Minimum deposit for new streams |
| `streams : id → S` | Stream records |

Stream record `S`:

| Field | Meaning |
|-------|---------|
| `employer`, `employee` | Parties |
| `token` | Token being streamed |
| `deposit` | Total funded amount (D) |
| `withdrawn` | Total paid to employee (W) |
| `rate` | Tokens per second (r > 0) |
| `start` | Start time |
| `stop` | Hard stop time, or 0 for none |
| `last` | Time accrual was last settled |
| `banked` | Accrual banked at an earlier rate (B) |
| `status` ∈ {Active, Paused, Cancelled, Exhausted} | Lifecycle state |
| `metadata : key → value` | Optional bounded map of short string pairs (e.g. `department`, `cost_center`) for off-chain categorisation |
| `splits` | Optional list of `(beneficiary, bps)` pairs; empty for non-split streams |

### 2.1 Metadata

`metadata` is a small map of short string keys to short string values, attached at create time and replaceable on update. It is purely descriptive: it never affects accrual, claimable amounts or payouts, and it is readable via a query. The map is bounded: at most `MAX_METADATA_ENTRIES` entries, each key and value at most `MAX_METADATA_LEN` bytes. An operation that would exceed either cap is rejected (`⊥`). Storing metadata must not change any other field of `S`.

### 2.2 Split beneficiaries

A stream may carry `splits`, a list of `(beneficiary, bps)` pairs where each `bps` is in basis points (1 bps = 0.01%). Splits are allowed on payroll streams and on linear vest streams. The list is valid when it is non-empty, every `bps > 0`, every beneficiary is distinct, and `Σ bps ≤ 10000`. The remainder `10000 − Σ bps` (if any) accrues to `S.employee`. When `splits` is empty, the whole claimable amount goes to `S.employee`.
=======

## 3. Accrual formula

Let `end(t) = min(t, S.stop)` if `S.stop > 0`, else `t`.

```
earned(S, t)    = r · max(0, end(t) − S.last) + S.banked        if status ∈ {Active, Paused}
claimable(S, t) = min(earned(S, t), D − W)                      if status ∈ {Active, Paused}
claimable(S, t) = 0                                             if status ∈ {Cancelled, Exhausted}
```

Arithmetic overflow rejects the operation (`⊥`). It never wraps.

For a split stream, the claimable amount is divided among the beneficiaries. For each `(b, bps)` in `S.splits`:

```
share(b, S, t) = floor(claimable(S, t) · bps / 10000)
```

and the employee receives `claimable(S, t) − Σ share(b, S, t)`. Rounding dust therefore always stays with the employee, so the shares plus the employee's portion sum exactly to `claimable(S, t)`.

## 4. Operations

### 4.1 Create

`create(employer, employee, token, D, r, stop)` requires `auth(employer)`, `¬paused`, `D ≥ minDeposit`, `D > 0`, `r > 0`, and `stop = 0 ∨ stop > t`.

Effect: `D` tokens move from employer to the protocol. A new `S` is stored with `W = 0`, `B = 0`, `start = last = t`, `status = Active`, empty `splits`, and the supplied `metadata` (default empty).

`createSplit(employer, employee, token, D, r, stop, splits, metadata)` is the same operation with the additional requirements that `splits` is valid per §2.2 and `metadata` satisfies the caps in §2.1. It is available for payroll streams and for linear vest streams; the vest variant (`createSplitVest`) additionally requires `stop > t` and applies the same split rules to vesting claimable amounts.

### 4.2 Withdraw

`withdraw(S)` requires `auth(S.employee)`, `¬paused`, and `S.status ∈ {Active, Exhausted}`.

Effect: let `c = claimable(S, t)`. Then `c` tokens are distributed: each split beneficiary `b` receives `share(b, S, t)` and the employee receives the remainder. `W ← W + c`, `last ← end(t)`, `B ← 0`. If `W = D`, then `status ← Exhausted`.

### 4.3 Top up

`topUp(S, a)` requires `auth(S.employer)`, `a > 0`, and `S.status ∈ {Active, Paused}`. Effect: `D ← D + a`.

### 4.3.1 Metadata update

`updateMetadata(S, metadata)` requires `auth(S.employer)` and `S.status ∈ {Active, Paused}`. The new map must satisfy the caps in §2.1. Effect: `S.metadata ← metadata`. No other field changes and no tokens move.

### 4.4 Rate update

`updateRate(S, r')` requires `auth(S.employer)` and `r' > 0`. Effect: `B ← earned(S, t)`, `last ← t`, `r ← r'`. Accrual earned at the old rate is kept.

### 4.5 Pause / resume (per stream)

- `pause(S)` requires `auth(S.employer)` and `status = Active`. Effect: `B ← earned(S, t)`, `last ← t`, `status ← Paused`.
- `resume(S)` requires `auth(S.employer)` and `status = Paused`. Effect: `last ← t`, `status ← Active`.

No accrual happens between `pause` and `resume`.

### 4.6 Global pause

`pauseContract` / `unpauseContract` require `auth(admin)`. Effect: set `paused`. While `paused`, state-changing user operations (create, withdraw) are rejected. Reads still work and no stream fields change.

### 4.7 Cancel

`cancel(S)` requires `auth(S.employer)` and `status ∈ {Active, Paused}`. Effect: let `c = claimable(S, t)`. The employee receives `c`, the employer receives `D − W − c`, and `status ← Cancelled`.

### 4.8 Expire

`expire(S)` requires `auth(admin)`, `S.stop > 0`, `t > S.stop + G` where `G` is the grace period (7 days), `status = Active`, and `claimable(S, t) = 0`. Effect: `status ← Exhausted`.

## 5. State machine

```
           pause            resume
 Active ─────────► Paused ─────────► Active
   │  │                │
   │  └── cancel ──────┴──► Cancelled   (terminal)
   │
   └── W = D or expire ──► Exhausted    (terminal)
```

## 6. Invariants

For every stream `S` and every reachable state:

1. **Solvency:** `0 ≤ W ≤ D`.
2. **Bounded claim:** `claimable(S, t) ≤ D − W`.
3. **No over-withdrawal:** the employee has received exactly `W` in total, so the sum of all withdrawals is at most `D`.
4. **Earned funds are protected from the employer:** no operation authorised only by the employer moves `claimable(S, t)` to anyone but the employee. Cancel pays `claimable` to the employee before refunding the employer.
5. **Monotonic accrual:** for `t₁ ≤ t₂` with no operations in between, `earned(S, t₁) ≤ earned(S, t₂)`.
6. **Terminal states:** `Cancelled` and `Exhausted` have `claimable = 0` and accept no further accrual.
7. **Conservation:** the protocol's token balance is at least `Σ (D − W)` over all non-terminal streams.
8. **Split conservation:** for a split stream, the sum of all beneficiary shares plus the employee's portion equals `claimable(S, t)` exactly; no share exceeds its basis-point fraction of `claimable(S, t)`.
9. **Metadata isolation:** `metadata` never influences accrual, claimable amounts, splits or payouts, and satisfies the caps in §2.1 in every reachable state.

## 7. Sign-off

| Role | Name | Date |
|------|------|------|
| Protocol lead | | |
| Security reviewer | | |
