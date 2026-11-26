# Mutation testing baseline (TEST-62)

This document records the mutation-testing baseline for the stream
contract's core math, as required by TEST-62.

## Running it

```sh
cargo install cargo-mutants
make mutation-test
```

CI runs the same scan on every push to `main`/`develop` and on every PR
targeting `main` via `.github/workflows/mutation.yml`. The scope is defined
in `mutants.toml` at the repo root and is limited to
`contracts/stream/src/lib.rs`, `utils.rs` and `validate.rs` — the files
containing the claimable-amount, fee and balance arithmetic.

## Baseline

As of this change, the existing `stream` test suite kills mutants in the
scoped files that change a arithmetic operator, comparison, or boundary
constant in:

- `protocol_fee_amount` (fee basis-point math)
- `stream_claimable_amount` (time-based accrual math)
- `update_rate` / balance and rate validation in `validate.rs`

Any mutant in these functions that **survives** the existing test suite is
treated as a CI failure (the `mutation-test` job in
`.github/workflows/mutation.yml` does not ignore a non-zero `cargo mutants`
exit code). This enforces the acceptance criterion that "surviving mutants
in core math fail CI."

Mutants outside the scoped files (event emission, storage key plumbing,
admin bookkeeping) are intentionally excluded from the gate: they carry no
direct financial risk and keeping them in scope would make CI runtime and
noise unmanageable for a `Large`-effort baseline pass. If a future PR wants
to tighten coverage further, extend `examine_globs` in `mutants.toml` and
record the new baseline here along with any mutants that are deliberately
left unkilled (e.g. equivalent mutations, or log-only code), per the policy
already documented in `CONTRIBUTING.md`.

## Updating the baseline

When intentionally adding new core math, re-run `make mutation-test`
locally, confirm all mutants in the new code are killed, and update this
file's "Baseline" section to describe the new functions covered.
