# PayStream Contracts

[![CI](https://github.com/veracindarella/paystream-contracts/actions/workflows/ci.yml/badge.svg)](https://github.com/veracindarella/paystream-contracts/actions/workflows/ci.yml)
[![Coverage](https://codecov.io/gh/veracindarella/paystream-contracts/branch/main/graph/badge.svg)](https://codecov.io/gh/veracindarella/paystream-contracts)
[![cargo-deny](https://github.com/veracindarella/paystream-contracts/actions/workflows/deny.yml/badge.svg)](https://github.com/veracindarella/paystream-contracts/actions/workflows/deny.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.84.0-orange.svg)](Cargo.toml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Soroban smart contracts for **PayStream** — decentralized payroll and salary streaming on the Stellar blockchain.

PayStream lets employers stream salaries to employees in real-time, per-second. Instead of waiting for a monthly paycheck, employees earn and can withdraw their salary continuously as they work — fully on-chain, trustless, and transparent.

> **New here?** Create your first testnet stream in 5 minutes with the **[Quickstart](docs/quickstart.md)**.

---

## Why PayStream?

- **Real-time pay** — employees access earned wages any time, not just payday
- **Trustless escrow** — funds locked on-chain; employer cannot claw back earned salary
- **Transparent** — every stream, withdrawal, and cancellation is an immutable on-chain event
- **Stellar-native** — built on Stellar's fast, low-fee infrastructure with Soroban smart contracts
- **Flexible** — pause, resume, top-up, or cancel streams; optional hard stop time
- **Multi-token** — each stream can use any [SEP-41](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md) compliant token; employer and employee can run concurrent streams in different assets

---

## Architecture

Solid arrows are contract calls made by each actor; dashed arrows are token transfers.

```mermaid
flowchart LR
    Admin([Admin])
    Employer([Employer])
    Employee([Employee])
    SC[StreamContract]
    TC[TokenContract<br/>SEP-41 / SAC]

    Admin -->|"initialize, propose_admin / accept_admin,<br/>pause_contract / unpause_contract,<br/>set_min_deposit, upgrade, migrate"| SC
    Employer -->|"create_stream, create_streams_batch, top_up,<br/>pause_stream, resume_stream, update_rate,<br/>cancel_stream, cancel_streams_batch"| SC
    Employee -->|"withdraw, withdraw_all"| SC
    SC -->|"transfer"| TC

    Employer -.->|"deposit / top-up"| SC
    SC -.->|"earned salary<br/>(withdraw, cancel)"| Employee
    SC -.->|"unearned refund (cancel)"| Employer
```

Anyone can call the read-only functions (`get_stream`, `claimable`, `stream_status`, …) and `settle_stream`.

### Stream Lifecycle

```mermaid
stateDiagram-v2
    [*] --> Active: create_stream
    Active --> Paused: pause_stream
    Paused --> Active: resume_stream
    Active --> Cancelled: cancel_stream
    Paused --> Cancelled: cancel_stream
    Active --> Exhausted: withdraw / settle_stream (deposit fully paid or stop_time passed)
    Cancelled --> [*]
    Exhausted --> [*]
```

See the [glossary](docs/glossary.md) for terms and the [client guide](docs/client-guide.md) for JavaScript integration examples.

---

## Project Structure

```
.
├── contracts
│   ├── stream              # Core salary streaming and escrow contract
│   │   ├── src
│   │   │   ├── lib.rs      # Stream entrypoints
│   │   │   ├── storage.rs  # Persistence and claimable calculation
│   │   │   ├── events.rs   # On-chain event publishing
│   │   │   ├── types.rs    # Domain models and storage keys
│   │   │   └── test.rs     # Contract tests
│   │   └── Cargo.toml
│   └── token               # Fungible payment token contract
│       ├── src
│       │   ├── lib.rs
│       │   ├── storage.rs
│       │   ├── types.rs
│       │   └── test.rs
│       └── Cargo.toml
├── scripts
│   ├── build.sh
│   ├── deploy-local.sh
│   ├── deploy-testnet.sh
│   └── init-testnet.sh
├── Cargo.toml
├── Makefile
├── CONTRIBUTING.md
├── SECURITY.md
└── README.md
```

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (latest stable)
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)

```bash
git clone https://github.com/veracindarella/paystream-contracts.git
cd paystream-contracts
rustup target add wasm32-unknown-unknown
```

### Build

```bash
make build
# or: stellar contract build
```

### Test

```bash
make test
# or: cargo test
```

### Format & Lint

```bash
make fmt-check
make lint
```

---

### Docker (no local Rust/Stellar CLI required)

Build and test entirely inside Docker — no local Rust or Stellar CLI installation needed.

**Run tests:**
```bash
docker compose run --rm test
```

**Build contracts only:**
```bash
docker compose run --rm build stellar contract build
```

The `cargo-cache` volume persists the Cargo registry between runs so subsequent builds are fast.

---

## Stream Contract Reference

> Full parameter, return value, error, and example documentation: **[docs/api-reference.md](docs/api-reference.md)**

### Functions

| Function | Caller | Description |
|---|---|---|
| `initialize(admin)` | Admin | Set contract admin |
| `create_stream(employer, employee, token, deposit, rate_per_second, stop_time)` | Employer | Create stream, lock deposit |
| `create_streams_batch(employer, params)` | Employer | Create multiple streams atomically; all succeed or all revert |
| `withdraw(employee, stream_id)` | Employee | Withdraw all claimable earnings |
| `withdraw_all(employee)` | Employee | Withdraw from all streams in one transaction |
| `top_up(employer, stream_id, amount)` | Employer | Add more funds to active stream |
| `pause_stream(employer, stream_id)` | Employer | Pause accrual |
| `resume_stream(employer, stream_id)` | Employer | Resume accrual |
| `cancel_stream(employer, stream_id)` | Employer | Pay employee earned share, refund remainder |
| `cancel_streams_batch(employer, stream_ids)` | Employer | Cancel multiple streams atomically; all succeed or all revert |
| `get_stream(stream_id)` | Anyone | Read stream state |
| `stream_status(stream_id)` | Anyone | Query stream status only (lightweight) |
| `claimable(stream_id)` | Anyone | Query withdrawable amount right now |
| `stream_count()` | Anyone | Total streams created |
| `stream_count_by_employer(employer)` | Anyone | Number of streams owned by an employer |
| `stream_count_by_employee(employee)` | Anyone | Number of streams paying an employee |

### Batch vs Individual Stream Creation — Fee Comparison

| Approach | Transactions | Approx. fee |
|---|---|---|
| N individual `create_stream` calls | N | N × base fee |
| One `create_streams_batch` call | 1 | 1 × base fee + per-stream resource overhead |

`create_streams_batch` is cheaper for N ≥ 2 because Stellar charges one base fee per transaction. Per-stream resource overhead grows linearly but is far smaller than the per-transaction base fee saved.

### Stream Status Lifecycle

```
Active → Paused → Active
Active → Cancelled
Active → Exhausted  (deposit fully streamed, or stop_time passed — via withdraw or settle_stream)
```

### Claimable Calculation

```
claimable = min(
    (now - last_withdraw_time) * rate_per_second,
    deposit - withdrawn
)
```

Time is capped at `stop_time` if set. Paused time is excluded. See the [worked examples](docs/api-reference.md#worked-examples).

## Documentation

- [API Reference](docs/api-reference.md)
- [FAQ](docs/faq.md) — common integrator questions
- [Operations Runbook](docs/operations-runbook.md) — admin procedures (nonce, pause, min deposit, admin transfer, upgrade)

---

## Deployed Contracts

Current testnet deployments (source of truth: [docs/testnet.md](docs/testnet.md)).

| Contract | Network | Address | Explorer Link |
|---|---|---|---|
| PayStream Token | Stellar Testnet | `CDZQHVQHQMHIGJGSQIJVXGPGNZJQDQV4BBKMG7MSIDTJTTBBQYAEUPB` | [StellarExpert](https://stellar.expert/explorer/testnet/contract/CDZQHVQHQMHIGJGSQIJVXGPGNZJQDQV4BBKMG7MSIDTJTTBBQYAEUPB) |
| PayStream Stream | Stellar Testnet | `CBXKDJUQYQDQKSQT6AKZRQWXDXTQHVXHBQQJQ3WKDXJHPNMRGYAQMRS` | [StellarExpert](https://stellar.expert/explorer/testnet/contract/CBXKDJUQYQDQKSQT6AKZRQWXDXTQHVXHBQQJQ3WKDXJHPNMRGYAQMRS) |

> Contract IDs change after every redeployment. When updating [docs/testnet.md](docs/testnet.md), update this table in the same PR.

---

## Deployment

### Testnet

```bash
./scripts/build.sh
./scripts/deploy-testnet.sh

export STELLAR_ADMIN_ADDRESS=<YOUR_PUBLIC_KEY>
export TOKEN_CONTRACT_ID=<FROM_DEPLOY>
export STREAM_CONTRACT_ID=<FROM_DEPLOY>
./scripts/init-testnet.sh
```

### Local

```bash
make deploy-local
```

---

### Verifying Release Provenance

Each GitHub Release includes a [SLSA](https://slsa.dev) provenance file
(`paystream-contracts.intoto.jsonl`) generated by
[`slsa-github-generator`](https://github.com/slsa-framework/slsa-github-generator).
It proves the released WASM files were built by this repository's CI from the tagged commit.

```bash
# Install the verifier
go install github.com/slsa-framework/slsa-verifier/v2/cli/slsa-verifier@latest

# Download the WASM and provenance from the release, then verify
TAG=v1.0.0
gh release download "$TAG" -R veracindarella/paystream-contracts
slsa-verifier verify-artifact paystream_stream.wasm paystream_token.wasm \
  --provenance-path paystream-contracts.intoto.jsonl \
  --source-uri github.com/veracindarella/paystream-contracts \
  --source-tag "$TAG"
```

## Technology Stack

| Layer | Technology |
|---|---|
| Blockchain | Stellar (Soroban) |
| Language | Rust |
| SDK | Soroban SDK v22.0.0 |
| CI/CD | GitHub Actions |

---

## Contributing & Bounties

We welcome contributions from the community.

- **Developer Setup & Guidelines**: Review [CONTRIBUTING.md](CONTRIBUTING.md) for environment setup, coding standards, and PR workflows.
- **Bounty Program**: PayStream maintains an active bounty track for open-source contributors across smart contracts, testing, security, and tooling. See [docs/bounties.md](docs/bounties.md) for open bounties, criteria, and entry-level tasks tagged with `good-first-issue`.
- **First-Time Contributors**: Browse open beginner-friendly tasks in the [Good First Issues Tracker](https://github.com/veracindarella/paystream-contracts/issues?q=is%3Aissue+is%3Aopen+label%3Agood-first-issue).

## Security

See [SECURITY.md](SECURITY.md) for the full security policy and vulnerability reporting.
To rotate the admin keypair, follow the [key rotation runbook](docs/security/key-rotation.md).
Report vulnerabilities to `security@paystream.example` — not via public issues.

## License

[Apache 2.0](LICENSE)

---

Built with ❤️ on Stellar
