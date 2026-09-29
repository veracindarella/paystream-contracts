# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Email: `security@paystream.example`

You will receive acknowledgement within 48 hours and a resolution timeline within 7 days.

## Disclosure Timeline

| Stage | Target |
|-------|--------|
| Acknowledge receipt of report | Within 48 hours |
| Assess and assign severity | Within 5 days |
| Fix deployed — Critical | Within 14 days of assessment |
| Fix deployed — High | Within 30 days of assessment |
| Fix deployed — Medium / Low | Next scheduled release |
| Public disclosure | Coordinated with the reporter after the fix is deployed |

Reporters are credited (unless they prefer to remain anonymous) in the Hall of Fame below and in the
release notes at the time of public disclosure. We ask reporters not to disclose the issue publicly
until the coordinated disclosure date.

## Bug Bounty

A formal security bug bounty program is planned but not yet active. Until it launches:

- **In scope:** contracts in `contracts/` (stream and token) and deployment scripts in `scripts/`
- **Out of scope:** third-party dependencies, the Stellar network itself, social engineering, and
  issues already listed in [audits/remediation.md](audits/remediation.md)
- **Rewards:** determined case-by-case based on severity; see [docs/bounties.md](docs/bounties.md)
  for the general contributor bounty program

## Hall of Fame

We thank the following researchers for responsibly disclosing vulnerabilities:

| Reporter | Finding | Date |
|----------|---------|------|
| _No reports yet — be the first!_ | | |

## Security Audits

| Date | Auditor | Report | Remediation |
|------|---------|--------|-------------|
| 2026-04 | Trail of Bits | [2026-04-trail-of-bits.md](audits/2026-04-trail-of-bits.md) | [remediation.md](audits/remediation.md) |

All high and medium findings from the April 2026 audit have been resolved. One low-severity
finding (LOW-02: re-initialization guard) remains open and must be resolved before mainnet
deployment. See [audits/remediation.md](audits/remediation.md) for the full status breakdown.

---

## Key Rotation

If the admin keypair needs to be rotated (planned or emergency), follow the
step-by-step runbook in
[docs/security/key-rotation.md](docs/security/key-rotation.md).

---

## Security Design Notes

- All state-changing functions require explicit `require_auth()` from the relevant party
- Employer cannot withdraw employee funds; employee cannot access unearned funds
- Claimable amount is always capped at `deposit - withdrawn` — no over-payment possible
- Cancel pays employee their earned share first, then refunds employer the remainder
- Paused time is excluded from accrual — `last_withdraw_time` is reset on resume
- All token amounts use `i128` — no floating-point arithmetic
- Stop time is validated to be in the future at stream creation
