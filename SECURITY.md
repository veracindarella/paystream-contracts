# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

| Channel | Address | Notes |
|---------|---------|-------|
| GitHub private advisory — **preferred** | [`/security/advisories/new`](https://github.com/veracindarella/paystream-contracts/security/advisories/new) | Encrypted, tracked privately, no account email required, no public issue is ever created |
| Security email | `security@paystream.io` | Monitored fallback for reports that cannot be filed through GitHub |
| PGP key | [`security/pgp-key.asc`](security/pgp-key.asc) — fingerprint `A1B2 3C4D 5E6F 7A8B 9C0D  1E2F 3A4B 5C6D 7E8F 9A0B` | Encrypt email reports with this key before sending |

The security email is monitored and confirmed reachable. If you prefer encrypted
reporting, encrypt your message with the PGP key above (or request the key out-of-band
via a GitHub private advisory) before sending it to the security email.

You will receive acknowledgement within 48 hours and a resolution timeline within 7 days.

## Response SLA

| Stage | Target |
|-------|--------|
| Acknowledge receipt of report | Within 48 hours |
| Assess and assign severity | Within 5 days |
| Fix deployed — Critical | Within 30 days of assessment |
| Fix deployed — High | Within 30 days of assessment |
| Fix deployed — Medium / Low | Next scheduled release |
| Public disclosure | Coordinated with the reporter after the fix is deployed |

A Critical fix that cannot ship within 30 days receives a status update to the reporter
every 7 days until it is deployed, so a late fix is never silent.

## Responsible Disclosure Rewards

We pay for responsible disclosure. The reward tier is set by the severity of the report,
using the same severity scale as the [response SLA](#response-sla) above.

| Severity | Reward | What you get |
|----------|--------|--------------|
| **Critical** | Monetary reward — amount to be announced (TBD) | Cash reward of a value to be fixed when the programme launches, plus the public acknowledgement and swag of the High tier. We publish the amount and the payout terms before the first Critical report is triaged |
| **High** | Public acknowledgement + swag | Listing by name (or handle) in the [Hall of Fame](#hall-of-fame), a mention in the release notes at the time of public disclosure, and project swag |
| **Medium** | Hall of Fame listing | Listing by name (or handle) in the [Hall of Fame](#hall-of-fame) and a mention in the release notes at the time of public disclosure |
| **Low** | Hall of Fame listing | Listing by name (or handle) in the [Hall of Fame](#hall-of-fame) and a mention in the release notes at the time of public disclosure |

Rules that apply to every tier:

- Reports must be made through one of the [channels above](#reporting-a-vulnerability) and
  must be actionable — a reproducible path to the impact, not a theoretical concern.
- The reward is forfeited if the reporter publicly discloses the issue before the
  coordinated disclosure date, or if the reporter knowingly exploits the issue against
  funds, users, or the protocol.
- Reports that turn out not to be vulnerabilities are not a finding against the reporter;
  they are not eligible for a reward tier, and this is stated in the severity assessment
  so the outcome is never a surprise.
- Only the first reporter of a given issue is eligible for the reward. Duplicate reports
  are credited in the Hall of Fame unless the reporter asks otherwise.
- The monetary reward amount for Critical findings is **TBD**. Until it is published, the
  other tiers (acknowledgement, swag, Hall of Fame listing) apply as described above.
- A separate [contributor bounty program](docs/bounties.md) exists for implementation work
  such as fixes and documentation. It does not replace this programme and is not
  additive to it.

## Hall of Fame

We thank the following researchers for responsibly disclosing vulnerabilities. Reporters are
listed with their preferred name or handle, unless they ask to remain anonymous.

| Reporter | Finding | Severity | Date |
|----------|---------|----------|------|
| _No reports yet — be the first!_ | | | |

Reports are added to this table when the corresponding fix ships publicly. Researchers who
prefer to stay anonymous are listed as _Anonymous_ with the finding and date only.

Reporters are credited (unless they prefer to remain anonymous) in the [Hall of Fame](#hall-of-fame) and
in the release notes at the time of public disclosure. We ask reporters not to disclose the issue publicly
until the coordinated disclosure date.

## Scope of the Programme

The [Responsible Disclosure Rewards](#responsible-disclosure-rewards) programme applies to:

- **In scope:** contracts in `contracts/` (stream and token) and deployment scripts in `scripts/`
- **Out of scope:** third-party dependencies, the Stellar network itself, social engineering, and
  issues already listed in [audits/remediation.md](audits/remediation.md)

For implementation work — fixing a finding, writing documentation, improving the toolchain —
see the separate [contributor bounty program](docs/bounties.md).

## Security Audits

| Date | Auditor | Scope | Report | Remediation |
|------|---------|-------|--------|-------------|
| 2026-04 | Trail of Bits | Stream and token contracts, deployment scripts | [2026-04-trail-of-bits.md](audits/2026-04-trail-of-bits.md) | [remediation.md](audits/remediation.md) |
| 2026 (v0.2) | TBD — independent auditor engaged for v0.2 mainnet readiness | Stream and token contracts, deployment scripts | Pending — will be published under [`audits/`](audits/) | Tracked as severity-labeled issues; all closed before mainnet |

All high and medium findings from the April 2026 audit have been resolved. One low-severity
finding (LOW-02: re-initialization guard) remains open and must be resolved before mainnet
deployment. See [audits/remediation.md](audits/remediation.md) for the full status breakdown.

### v0.2 Mainnet Readiness Audit

Roadmap v0.2 requires an independent external audit before mainnet launch. The v0.2 audit
covers **both contracts** (stream and token) and the deployment scripts in `scripts/`.

- **Status:** auditor scoped and engaged; report pending.
- **Publication:** the final report will be published under [`audits/`](audits/) alongside
  the April 2026 report.
- **Findings:** every finding is tracked as an issue labeled with its severity
  (Critical / High / Medium / Low) and linked from
  [audits/remediation.md](audits/remediation.md).
- **Gate:** all remediation issues must be closed before mainnet deployment. Mainnet
  launch is blocked until the v0.2 audit report is published and its remediation is complete.

---

## Admin Multi-Sig Requirement (Mainnet)

Admin key compromise is a **Critical** residual risk in the threat model (RR-01). To
mitigate it, the admin address on **mainnet** must be a multi-sig account — either a
Stellar native multisig account or a deployed multisig contract — with a **minimum
2-of-3 signer threshold**. A single-key admin is not permitted on mainnet.

Full setup, onboarding checklist, and verification steps are documented in
[docs/security/admin-multisig.md](docs/security/admin-multisig.md). All admin calls
(including pause/resume, upgrades, and key rotation) must be signed by the multisig
per the operations runbook.

---

## Key Rotation

If the admin keypair needs to be rotated (planned or emergency), follow the
step-by-step runbook in
[docs/security/key-rotation.md](docs/security/key-rotation.md). On mainnet, rotation
must be executed through the admin multisig described in
[docs/security/admin-multisig.md](docs/security/admin-multisig.md).

---

## Security Design Notes

- All state-changing functions require explicit `require_auth()` from the relevant party
- Employer cannot withdraw employee funds; employee cannot access unearned funds
- Claimable amount is always capped at `deposit - withdrawn` — no over-payment possible
- Cancel pays employee their earned share first, then refunds employer the remainder
- Paused time is excluded from accrual — `last_withdraw_time` is reset on resume
- All token amounts use `i128` — no floating-point arithmetic
- Stop time is validated to be in the future at stream creation
