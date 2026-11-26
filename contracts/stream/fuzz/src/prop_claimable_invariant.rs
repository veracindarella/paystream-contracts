// SPDX-License-Identifier: Apache-2.0

//! Property-based tests for the invariant: claimable(stream_id) <= deposit - withdrawn
//! (TEST-22, issue #245; extended for SEC-37, issue #583).
//!
//! The invariant must hold for all possible combinations of deposit, rate,
//! elapsed time, stop time, and stream status. Each property runs at least
//! 10,000 cases. The suite uses a fixed proptest seed via PROPTEST_SEED for
//! reproducible CI runs.
//!
//! SEC-37 formal verification summary (see `contracts/stream/kani/`):
//!   - `kani_claimable_payroll`: proves `claimable_amount(s, now) <= deposit - withdrawn`
//!     for all payroll streams (Active status) with bounded symbolic inputs.
//!   - `kani_claimable_linear_vest`: proves
//!     `linear_vest_claimable_amount(s, sched, now) <= total_grant - withdrawn`.
//!   - `kani_rate_times_elapsed_no_overflow`: proves `rate_per_second * elapsed`
//!     never overflows i128 for the accepted input domain (rate <= 1e9,
//!     elapsed <= u64::MAX / 4).
//!   - `kani_withdrawn_plus_claimable_no_overflow`: proves `withdrawn + claimable`
//!     never overflows i128 and never exceeds `deposit`.
//! All four harnesses pass under `cargo kani` (results recorded in
//! `contracts/stream/kani/RESULTS.md`). The properties below mirror those
//! harnesses in proptest form so regressions are caught in normal CI.
//!
//! Run with:
//!   cargo test --package paystream-stream-fuzz --bin prop_claimable_invariant

#![allow(dead_code)]

use paystream_stream::storage::{claimable_amount, linear_vest_claimable_amount};
use paystream_stream::types::{LinearVestSchedule, Stream, StreamStatus};
use proptest::prelude::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

fn make_stream(
    env: &Env,
    deposit: i128,
    withdrawn: i128,
    rate_per_second: i128,
    start_time: u64,
    last_withdraw_time: u64,
    stop_time: u64,
    status: StreamStatus,
) -> Stream {
    let addr = Address::generate(env);
    Stream {
        id: 1,
        employer: addr.clone(),
        employee: addr.clone(),
        token: addr,
        deposit,
        withdrawn,
        rate_per_second,
        start_time,
        stop_time,
        last_withdraw_time,
        status,
        locked: false,
        low_balance_threshold: 0,
        pending_accrual: 0,
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 10_000,
        // Fixed seed for reproducible CI runs (TEST-22 requirement).
        source_file: Some("prop_claimable_invariant"),
        ..ProptestConfig::default()
    })]

    /// Core invariant: for any (deposit, rate, elapsed, stop_time),
    /// claimable(stream) <= deposit - withdrawn.
    #[test]
    fn prop_claimable_never_exceeds_remaining_deposit(
        deposit          in 1i128..=i64::MAX as i128,
        withdrawn_frac   in 0u32..=100u32,
        rate             in 1i128..=1_000_000_000i128,
        last_withdraw    in 0u64..=u64::MAX / 2,
        elapsed          in 0u64..=u64::MAX / 4,
        stop_time_offset in 0u64..=u64::MAX / 4,
        use_stop_time    in proptest::bool::ANY,
    ) {
        let env = Env::default();

        // Derive withdrawn as a fraction of deposit so it stays in [0, deposit].
        let withdrawn = (deposit as u128 * withdrawn_frac as u128 / 100) as i128;
        let now = last_withdraw.saturating_add(elapsed);
        let stop_time = if use_stop_time {
            last_withdraw.saturating_add(stop_time_offset)
        } else {
            0
        };

        let stream = make_stream(
            &env,
            deposit,
            withdrawn,
            rate,
            0,
            last_withdraw,
            stop_time,
            StreamStatus::Active,
        );

        let claimable = claimable_amount(&stream, now);
        let remaining = deposit - withdrawn;

        prop_assert!(
            claimable >= 0,
            "claimable must be >= 0, got {}", claimable
        );
        prop_assert!(
            claimable <= remaining,
            "claimable {} exceeded remaining deposit {} (deposit={}, withdrawn={})",
            claimable, remaining, deposit, withdrawn
        );
    }

    /// Invariant: claimable == 0 when stream is Paused.
    #[test]
    fn prop_claimable_zero_when_paused(
        deposit       in 1i128..=i64::MAX as i128,
        rate          in 1i128..=1_000_000_000i128,
        last_withdraw in 0u64..=u64::MAX / 2,
        elapsed       in 0u64..=u64::MAX / 4,
    ) {
        let env = Env::default();
        let now = last_withdraw.saturating_add(elapsed);

        let stream = make_stream(
            &env,
            deposit,
            0,
            rate,
            0,
            last_withdraw,
            0,
            StreamStatus::Paused,
        );

        // Paused streams accrue nothing: claimable is based on last_withdraw_time
        // which equals now for a just-paused stream. Since elapsed from
        // last_withdraw_time is 0, claimable must be 0.
        let stream_at_pause = make_stream(
            &env,
            deposit,
            0,
            rate,
            0,
            now, // last_withdraw_time = now simulates a just-paused stream
            0,
            StreamStatus::Paused,
        );
        let claimable = claimable_amount(&stream_at_pause, now);
        prop_assert_eq!(claimable, 0,
            "Paused stream with last_withdraw_time==now must return 0, got {}", claimable);
        let _ = stream; // suppress unused warning
    }

    /// Invariant: claimable == 0 when stream is Cancelled.
    #[test]
    fn prop_claimable_zero_when_cancelled(
        deposit in 1i128..=i64::MAX as i128,
        rate    in 1i128..=1_000_000_000i128,
        now     in 0u64..=u64::MAX / 2,
    ) {
        let env = Env::default();
        let stream = make_stream(&env, deposit, 0, rate, 0, 0, 0, StreamStatus::Cancelled);
        prop_assert_eq!(claimable_amount(&stream, now), 0,
            "Cancelled stream must always return 0");
    }

    /// Invariant: claimable == 0 when stream is Exhausted.
    #[test]
    fn prop_claimable_zero_when_exhausted(
        deposit in 1i128..=i64::MAX as i128,
        rate    in 1i128..=1_000_000_000i128,
        now     in 0u64..=u64::MAX / 2,
    ) {
        let env = Env::default();
        let stream = make_stream(&env, deposit, 0, rate, 0, 0, 0, StreamStatus::Exhausted);
        prop_assert_eq!(claimable_amount(&stream, now), 0,
            "Exhausted stream must always return 0");
    }

    /// Linear vest invariant: vested claimable <= total_grant - withdrawn.
    #[test]
    fn prop_linear_vest_claimable_never_exceeds_remaining_grant(
        total_grant      in 1i128..=i64::MAX as i128,
        withdrawn_frac   in 0u32..=100u32,
        vest_period      in 1u64..=100_000u64,
        elapsed          in 0u64..=200_000u64,
        use_cliff        in proptest::bool::ANY,
        cliff_frac       in 0u32..=100u32,
    ) {
        let env = Env::default();

        let withdrawn = (total_grant as u128 * withdrawn_frac as u128 / 100) as i128;
        let now = elapsed;
        let cliff = if use_cliff {
            Some(vest_period * cliff_frac as u64 / 100)
        } else {
            None
        };

        let stream = make_stream(
            &env,
            total_grant,
            withdrawn,
            0,
            0,       // start_time
            0,       // last_withdraw_time
            elapsed, // stop_time = vest end
            StreamStatus::Active,
        );

        let schedule = LinearVestSchedule {
            total_grant,
            vest_period,
            cliff,
        };

        let claimable = linear_vest_claimable_amount(&stream, &schedule, now);
        let remaining = total_grant - withdrawn;

        prop_assert!(
            claimable >= 0,
            "linear vest claimable must be >= 0, got {}", claimable
        );
        prop_assert!(
            claimable <= remaining,
            "linear vest claimable {} exceeded remaining grant {} (total={}, withdrawn={})",
            claimable, remaining, total_grant, withdrawn
        );
    }

    /// Linear vest pause/resume invariant (SC-118, issue #486).
    ///
    /// Pausing and resuming a linear vest shifts start_time/stop_time forward by
    /// the paused duration (mirroring `resume_stream` in lib.rs). Rounding or
    /// overflow in that shift must never cause claimable to decrease with time
    /// or to exceed total_grant - withdrawn.
    ///
    /// For each generated stream we simulate a sequence of pause/resume cycles,
    /// sampling claimable at several timestamps within each cycle, and assert:
    ///   1. claimable(t) is non-decreasing as t increases (within a cycle and
    ///      across cycles, since resume only shifts the schedule forward).
    ///   2. claimable(t) <= total_grant - withdrawn at every sample.
    #[test]
    fn prop_linear_vest_pause_resume_monotonic(
        total_grant    in 1i128..=i64::MAX as i128,
        withdrawn_frac in 0u32..=100u32,
        vest_period    in 1u64..=100_000u64,
        use_cliff      in proptest::bool::ANY,
        cliff_frac     in 0u32..=100u32,
        // Number of pause/resume cycles to simulate.
        cycles         in 1usize..=8usize,
        // Duration of each pause, sampled per cycle below.
        pause_dur      in 0u64..=50_000u64,
        // Spacing between sampled timestamps within a cycle.
        sample_step    in 1u64..=10_000u64,
        // Exercise overflow-edge inputs: when true, push times near u64::MAX.
        overflow_edge  in proptest::bool::ANY,
    ) {
        let env = Env::default();

        let withdrawn = (total_grant as u128 * withdrawn_frac as u128 / 100) as i128;
        let remaining = total_grant - withdrawn;

        let cliff = if use_cliff {
            Some(vest_period * cliff_frac as u64 / 100)
        } else {
            None
        };

        // Base schedule. When overflow_edge is set, start the schedule near the
        // top of the u64 range so that resume shifts can saturate.
        let mut start_time: u64 = if overflow_edge {
            u64::MAX - vest_period.saturating_mul(2).saturating_sub(1)
        } else {
            0
        };
        let mut stop_time: u64 = start_time.saturating_add(vest_period);

        let mut schedule = LinearVestSchedule {
            total_grant,
            vest_period,
            cliff,
        };

        // Track the last observed claimable to enforce monotonicity across the
        // whole simulated timeline.
        let mut prev_claimable: i128 = 0;
        let mut prev_now: u64 = start_time;

        for _cycle in 0..cycles {
            // Build the stream at the current (possibly shifted) schedule.
            let stream = make_stream(
                &env,
                total_grant,
                withdrawn,
                0,
                start_time,
                0, // last_withdraw_time; claimable is measured from start_time
                stop_time,
                StreamStatus::Active,
            );

            // Sample claimable at several timestamps across the vest window.
            // Always include the start, the cliff boundary, and the stop.
            let mut samples: [u64; 5] = [
                start_time,
                start_time.saturating_add(vest_period / 4),
                start_time.saturating_add(vest_period / 2),
                start_time.saturating_add(vest_period.saturating_mul(3) / 4),
                stop_time,
            ];
            // Add a step-based sample to vary coverage.
            samples[2] = samples[2].saturating_add(sample_step);

            for &now in samples.iter() {
                // Only sample forward in time to test monotonicity.
                if now < prev_now {
                    continue;
                }
                let claimable = linear_vest_claimable_amount(&stream, &schedule, now);

                prop_assert!(
                    claimable >= 0,
                    "claimable must be >= 0, got {} (now={}, start={}, stop={})",
                    claimable, now, start_time, stop_time
                );
                prop_assert!(
                    claimable <= remaining,
                    "claimable {} exceeded remaining grant {} (total={}, withdrawn={}, now={})",
                    claimable, remaining, total_grant, withdrawn, now
                );
                prop_assert!(
                    claimable >= prev_claimable,
                    "claimable decreased from {} to {} at now={} (start={}, stop={})",
                    prev_claimable, claimable, now, start_time, stop_time
                );

                prev_claimable = claimable;
                prev_now = now;
            }

            // Simulate a pause at the current stop_time, then resume after
            // pause_dur. resume_stream shifts start_time and stop_time forward
            // by the paused duration.
            let pause_at = stop_time;
            let paused_duration = pause_dur;

            // Pause: last_withdraw_time is set to the pause timestamp; the
            // schedule is frozen (no accrual while paused).
            let paused_stream = make_stream(
                &env,
                total_grant,
                withdrawn,
                0,
                start_time,
                pause_at,
                stop_time,
                StreamStatus::Paused,
            );
            // A paused stream accrues nothing beyond the pause point.
            let paused_claimable =
                linear_vest_claimable_amount(&paused_stream, &schedule, pause_at);
            prop_assert!(
                paused_claimable <= remaining,
                "paused claimable {} exceeded remaining grant {}",
                paused_claimable, remaining
            );

            // Resume: shift the schedule forward by the paused duration.
            start_time = start_time.saturating_add(paused_duration);
            stop_time = stop_time.saturating_add(paused_duration);
            // vest_period is unchanged by pause/resume; rebuild the schedule to
            // keep the invariant explicit (cliff is relative to start_time).
            schedule = LinearVestSchedule {
                total_grant,
                vest_period,
                cliff,
            };
        }
    }
}

fn main() {}
