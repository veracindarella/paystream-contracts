# PayStream Roadmap

This roadmap sets expectations for contributors, integrators, and partners.
Timelines are given in quarters and are targets, not commitments. It is
updated on every release — see [CHANGELOG.md](../CHANGELOG.md) for what has
shipped.

| Milestone | Theme | Target |
|---|---|---|
| v0.1.x | Testnet (current) | Now |
| v0.2 | Mainnet readiness | Q4 2026 |
| v0.3 | Advanced features | Q1 2027 |
| v1.0 | Mainnet launch | Q2 2027 |

---

## v0.1.x — Testnet (current)

Core per-second salary streaming deployed on Stellar testnet.

- Stream lifecycle: create, withdraw, top up, pause/resume, cancel, settle
- Batch create/cancel, `withdraw_all`, employer/employee indexes
- Admin controls: two-step admin transfer, contract pause, minimum deposit, upgrade/migrate

## v0.2 — Mainnet readiness

Security and correctness work required before real funds are streamed.

- **SC-01** — Guard `initialize` against re-initialization (merged in #126)
- **SEC-01** — Complete an external security audit and resolve all findings
- **SEC-02** — Address audit follow-ups and publish the report under `audits/`
- Stable error codes and event schema documented in [api-reference.md](api-reference.md)

## v0.3 — Advanced features

Features that make continuous payroll practical without manual intervention.

- **Vesting** — cliff and linear vesting schedules on streams
- **Auto top-up** — PROD-13 (#122): employer-authorized recurring top-ups
- Improved query endpoints for indexers and dashboards

## v1.0 — Mainnet launch

- All v0.2 and v0.3 issues closed
- Mainnet deployment with verified WASM hash and published contract IDs
- Integrator guide and SDK examples
- Upgrade and incident-response runbooks finalised

---

## Contributing to the roadmap

Pick an issue from the milestone you care about, or propose a new item in
[GitHub Discussions → Ideas](https://github.com/veracindarella/paystream-contracts/discussions).
See [CONTRIBUTING.md](../CONTRIBUTING.md) to get started.
