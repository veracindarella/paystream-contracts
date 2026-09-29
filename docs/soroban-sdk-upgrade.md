# Soroban SDK Upgrade Guide

This guide covers bumping the `soroban-sdk` dependency in `Cargo.toml`. For upgrading a *deployed* contract's WASM, see [upgrade-guide.md](upgrade-guide.md).

SDK upgrades can change host behaviour, auth semantics, storage APIs and arithmetic helpers, so every bump must re-verify the contracts' security assumptions — not just recompile them.

## Steps

### 1. Check the SDK release notes

Read the [soroban-sdk releases](https://github.com/stellar/rs-soroban-sdk/releases) and the matching [Stellar protocol / CAP notes](https://developers.stellar.org/docs/networks/software-versions) for every version between the current and target versions. Note anything touching:

- cross-contract calls, auth (`require_auth`) or the reentrancy model
- storage APIs and TTL (`extend_ttl`) semantics
- integer / `i128` arithmetic helpers and overflow behaviour
- the `testutils` feature, events, or snapshot format
- minimum supported Rust version

### 2. Update `Cargo.toml`

The SDK version is pinned once in the workspace root and inherited by both contracts:

```toml
# Cargo.toml
[workspace.dependencies]
soroban-sdk = { version = "<NEW_VERSION>" }
```

Then refresh the lockfile:

```bash
cargo update -p soroban-sdk
```

Update the SDK version in the README "Technology Stack" table.

### 3. Run tests

```bash
make fmt-check
make lint
make test
make build
```

Fix any compile errors or API changes. If test snapshots (`test_snapshots/`) change, review every diff — an unexpected change in events, auth or storage is a red flag, not noise.

### 4. Re-verify the reentrancy analysis

Follow the [re-verification checklist](security/reentrancy-analysis.md#6-re-verification-checklist) and update `docs/security/reentrancy-analysis.md` with the new SDK version and review date.

### 5. Re-run benchmarks

Re-run the benchmarks described in [benchmarks/gas-optimization-report.md](../benchmarks/gas-optimization-report.md#methodology) and compare against the recorded results. Investigate any significant CPU/memory regression and update the report.

### 6. Update `rust-toolchain.toml` if needed

If the new SDK raises its minimum supported Rust version or changes the WASM target, update `rust-toolchain.toml` (currently `channel = "stable"`, target `wasm32-unknown-unknown`) and the Dockerfile accordingly.

## Historical SDK versions

| SDK version | Adopted in | Migration notes |
|---|---|---|
| 22.0.0 | Initial release (`024a89d`) | Baseline. Contracts written against the v22 API; no migration required. |

Add a row for every future bump, summarising code changes and any re-verification findings.

## Re-verification checklist

- [ ] Release notes reviewed for every intermediate version
- [ ] `soroban-sdk` bumped in root `Cargo.toml` and `Cargo.lock` updated
- [ ] `make fmt-check`, `make lint`, `make test`, `make build` pass
- [ ] Test snapshot diffs reviewed and explained
- [ ] Auth: every state-changing entrypoint still calls `require_auth` on the correct address
- [ ] Reentrancy analysis re-verified and `reentrancy-analysis.md` updated
- [ ] Arithmetic: `claimable` / refund calculations still use checked or saturating operations; overflow tests pass
- [ ] Storage and TTL extension behaviour unchanged (or migration planned via `migrate`)
- [ ] Benchmarks re-run; `gas-optimization-report.md` updated
- [ ] `rust-toolchain.toml` / Dockerfile updated if required
- [ ] README SDK version and the history table above updated
- [ ] CHANGELOG entry added
