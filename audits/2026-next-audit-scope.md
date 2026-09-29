# Next Security Audit — Scope & Planning

**Status:** Planned  
**Required before:** Mainnet deployment  
**Trigger:** Significant contract changes since the Trail of Bits audit (2026-04-23)

---

## Background

The Trail of Bits audit covered `main` @ HEAD as of 2026-04-23. The following features have
been added or changed since that commit and require a delta audit before mainnet go-live:

| Change | Description |
|--------|-------------|
| Admin nonce | Monotonic nonce on all admin operations (replay protection) |
| `min_deposit` | Admin-configurable minimum deposit enforcement |
| Batch streams | `create_streams_batch` and `cancel_streams_batch` entry points |
| TTL hardening | Extended persistent-storage TTL on every stream read/write |
| Two-step admin transfer | `propose_admin` / `accept_admin` flow |
| `withdraw_all` | Batch employee withdrawal across all streams |
| `update_rate` | Employer can change `rate_per_second` without cancelling |
| `stream_status` / `claimable_at` | New read-only entry points |

---

## Audit Scope

All changes merged to `main` after commit `HEAD@2026-04-23`.

Files in scope:

- `contracts/stream/src/lib.rs`
- `contracts/stream/src/storage.rs`
- `contracts/stream/src/types.rs`
- `contracts/stream/src/events.rs`
- `contracts/stream/src/validate.rs`
- `contracts/stream/src/test.rs`

Files out of scope (unchanged since prior audit):

- `contracts/token/` (no material changes)
- Off-chain scripts and CI configuration

---

## Acceptance Criteria

- [ ] Scope document shared with at least one external auditor
- [ ] Engagement confirmed and dates agreed
- [ ] Audit report delivered and stored in `audits/` following existing naming convention
- [ ] All Critical and High findings resolved before mainnet go-live
- [ ] `audits/remediation.md` updated with new audit results

---

## Auditor Candidates

Preferred firms (in order):

1. Trail of Bits (continuity from prior engagement)
2. OtterSec
3. Halborn

---

## Timeline

| Milestone | Target |
|-----------|--------|
| Scope finalised and RFP sent | TBD |
| Auditor engaged | TBD |
| Audit start | TBD |
| Report delivered | TBD |
| Findings resolved | TBD |
| Mainnet go-live | Blocked until above complete |
