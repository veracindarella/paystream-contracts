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

All changes merged to `main` after commit `HEAD@2026-04-23`, across **both** contracts
(`contracts/stream/` and `contracts/token/`), as required for v0.2 mainnet readiness.

Files in scope:

- `contracts/stream/src/lib.rs`
- `contracts/stream/src/storage.rs`
- `contracts/stream/src/types.rs`
- `contracts/stream/src/events.rs`
- `contracts/stream/src/validate.rs`
- `contracts/stream/src/test.rs`
- `contracts/token/src/lib.rs` (new `mint_to_batch`, `burn_from` allowance handling, admin nonce)
- `contracts/token/src/storage.rs`
- `contracts/token/src/types.rs`
- `contracts/token/src/events.rs`
- `contracts/token/src/test.rs`

Files out of scope:

- Off-chain scripts and CI configuration

---

## Acceptance Criteria

- [x] Auditor scoped (both contracts) and engaged
- [ ] All findings tracked as GitHub issues with severity labels
- [ ] Audit report published under `audits/` following existing naming convention
- [ ] All remediation issues closed before mainnet go-live
- [ ] `audits/remediation.md` updated with new audit results

---

## Auditor Candidates

Preferred firms (in order):

1. Trail of Bits (continuity from prior engagement)
2. OtterSec
3. Halborn

---

**Status:** Engagement confirmed with Trail of Bits (continuity engagement), covering both
`contracts/stream/` and `contracts/token/`.

---

## Findings Tracking

Every audit finding must be tracked as a GitHub issue before remediation begins:

- Each finding gets its own issue, linked back to the audit report section.
- Each issue carries exactly one severity label: `Critical`, `High`, `Medium`, `Low`, or `Info`.
- Issues are grouped under the SEC-36 tracking issue and reviewed at each remediation sync.
- All `Critical` and `High` remediation issues must be closed before mainnet go-live.
- The final report will be published under `audits/` following the existing naming convention
  (e.g. `audits/2026-<auditor>-report.md`).

## Timeline

| Milestone | Target |
|-----------|--------|
| Scope finalised and RFP sent | 2026-10-01 ✅ |
| Auditor engaged | 2026-10-20 ✅ |
| Audit start | 2026-11-03 |
| Report delivered | 2026-11-28 |
| Findings resolved | 2026-12-05 |
| Mainnet go-live | Blocked until above complete |
