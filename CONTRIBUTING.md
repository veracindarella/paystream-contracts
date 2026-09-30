# Contributing to PayStream

Thank you for contributing to PayStream — a Soroban smart contract system for real-time salary streaming on Stellar. This guide covers everything you need to go from zero to a merged PR.

---

## Table of Contents

- [Development Setup](#development-setup)
  - [macOS](#macos)
  - [Linux](#linux)
  - [Windows](#windows)
  - [Docker (any OS)](#docker-any-os)
- [Updating Stellar CLI Version](#updating-stellar-cli-version)
- [Project Structure](#project-structure)
- [Coding Standards](#coding-standards)
- [Commit Conventions](#commit-conventions)
- [Bounty Program & Finding Issues](#bounty-program--finding-issues)
  - [Finding your first issue](#finding-your-first-issue)
- [Questions & Discussions](#questions--discussions)
- [Pull Request Process](#pull-request-process)
- [Changelog](#changelog)
- [Testing Requirements](#testing-requirements)
- [Writing Fuzz Targets](#writing-fuzz-targets)
- [Code Review Expectations](#code-review-expectations)
- [Release Process](#release-process)
- [Glossary](#glossary)
- [License](#license)

---

## Development Setup

### Prerequisites (all platforms)

| Tool | Version | Purpose |
|---|---|---|
| Rust | stable (see `rust-toolchain.toml`) | Contract compilation |
| `wasm32-unknown-unknown` target | bundled via toolchain file | WASM output |
| Stellar CLI | 27.0.0 | Build, deploy, invoke |
| Docker + Compose | any recent | Optional — zero-install alternative |

The `rust-toolchain.toml` at the repo root pins the exact Rust channel and installs `rustfmt` and `clippy` automatically when you run any `cargo` command.

> **Quick setup**: Run `make setup` to automatically install Rust, add the wasm32 target, and install Stellar CLI at the pinned version. This works on macOS and Linux (including WSL on Windows).

---

### macOS

```bash
# 1. Clone the repository
git clone https://github.com/veracindarella/paystream-contracts.git
cd paystream-contracts

# 2. Run automated setup (installs Rust, wasm32 target, and Stellar CLI)
make setup

# 3. Verify
make test
```

> **Manual setup**: If you prefer to install dependencies manually, follow the steps below. Homebrew users can also install Rust via `brew install rust`, but `rustup` is preferred because it respects `rust-toolchain.toml`.

```bash
# Manual: Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Manual: Install Stellar CLI
cargo install --locked stellar-cli --version 27.0.0

# rust-toolchain.toml handles the target and components automatically
```

---

### Linux

```bash
# 1. Install build dependencies (Debian/Ubuntu)
sudo apt-get update && sudo apt-get install -y build-essential pkg-config libssl-dev

# 2. Clone the repository
git clone https://github.com/veracindarella/paystream-contracts.git
cd paystream-contracts

# 3. Run automated setup (installs Rust, wasm32 target, and Stellar CLI)
make setup

# 4. Verify
make test
```

For Fedora/RHEL replace step 1 with:
```bash
sudo dnf install gcc openssl-devel
```

> **Manual setup**: If you prefer to install dependencies manually:

```bash
# Manual: Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Manual: Install Stellar CLI
cargo install --locked stellar-cli --version 27.0.0
```

---

### Windows

Native Windows development is supported via WSL 2. Running inside WSL gives you a full Linux environment and avoids shell-script compatibility issues.

```powershell
# 1. Enable WSL 2 (run in PowerShell as Administrator)
wsl --install
# Restart when prompted, then open a WSL terminal

# Inside WSL:
# 2. Install build dependencies
sudo apt-get update && sudo apt-get install -y build-essential pkg-config libssl-dev

# 3. Clone the repository
git clone https://github.com/veracindarella/paystream-contracts.git
cd paystream-contracts

# 4. Run automated setup (installs Rust, wasm32 target, and Stellar CLI)
make setup

# 5. Verify
make test
```

> If you prefer not to use WSL, the [Docker path](#docker-any-os) below works natively on Windows with Docker Desktop.

> **Manual setup**: If you prefer to install dependencies manually inside WSL:

```bash
# Manual: Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Manual: Install Stellar CLI
cargo install --locked stellar-cli --version 27.0.0
```

---

### Docker (any OS)

No local Rust or Stellar CLI installation required. Docker Compose mounts the repo and caches the Cargo registry between runs.

```bash
# Run the full test suite
docker compose run --rm test

# Build WASM artifacts only
docker compose run --rm build stellar contract build
```

The `cargo-cache` volume persists across runs so subsequent builds are fast. WASM output lands in `target/wasm32-unknown-unknown/release/` on your host machine via the bind mount.

---

### Repository Setup

After cloning the repository and setting up your development environment, you may need to configure branch protection rules for the repository. This is typically done by repository maintainers.

```bash
# Configure branch protection for main and develop branches
# Requires: gh CLI authenticated with repo admin rights
./scripts/setup-branch-protection.sh [OWNER/REPO]
```

The script applies protection rules to both `main` and `develop` branches, including:
- Required status checks (CI build must pass)
- Pull request reviews (1 approving review required)
- No force pushes or deletions
- Idempotent: can be run multiple times safely

---

## Updating Stellar CLI Version

The Stellar CLI version is pinned in multiple places to ensure reproducible builds:

- **CI workflows**: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/integration.yml`, and `.github/workflows/benchmark.yml` (env variable `STELLAR_CLI_VERSION`)
- **Development setup**: `CONTRIBUTING.md` (Prerequisites table and manual setup instructions)
- **Makefile**: `make setup` target

### How to update

When a new Stellar CLI version is released:

1. Check the [stellar-cli releases](https://github.com/stellar/stellar-cli/releases) for the latest version
2. Update the version in all locations:
   - Update `STELLAR_CLI_VERSION` in all CI workflow files:
     - `.github/workflows/ci.yml`
     - `.github/workflows/release.yml`
     - `.github/workflows/integration.yml`
     - `.github/workflows/benchmark.yml`
   - Update the version in the Prerequisites table in `CONTRIBUTING.md`
   - Update the version in all manual setup commands in `CONTRIBUTING.md`
   - Update the version in the `make setup` target in `Makefile`
3. Run CI to verify the new version works correctly
4. Create a PR with the changes (type: `chore`)

### Automation

The project uses Dependabot for dependency updates (see `.github/dependabot.yml`). Dependabot tracks Cargo crate dependencies and GitHub Actions, but does not support tracking Cargo binary installations like `stellar-cli`. Therefore, Stellar CLI version updates must be done manually following the process above.

---

## Project Structure

```
.
├── contracts
│   ├── stream/         # Core salary streaming and escrow contract
│   │   └── src/
│   │       ├── lib.rs      # Public entrypoints (create, withdraw, cancel, …)
│   │       ├── storage.rs  # Persistence helpers and claimable calculation
│   │       ├── events.rs   # On-chain event publishing
│   │       ├── types.rs    # Domain models, storage keys, error constants
│   │       └── test.rs     # Contract tests
│   └── token/          # SEP-41 fungible token used in tests
├── scripts/            # Build, deploy, and init shell scripts
├── Cargo.toml          # Workspace manifest
├── Makefile            # Common dev tasks
├── rust-toolchain.toml # Pinned Rust toolchain
└── docker-compose.yml  # Zero-install build/test environment
```

---

## Coding Standards

### General

- All code must compile without warnings: `cargo clippy --all-targets -- -D warnings`
- Formatting is enforced: `cargo fmt --check` must pass
- `#![no_std]` — the standard library is not available in contract code; use `soroban_sdk` primitives
- No floating-point arithmetic — all token amounts use `i128`
- All arithmetic on amounts must use checked or saturating operations (`checked_add`, `checked_mul`, `saturating_sub`). Silent wrapping is a bug

### Contract-specific rules

- Every state-changing function must emit an event via `events.rs` — this includes `initialize`, which must emit a `contract_initialized` event so off-chain indexers can determine when and by whom the contract was initialised
- Authorization must be checked with `address.require_auth()` before any state mutation
- Reentrancy guards (`stream.locked`) must be set before any cross-contract call and released after
- New storage keys belong in the `DataKey` enum in `types.rs`
- New error codes follow the `E###` convention defined in `types.rs` and must be documented in the error table there

### Error codes

| Code | Constant | Meaning |
|---|---|---|
| E001 | `ERR_ZERO_RATE` | `rate_per_second` must be > 0 |
| E002 | `ERR_ZERO_DEPOSIT` | `deposit` must be > 0 |
| E003 | `ERR_REENTRANT` | Reentrant withdraw detected |
| E004 | `ERR_OVERFLOW` | Arithmetic overflow in claimable calculation |
| E005 | `ERR_STREAM_CANCELLED` | Cannot top up a cancelled stream |
| E006 | `ERR_STREAM_EXHAUSTED` | Cannot top up an exhausted stream |

When adding a new error, assign the next available code, add the constant to `types.rs`, and document it in the table above.

> **Keep docs in sync:** the [Error Codes table in `docs/api-reference.md`](docs/api-reference.md#error-codes) must match `contracts/stream/src/types.rs`. Whenever you add, change, or start raising an error from a new function, update its Code, Constant, Meaning, Triggered By, and Recommended Fix columns in the same PR.

### Documentation

- Public functions must have a doc comment (`///`) explaining parameters, return value, and any panic conditions
- Non-obvious logic must have inline comments — especially around reentrancy, overflow handling, and status transitions

---

## Commit Conventions

Follow [Conventional Commits](https://www.conventionalcommits.org/). Every commit message must have the form:

```
<type>(<optional scope>): <short description>

[optional body]

[optional footer(s)]
```

**Allowed types:**

| Type | When to use |
|---|---|
| `feat` | New contract function or observable behaviour |
| `fix` | Bug fix |
| `test` | Adding or updating tests only |
| `docs` | Documentation changes only |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `chore` | Build scripts, CI config, dependency bumps |
| `perf` | Performance improvement |

**Examples:**

```
feat(stream): add top-up function to stream contract

fix(storage): cap claimable at remaining deposit to prevent over-withdrawal

test(stream): add stop_time boundary test for claimable calculation

docs: expand CONTRIBUTING with OS-specific setup instructions

chore: bump soroban-sdk to 22.0.0
```

**Rules:**
- Subject line ≤ 72 characters, imperative mood ("add", not "added" or "adds")
- No period at the end of the subject line
- Reference issues in the footer: `Closes #15` or `Fixes #3`
- Breaking changes must include `BREAKING CHANGE:` in the footer

---

## Bounty Program & Finding Issues

Looking for tasks to contribute to?

- **Bounty Program**: PayStream hosts open bounties with defined criteria for merged PRs. See [docs/bounties.md](docs/bounties.md) for active bounty categories, rules, and claiming instructions.
- **Good First Issues**: If you are new to the codebase, check out issues tagged with [`good-first-issue`](https://github.com/veracindarella/paystream-contracts/issues?q=is%3Aissue+is%3Aopen+label%3Agood-first-issue).
- **Roadmap**: See [docs/roadmap.md](docs/roadmap.md) for upcoming milestones and the issues each one depends on.

### Finding your first issue

1. Browse issues labelled [`good-first-issue`](https://github.com/veracindarella/paystream-contracts/issues?q=is%3Aissue+is%3Aopen+label%3Agood-first-issue). These are self-contained, have clear acceptance criteria, and need no deep Soroban expertise.
2. Comment on the issue to say you're picking it up, so work isn't duplicated.
3. Stuck? Ping a maintainer in [GitHub Discussions](https://github.com/veracindarella/paystream-contracts/discussions) — every good first issue has a mentor available.
4. Keep the PR small and reference the issue with `Closes #<number>`.

---

## Questions & Discussions

- **Questions, ideas, and show-and-tell** → [GitHub Discussions](https://github.com/veracindarella/paystream-contracts/discussions) (categories: Q&A, Ideas, Show and Tell, Announcements). Start with the pinned "Welcome & FAQ" thread.
- **Bugs and concrete feature requests** → [GitHub Issues](https://github.com/veracindarella/paystream-contracts/issues).
- **Security vulnerabilities** → follow [SECURITY.md](SECURITY.md); never open a public issue.

---

## Pull Request Process

### Before opening a PR

Run the full local check suite and make sure everything passes:

```bash
make fmt-check   # formatting
make lint        # clippy -D warnings
make test        # cargo test
```

### Pre-commit hooks (recommended)

To avoid CI failures due to formatting or linting issues, install the pre-commit hook that automatically runs `cargo fmt --check` and `cargo clippy` before each commit:

```bash
make setup-hooks
```

The hook will prevent commits that fail formatting or linting checks. If you need to bypass the hook temporarily (e.g., for a work-in-progress commit), use:

```bash
git commit --no-verify
```

### Branch naming

Branch from `main` using the pattern `<type>/<short-description>`:

```
feat/batch-stream-creation
fix/claimable-overflow
docs/contributing-setup
```

### PR checklist

When you open a PR, the description template will include this checklist. All items must be checked before requesting review:

- [ ] `make test` passes locally
- [ ] `make lint` passes (no clippy warnings)
- [ ] `make fmt-check` passes (no formatting diff)
- [ ] New contract functions have tests in `test.rs`
- [ ] Events are emitted for all state-changing operations
- [ ] New error codes are added to `types.rs` and documented
- [ ] Doc comments added or updated for changed public functions
- [ ] README updated if public behaviour or the function table changed
- [ ] No new `unwrap()` calls without a comment explaining why it is safe
- [ ] `CHANGELOG.md` updated if the PR changes observable behavior (see [Changelog](#changelog))

### PR size

Keep PRs focused. A PR that touches a single concern is easier to review and faster to merge. If a feature requires both a contract change and a documentation update, they can live in the same PR — but unrelated changes should be separate.

### Merging

PRs are merged by a maintainer after at least one approving review and a passing CI run. Maintainers may squash commits to keep the history clean; if you want your individual commits preserved, say so in the PR description.

---

## Changelog

[CHANGELOG.md](CHANGELOG.md) follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

### Who and when

- **Every PR that changes observable behavior must include a CHANGELOG entry** — new or changed public functions, events, error codes, storage layout, fees, or CLI/deployment steps.
- The PR author adds the entry in the same PR. Reviewers should request changes if it is missing.
- Internal-only changes (refactors with no behavior change, tests, CI, typo fixes) do not need an entry.

### Format

Add entries under `## [Unreleased]`, in the matching section (create it if absent):

| Section | Use for |
|---|---|
| `### Added` | New functions, events, error codes, or features |
| `### Changed` | Changes to existing behavior or interfaces |
| `### Fixed` | Bug fixes |
| `### Security` | Vulnerability fixes or hardening |
| `### Removed` | Removed functions or features |

Prefix each entry with the issue ID when there is one:

```markdown
### Fixed

- SEC-04: `initialize` now rejects a second call on an already-initialised token.
```

### Releases

When cutting a release, a maintainer renames `[Unreleased]` to `[x.y.z] - YYYY-MM-DD` (following Semantic Versioning), adds a fresh empty `[Unreleased]` section above it, and tags the release commit.

---

## Testing Requirements

### Where tests live

Each contract has a `test.rs` module gated with `#[cfg(test)]`. Tests use the Soroban SDK test utilities (`Env::default()`, `mock_all_auths()`, ledger time manipulation) — no external network required.

### What must be tested

- Every new public contract function needs at least one happy-path test
- Every new error condition (`assert!`, `panic!`) needs a `#[should_panic(expected = "...")]` test that matches the exact error code (e.g. `"E001"`)
- Edge cases that are explicitly called out in comments (overflow, boundary timestamps, status transitions) must have dedicated tests
- Reentrancy guards must be tested by manually setting `stream.locked = true` via `env.as_contract(...)` before calling `withdraw`

### Test structure conventions

```rust
#[test]
fn test_<what_is_being_tested>() {
    // Arrange
    let (env, client) = setup();
    // ... set up actors and state

    // Act
    // ... call the function under test

    // Assert
    assert_eq!(...);
}
```

Use the shared `setup()` and `setup_token()` helpers defined at the top of `test.rs` rather than duplicating boilerplate.

### Running tests

```bash
make test          # run all tests
cargo test <name>  # run a single test by name
```

### Coverage

Coverage is measured with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) (compatible with Soroban/`no_std` crates) and reported by the `Coverage` workflow (`.github/workflows/coverage.yml`), which uploads `lcov.info` as a CI artifact and to Codecov.

```bash
cargo install cargo-llvm-cov
make coverage      # print a per-file coverage summary
```

Target baseline: **>90% line coverage** for `contracts/stream` and `contracts/token`. New code should not lower coverage.

### Mutation testing

[`cargo-mutants`](https://mutants.rs) checks that tests actually assert on behaviour, not just execute code.

```bash
cargo install cargo-mutants
make mutation-test # runs cargo mutants on the stream contract
```

Each surviving mutant must either be killed by a new test or documented as acceptable (e.g. equivalent mutations, or event/log-only code) in the PR that introduces it.

### Snapshot files

Test snapshots in `test_snapshots/` are generated automatically by the SDK. Commit them alongside the test that produces them. If a snapshot changes unexpectedly, investigate before updating it — an unexpected snapshot diff often indicates a behaviour regression.

---

## Writing Fuzz Targets

Fuzz targets live in the `paystream-stream-fuzz` package at `contracts/stream/fuzz/` (listed in the root `Cargo.toml` workspace `members`). They use
[proptest](https://docs.rs/proptest) to generate randomized inputs and assert invariants.

### Structure

```
contracts/stream/fuzz/
├── Cargo.toml              # package manifest; one [[bin]] entry per target
└── src/
    ├── fuzz_claimable.rs
    ├── fuzz_create_stream.rs
    └── fuzz_withdraw.rs
```

Each target is a binary with an empty `main()` and a `proptest!` block of `#[test]` properties. The package depends on
`paystream-stream` with the `testutils` feature so targets can call internal storage helpers and use `Env::default()`.

### Adding a new target

1. Create `contracts/stream/fuzz/src/fuzz_<name>.rs`:

   ```rust
   // SPDX-License-Identifier: Apache-2.0

   use proptest::prelude::*;

   proptest! {
       #![proptest_config(ProptestConfig::with_cases(1_000_000))]

       #[test]
       fn prop_example(a in 0i128..1_000_000, b in 0i128..1_000_000) {
           // Replace with a real contract invariant
           prop_assert!(a.checked_add(b).is_some());
       }
   }

   fn main() {}
   ```

2. Register it in `contracts/stream/fuzz/Cargo.toml`:

   ```toml
   [[bin]]
   name = "fuzz_<name>"
   path = "src/fuzz_<name>.rs"
   ```

### Running locally

```bash
cargo test --package paystream-stream-fuzz --bin fuzz_<name>
# Fewer cases for a quick check:
PROPTEST_CASES=1000 cargo test --package paystream-stream-fuzz --bin fuzz_<name>
```

### Updating CI

Add a step to `.github/workflows/fuzz.yml`:

```yaml
      - name: Run proptest fuzz (1 000 000 iterations) — <name>
        run: cargo test --package paystream-stream-fuzz --bin fuzz_<name>
```

### References

- [proptest book](https://proptest-rs.github.io/proptest/)
- [cargo-fuzz / Rust Fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz.html)

---

## Code Review Expectations

### For authors

- Respond to review comments within two business days
- If you disagree with a suggestion, explain your reasoning — don't just dismiss it
- Mark conversations as resolved only after addressing them (or reaching agreement to leave them as-is)
- Keep the PR up to date with `main`; rebase rather than merge to keep history linear

### For reviewers

- Review within two business days of being assigned
- Distinguish between blocking issues and non-blocking suggestions — prefix non-blocking comments with `nit:` or `suggestion:`
- Focus on correctness, security, and maintainability; style issues are handled by `clippy` and `rustfmt` automatically
- When reviewing contract code, pay particular attention to:
  - Authorization checks (`require_auth` present and in the right place)
  - Arithmetic overflow paths
  - Status transition correctness (`Active → Paused → Active`, `Active → Cancelled`, etc.)
  - Event emission for every state change
  - Reentrancy safety around cross-contract token transfers

### Security-sensitive changes

Any change to `withdraw`, `cancel_stream`, token transfer logic, or the reentrancy guard requires sign-off from a maintainer with contract security experience before merge. Tag such PRs with the `security` label.

---

## Release Process

Releases follow a formal checklist to ensure all requirements are met before deploying to production. Use the [Release Checklist issue template](.github/ISSUE_TEMPLATE/release.md) to track release progress.

### Creating a Release

1. Open a new issue using the "Release Checklist" template
2. Replace `{VERSION}` with the actual version number (e.g., v1.2.3)
3. Complete all items in the pre-release checklist:
   - SC-01 (Reentrancy Guard) verification
   - All tests pass (unit, integration, linting, formatting, deny)
   - Audit status (if applicable)
   - Testnet deployment verification
4. Complete all release steps:
   - Update CHANGELOG.md
   - Version bump in Cargo.toml (if applicable)
   - Create and push git tag
   - Create GitHub release with notes
   - Deploy to mainnet (if applicable)

### Release Checklist Items

The release template includes checks for:
- **SC-01 Verification**: Reentrancy guard analysis and documentation
- **Testing**: Unit tests, integration tests, linting, formatting, deny checks
- **Audit Status**: External audit completion and findings addressed
- **Testnet Deployment**: Contract deployed and verified on testnet
- **Documentation**: CHANGELOG update, version bump, API docs
- **Git Operations**: Tag creation and push
- **GitHub Release**: Release creation with notes and deployment hashes
- **Post-Release**: Mainnet deployment, announcements

See the [Release Checklist template](.github/ISSUE_TEMPLATE/release.md) for the complete checklist.

---

## Glossary

See [docs/glossary.md](docs/glossary.md) for the full glossary of Soroban and PayStream terms.

| Term | Meaning |
|---|---|
| Stream | A salary stream from employer to employee |
| Deposit | Funds locked by employer at stream creation |
| Rate | Tokens released per second to the employee (`rate_per_second`) |
| Claimable | Tokens earned but not yet withdrawn |
| Stop time | Optional hard end timestamp; `0` means indefinite |
| Exhausted | Stream status when the full deposit has been withdrawn |
| SEP-41 | Stellar token interface standard (equivalent to ERC-20) |
| SAC | Stellar Asset Contract — a SEP-41 wrapper for native Stellar assets |

---

## License

Apache 2.0 — contributions are licensed under the same terms as the project.
