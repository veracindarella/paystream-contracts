# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Canonical read-only stream index entrypoints `stream_ids_by_employer` and `stream_ids_by_employee`; the former `streams_by_*` entrypoints remain as deprecated aliases.
- SC-06: `stream_status(stream_id) -> StreamStatus` — lightweight view that returns only
  the stream status without loading the full `Stream` struct into the caller's response.
  Reduces resource consumption for read-heavy off-chain indexers polling many streams.
- SC-31: `pause_stream_batch(employer, stream_ids)` — bulk pause entry point allowing employers
  to pause up to 50 active streams atomically in a single transaction. Emits status events on success
  and reverts atomically if any stream in the batch fails.
- SC-32: `resume_stream_batch(employer, stream_ids)` — bulk resume entry point allowing employers
  to resume up to 50 paused streams atomically in a single transaction, restarting accrual and resetting
  last_withdraw_time for each stream. Emits status events on success and reverts atomically if any stream is not Paused.
- SC-34: `get_auto_topup_config(stream_id) -> Option<AutoTopupConfig>` — public read-only query function
  for retrieving recurring auto top-up configuration for off-chain dashboards and indexers.
- `set_low_balance_threshold(employer, stream_id, threshold)` — allows employers to update or disable (`threshold = 0`) the low-balance alert threshold at any time without topping up. Emits `threshold_updated` event.

## [0.1.0] - 2026-04-24

### Added

- Initial `StreamContract` with `create_stream`, `withdraw`, `top_up`, `pause_stream`, `resume_stream`, and `cancel_stream` entrypoints
- Initial `TokenContract` (SEP-41 compliant) with `initialize`, `transfer`, `transfer_from`, `approve`, `mint`, and `burn`
- Per-second salary streaming with configurable `rate_per_second` and optional `stop_time`
- `claimable` and `claimable_at` query functions for real-time and projected earnings
- `create_streams_batch` for atomic multi-stream creation in a single transaction
- Multi-token support — each stream accepts any SEP-41 compliant token address
- Per-employer and per-employee stream index (`streams_by_employer`, `streams_by_employee`)
- Emergency contract-level `pause_contract` / `unpause_contract` for admin
- Admin-governed minimum deposit enforcement (`set_min_deposit`)
- Contract upgrade path via `upgrade` (WASM hash swap) and `migrate` (no-op base hook)
- Reentrancy guard (`locked` flag) on `withdraw` as defence-in-depth (issue #1)
- Checked arithmetic throughout `claimable_amount` to prevent silent overflow (issue #2)
- Zero-rate validation — `create_stream` rejects `rate_per_second = 0` with `E001` (issue #3)
- Graceful `withdraw` on exhausted streams returns `0` instead of panicking (issue #10)
- `top_up` rejects cancelled (`E005`) and exhausted (`E006`) streams (issue #11)
- Pause/resume time-accounting — paused intervals excluded from claimable calculation (issue #15)
- `StreamStatus` lifecycle: `Active → Paused → Active`, `Active → Cancelled`, `Active → Exhausted`
- On-chain events: `created`, `withdraw`, `status`, `topup`, `paused`
- Rust toolchain pinned via `rust-toolchain.toml`
- Docker-based local development environment (`docker-compose.yml`)
- GitHub Actions CI with build, test, lint, and `cargo audit` vulnerability scanning
- Branch protection script (`scripts/protect-main.sh`)
- Security audit report (Trail of Bits, 2026-04-23) — all high/medium findings resolved
- Gas optimisation pass: `withdraw` CPU instructions reduced ~19%, `claimable` ~21%
- Upgrade guide (`docs/upgrade-guide.md`)

### Security

- `top_up` now verifies `employer == stream.employer` before token transfer (audit HIGH-01)
- `claimable_amount` uses `checked_mul` to prevent overflow; panics with `E004` on overflow (audit MED-01)
- Reentrancy analysis documented; `locked` guard added as defence-in-depth (audit MED-02)

[Unreleased]: https://github.com/veracindarella/paystream-contracts/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/veracindarella/paystream-contracts/releases/tag/v0.1.0
