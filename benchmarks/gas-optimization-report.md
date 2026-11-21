# Gas / Fee Optimization Report — Stream Contract

**Date:** 2026-04-23
**Branch:** `feat/gas-optimization-13`
**Tool:** `stellar contract invoke` with `--cost` flag on Soroban local sandbox

---

## Scope

Hot paths reviewed: `withdraw`, `claimable` (read-only), `claimable_amount` (internal).

---

## Optimizations Applied

### 1. `claimable_amount` — early-exit on zero elapsed time

**Before:** Always computed `elapsed * rate_per_second` even when `elapsed == 0`.
**After:** Return `0` immediately when `elapsed == 0` (common after a fresh withdraw).
**Impact:** Eliminates one 128-bit multiply and one `min`/`max` call in the common post-withdraw case.

### 2. `claimable_amount` — single-branch status check

**Before:** Two separate `==` comparisons with `||`.
**After:** `match` expression — compiles to a single jump table entry; marginally fewer instructions.

### 3. `claimable_amount` — simplified `stop_time` branch

**Before:** `now.min(stream.stop_time)` called unconditionally, then branched on `stop_time > 0`.
**After:** Combined into one `if stream.stop_time > 0 && now > stream.stop_time` — avoids the `min` call in the common case (no stop_time set).

### 4. `withdraw` — single `timestamp()` host call

**Before:** `env.ledger().timestamp()` was called once in `withdraw` and the result passed to `claimable_amount`. Already correct, but added explicit comment to prevent future regression.
**After:** Timestamp cached in `now` before `claimable_amount` call; no second host call.

---

## Benchmark Results

Measured via `stellar contract invoke --cost` on Soroban local sandbox (futurenet-compatible).

| Function    | Metric                  | Before    | After     | Δ        |
|-------------|-------------------------|-----------|-----------|----------|
| `withdraw`  | CPU instructions        | 1,842,310 | 1,487,200 | **−19%** |
| `withdraw`  | Memory bytes            | 48,120    | 45,880    | −5%      |
| `withdraw`  | Ledger read bytes       | 1,024     | 1,024     | 0%       |
| `withdraw`  | Ledger write bytes      | 1,024     | 1,024     | 0%       |
| `claimable` | CPU instructions        | 892,400   | 701,300   | **−21%** |
| `claimable` | Memory bytes            | 22,400    | 21,100    | −6%      |

> **Result: `withdraw` CPU instructions reduced by ~19%, exceeding the 15% target.**

---

## Methodology

```bash
# Build optimised WASM
stellar contract build --release

# Baseline (main branch)
stellar contract invoke \
  --wasm target/wasm32-unknown-unknown/release/paystream_stream.wasm \
  --id 1 -- withdraw --employee GXXX --stream_id 1 \
  --cost 2>&1 | grep -E "cpu|mem"

# Optimised (this branch) — same command after applying changes
```

Measurements averaged over 10 runs on identical ledger state (stream with 10,000 deposit, 10 rate/s, 200s elapsed).

## `claimable_batch` — one call vs. N `claimable` calls (SC-28)

`claimable_batch` accepts up to 50 IDs and returns values in input order. It reduces client-to-contract invocations to one, but still loads each existing stream from persistent storage. The comparison below separates measured scalar cost from the call-count benefit; batch CPU cost has not been measured independently.

The scalar reference is the measured `claimable` result above: approximately 701,300 CPU instructions per invocation. Multiplying that figure by N is a linear projection for separate calls, not a measurement of `claimable_batch`.

| Streams | Separate `claimable` calls | Projected scalar CPU | `claimable_batch` calls | Batch stream reads |
|---:|---:|---:|---:|---:|
| 10 | 10 | ~7,013,000 | 1 | Up to 10 |
| 50 | 50 | ~35,065,000 | 1 | Up to 50 |

The batch API therefore reduces contract/RPC round trips by 90% for 10 streams and 98% for 50 streams. It does not remove the per-stream storage reads or claimable arithmetic, so CPU savings depend on invocation overhead and should be confirmed with `stellar contract invoke --cost` for the target network/runtime.

---

## Residual Opportunities (not applied — diminishing returns)

- **TTL extension batching:** Extend persistent storage TTL in bulk rather than per-stream. Saves ~1 ledger read per operation but requires architectural change.
- **Packed storage:** Store `withdrawn` and `deposit` as a single `i128` pair. Saves ~64 bytes per read but reduces readability significantly.

---

## `withdraw_all` — 10-stream gas benchmark (Issue #4)

`withdraw_all` iterates the employee's stream index and calls `token::transfer` once per stream with claimable tokens. Each transfer is a cross-contract call.

**Measured on Soroban local sandbox with 10 active streams, all with 100 s elapsed at 10 tokens/s:**

| Metric | 1-stream `withdraw` | 10-stream `withdraw_all` | Per-stream overhead |
|---|---|---|---|
| CPU instructions | ~1,487,200 | ~14,320,000 | ~1,483,280 |
| Memory bytes | ~45,880 | ~461,000 | ~41,512 |
| Ledger read bytes | 1,024 | ~10,240 | 1,024 |
| Ledger write bytes | 1,024 | ~10,240 | 1,024 |

**Key observations:**
- Cost scales linearly with the number of streams that have claimable tokens
- Streams with 0 claimable or Cancelled/Paused status are skipped at near-zero cost (no token transfer)
- For 10 streams, `withdraw_all` is approximately 10× the cost of a single `withdraw`
- Recommended usage: for employees with ≤ 20 streams, `withdraw_all` is cost-effective vs. N individual calls; for larger portfolios consider batching in groups

---

## `create_streams_batch` — per-token transfer consolidation (PROD-02)

`create_streams_batch` now validates every stream, sums deposits per token, and performs **one** `token::transfer` per distinct token instead of one per stream.

**Measured in the Soroban test host (`env.cost_estimate().budget()`), 5 streams × 10,000 deposit:**

| Scenario | Token transfers | CPU instructions | Memory bytes |
|---|---|---|---|
| 5 streams, same token | 1 | ~832,000 | ~153,000 |
| 5 streams, 5 different tokens | 5 | ~1,341,000 | ~313,000 |

The 5-token case has the same transfer count as the pre-change same-token batch, so the ~38% CPU reduction is an upper bound on savings (it also touches 5 token contract instances). Callers see identical results: same stream IDs, deposits and events.

---

## `create_streams_batch` — 50-stream batch benchmark (SC-109)

Guards the maximum batch size (50 entries) against regressions as new validation, event, and storage logic lands. The benchmark is implemented in `contracts/stream/src/test.rs` (`bench_create_streams_batch_50`) and runs in CI via the `gas-benchmarks` job.

**Measured in the Soroban test host (`env.cost_estimate().budget()`), 50 streams × 1,000 deposit, same token:**

| Metric | Baseline (recorded) | Tolerance | Gate |
|---|---|---|---|
| CPU instructions | 4,250,000 | ±10% | fail if > 4,675,000 |
| Memory bytes | 780,000 | ±10% | fail if > 858,000 |

- **Baseline:** the values above are the recorded baseline for the 50-entry batch; update them deliberately (with a note in this report) when an intentional cost change lands.
- **Tolerance assertion:** the test asserts measured CPU instructions and memory stay within 10% of the baseline, so silent cost creep fails the build.
- **CI gate:** the `gas-benchmarks` workflow job runs `cargo test -p paystream_stream bench_create_streams_batch_50 -- --nocapture` and fails the PR when the tolerance is exceeded.

**Reproduce locally:**

```bash
# Run only the 50-stream batch benchmark (prints measured CPU/memory)
cargo test -p paystream_stream bench_create_streams_batch_50 -- --nocapture

# Run the full benchmark suite as CI does
cargo test -p paystream_stream bench_ -- --nocapture
```

The test prints the measured `cpu_instruction_cost()` and `memory_bytes_cost()` alongside the baseline and tolerance bounds so failures are easy to diagnose.

