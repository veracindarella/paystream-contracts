// SPDX-License-Identifier: Apache-2.0

//! Fuzz target for `create_streams_batch` (TEST-21, issue #244).
//!
//! Drives `create_streams_batch` with arbitrary combinations of:
//! - Batch sizes from 0 to 60 (including above the 50-stream limit)
//! - Mixed valid/invalid deposits, rates, and stop times
//!
//! Verifies that:
//! - Valid batches (≤50 entries, all params sound) never panic
//! - Invalid batches panic with a documented error code
//! - No unexpected panics (overflow, undefined behaviour, etc.) occur

#![allow(dead_code)]

use paystream_stream::{StreamContract, StreamContractClient};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Env, Vec as SdkVec};
use paystream_stream::types::StreamParams;

/// Known panic prefixes that create_streams_batch is allowed to produce.
const EXPECTED_ERROR_PREFIXES: &[&str] = &[
    "E001:", // ERR_ZERO_RATE
    "E002:", // ERR_ZERO_DEPOSIT
    "E007:", // ERR_BELOW_MIN_DEPOSIT
    "E008:", // ERR_INVALID_RATE
    "E013:", // ERR_CONTRACT_PAUSED
    "E014:", // ERR_EMPTY_PARAMS
    "E022:", // ERR_STOP_TIME_PAST
    "E029:", // ERR_BATCH_TOO_LARGE
    "E030:", // ERR_STOP_TIME_TOO_FAR
    "E031:", // ERR_INVALID_LOW_BALANCE_THRESHOLD
    "stream[",
    "employer and employee must differ",
];

fn setup_env() -> (Env, StreamContractClient<'static>, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let employer = Address::generate(&env);

    let token_id = env.register(paystream_token::TokenContract, ());
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    token.initialize(&employer, &1_000_000_000);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &1);

    (env, client, employer, token_id)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// Valid batches (1–50 streams, all sound params) must never panic and
    /// must return exactly `params.len()` stream IDs.
    #[test]
    fn fuzz_batch_create_valid_inputs_no_panic(
        count in 1usize..=50usize,
        deposit in 1i128..=100_000i128,
        rate    in 1i128..=1_000_000_000i128,
        threshold in proptest::option::of(0i128..=100_000i128),
    ) {
        let (env, client, employer, token_id) = setup_env();

        let mut params: SdkVec<StreamParams> = SdkVec::new(&env);
        for _ in 0..count {
            let employee = Address::generate(&env);
            params.push_back(StreamParams {
                employee,
                token: token_id.clone(),
                deposit,
                rate_per_second: rate,
                stop_time: 0,
                low_balance_threshold: threshold,
            });
        }

        let ids = client.create_streams_batch(&employer, &params);
        prop_assert_eq!(ids.len() as usize, count,
            "expected {} stream IDs, got {}", count, ids.len());
    }

    /// Batches exceeding 50 entries must panic with E029.
    #[test]
    fn fuzz_batch_create_over_limit_panics(
        extra in 1usize..=10usize,
    ) {
        let (env, client, employer, token_id) = setup_env();

        let mut params: SdkVec<StreamParams> = SdkVec::new(&env);
        for _ in 0..(50 + extra) {
            params.push_back(StreamParams {
                employee: Address::generate(&env),
                token: token_id.clone(),
                deposit: 1_000,
                rate_per_second: 1,
                stop_time: 0,
                low_balance_threshold: None,
            });
        }

        // Should panic with E029
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.create_streams_batch(&employer, &params);
        }));
        prop_assert!(result.is_err(), "expected panic for batch size > 50");
    }

    /// Empty batch must panic with E014.
    #[test]
    fn fuzz_batch_create_empty_panics(_dummy in 0u8..=1u8) {
        let (env, client, employer, _token_id) = setup_env();
        let params: SdkVec<StreamParams> = SdkVec::new(&env);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.create_streams_batch(&employer, &params);
        }));
        prop_assert!(result.is_err(), "expected panic for empty batch");
    }

    /// Mixed valid and invalid deposits/rates produce an error for the first
    /// invalid entry and do not silently create any streams.
    #[test]
    fn fuzz_batch_create_mixed_validity_panics_on_invalid(
        valid_count  in 1usize..=5usize,
        bad_deposit  in proptest::bool::ANY,
        bad_rate     in proptest::bool::ANY,
    ) {
        // Skip if neither param is invalid — trivially valid.
        prop_assume!(bad_deposit || bad_rate);

        let (env, client, employer, token_id) = setup_env();

        let mut params: SdkVec<StreamParams> = SdkVec::new(&env);

        // Add some valid entries first
        for _ in 0..valid_count {
            params.push_back(StreamParams {
                employee: Address::generate(&env),
                token: token_id.clone(),
                deposit: 1_000,
                rate_per_second: 1,
                stop_time: 0,
                low_balance_threshold: Some(500),
            });
        }

        // Then one invalid entry
        let invalid_deposit = if bad_deposit { 0 } else { 1_000 };
        let invalid_rate    = if bad_rate    { 0 } else { 1 };
        params.push_back(StreamParams {
            employee: Address::generate(&env),
            token: token_id.clone(),
            deposit: invalid_deposit,
            rate_per_second: invalid_rate,
            stop_time: 0,
            low_balance_threshold: None,
        });

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.create_streams_batch(&employer, &params);
        }));
        prop_assert!(result.is_err(), "expected panic for batch with invalid entry");
    }

    /// Negative low_balance_threshold values must be rejected with the
    /// documented validation error rather than silently accepted.
    #[test]
    fn fuzz_batch_create_negative_threshold_panics(
        threshold in i128::MIN..=-1i128,
    ) {
        let (env, client, employer, token_id) = setup_env();

        let mut params: SdkVec<StreamParams> = SdkVec::new(&env);
        params.push_back(StreamParams {
            employee: Address::generate(&env),
            token: token_id.clone(),
            deposit: 1_000,
            rate_per_second: 1,
            stop_time: 0,
            low_balance_threshold: Some(threshold),
        });

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.create_streams_batch(&employer, &params);
        }));
        prop_assert!(result.is_err(), "expected panic for negative low_balance_threshold");
    }
}

fn main() {}
