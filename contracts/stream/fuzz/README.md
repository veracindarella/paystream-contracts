# PayStream Stream Contract – Fuzz & Property-Based Tests

All targets live under `contracts/stream/fuzz/src/` and are compiled as
standalone binaries driven by [proptest](https://github.com/proptest-rs/proptest).

Run any target with:

```sh
cargo test --package paystream-stream-fuzz --bin <target-name>
```

---

## Targets

| Binary | Issue | Description |
|--------|-------|-------------|
| `fuzz_claimable` | SEC-08 | `claimable_amount` never panics unexpectedly and is always in `[0, deposit - withdrawn]` |
| `fuzz_withdraw` | SEC-09 | `withdraw` arithmetic is bounded and the Exhausted transition is sound |
| `fuzz_create_stream` | SEC-08 | `validate_create_stream` only panics with documented error codes |
| `fuzz_admin_nonce` | — | Admin nonce replay-protection is exhaustively verified |
| `fuzz_batch_create` | TEST-21 (#244) | `create_streams_batch` handles 0–60 entries, mixed valid/invalid params |
| `fuzz_batch_cancel` | TEST-21 (#244) | `cancel_streams_batch` handles random stream ID sets, wrong owners, over-limit batches |
| `prop_state_machine` | TEST-02 | Full lifecycle state-machine: status transitions, withdrawn ≤ deposit, claimable ≥ 0 |
| `prop_claimable_invariant` | TEST-22 (#245) | `claimable ≤ deposit - withdrawn` for all inputs; zero for Paused / Cancelled / Exhausted |
| `fuzz_update_rate_withdraw` | TEST-59 (#574) | Random `update_rate` / `withdraw` interleavings keep claimable monotonic and ≤ deposit; no panics, no double-counted `pending_accrual` |
| `fuzz_settle_stream` | TEST-60 (#575) | `settle_stream` across random `stop_time` / claimable combos: no double counting, `withdrawn == pre-claimable`, status `Exhausted`, including claimable 0 |

---

## CI

The CI fuzz workflow runs each target for 30 seconds with a fixed seed:

```sh
PROPTEST_SEED=0 cargo test --package paystream-stream-fuzz --bin fuzz_batch_create
PROPTEST_SEED=0 cargo test --package paystream-stream-fuzz --bin fuzz_batch_cancel
PROPTEST_SEED=0 cargo test --package paystream-stream-fuzz --bin prop_claimable_invariant
PROPTEST_SEED=0 cargo test --package paystream-stream-fuzz --bin fuzz_update_rate_withdraw
PROPTEST_SEED=0 cargo test --package paystream-stream-fuzz --bin fuzz_settle_stream
```

Targets are expected to complete without new crashes. Any panic not matching
a documented error code is treated as a bug.
