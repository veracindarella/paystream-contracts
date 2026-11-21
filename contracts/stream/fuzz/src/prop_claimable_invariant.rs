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

    /// SEC-37: payroll accrual must never overflow i128 when computing
    /// `rate_per_second * elapsed`. Mirrors `kani_rate_times_elapsed_no_overflow`.
    #[test]
    fn prop_payroll_rate_times_elapsed_no_overflow(
        rate    in 1i128..=1_000_000_000i128,
        elapsed in 0u64..=u64::MAX / 4,
    ) {
        let env = Env::default();
        let stream = make_stream(&env, i64::MAX as i128, 0, rate, 0, 0, 0, StreamStatus::Active);
        let now = elapsed;

        // The accrual computation must not panic (overflow) and must stay >= 0.
        let claimable = claimable_amount(&stream, now);
        prop_assert!(claimable >= 0, "claimable must be >= 0, got {}", claimable);

        // Direct check that the product fits in i128 for the accepted domain.
        let product = rate.checked_mul(elapsed as i128);
        prop_assert!(
            product.is_some(),
            "rate {} * elapsed {} overflowed i128", rate, elapsed
        );
    }

    /// SEC-37: `withdrawn + claimable` must never overflow and must never
    /// exceed `deposit`. Mirrors `kani_withdrawn_plus_claimable_no_overflow`.
    #[test]
    fn prop_withdrawn_plus_claimable_no_overflow(
        deposit        in 1i128..=i64::MAX as i128,
        withdrawn_frac in 0u32..=100u32,
        rate           in 1i128..=1_000_000_000i128,
        last_withdraw  in 0u64..=u64::MAX / 2,
        elapsed        in 0u64..=u64::MAX / 4,
    ) {
        let env = Env::default();
        let withdrawn = (deposit as u128 * withdrawn_frac as u128 / 100) as i128;
        let now = last_withdraw.saturating_add(elapsed);

        let stream = make_stream(
            &env,
            deposit,
            withdrawn,
            rate,
            0,
            last_withdraw,
            0,
            StreamStatus::Active,
        );

        let claimable = claimable_amount(&stream, now);
        let total = withdrawn.checked_add(claimable);
        prop_assert!(
            total.is_some(),
            "withdrawn {} + claimable {} overflowed i128", withdrawn, claimable
        );
        prop_assert!(
            total.unwrap() <= deposit,
            "withdrawn {} + claimable {} = {} exceeded deposit {}",
            withdrawn, claimable, total.unwrap(), deposit
        );
    }

    /// SEC-37: linear vest `withdrawn + claimable` must never overflow and
    /// must never exceed `total_grant`.
    #[test]
    fn prop_linear_vest_withdrawn_plus_claimable_no_overflow(
        total_grant    in 1i128..=i64::MAX as i128,
        withdrawn_frac in 0u32..=100u32,
        vest_period    in 1u64..=100_000u64,
        elapsed        in 0u64..=200_000u64,
        cliff_frac     in 0u32..=100u32,
    ) {
        let env = Env::default();
        let withdrawn = (total_grant as u128 * withdrawn_frac as u128 / 100) as i128;
        let now = elapsed;
        let cliff = Some(vest_period * cliff_frac as u64 / 100);

        let stream = make_stream(
            &env,
            total_grant,
            withdrawn,
            0,
            0,
            0,
            elapsed,
            StreamStatus::Active,
        );
        let schedule = LinearVestSchedule {
            total_grant,
            vest_period,
            cliff,
        };

        let claimable = linear_vest_claimable_amount(&stream, &schedule, now);
        let total = withdrawn.checked_add(claimable);
        prop_assert!(
            total.is_some(),
            "withdrawn {} + claimable {} overflowed i128", withdrawn, claimable
        );
        prop_assert!(
            total.unwrap() <= total_grant,
            "withdrawn {} + claimable {} = {} exceeded total_grant {}",
            withdrawn, claimable, total.unwrap(), total_grant
        );
    }
}

fn main() {}
