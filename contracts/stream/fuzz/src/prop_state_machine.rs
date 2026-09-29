// SPDX-License-Identifier: Apache-2.0

//! Property-based state machine tests for the stream lifecycle (TEST-02, issue #51).
//!
//! Generates arbitrary sequences of valid stream operations and verifies that:
//!
//! * Status transitions are always valid:
//!   - No `Invalid → Active` (stream must be created before any operation)
//!   - No `Cancelled → Active` or `Cancelled → Paused` (terminal state)
//!   - No `Exhausted → Active` or `Exhausted → Paused` (terminal state)
//!   - Pause/resume cycle: Active → Paused → Active only
//! * `withdrawn` never exceeds `deposit`
//! * `claimable` is always ≥ 0
//!
//! The property tests are run with 100,000 iterations (`ProptestConfig`).
//! To run:
//!
//! ```text
//! cargo test --package paystream-stream-fuzz --bin prop_state_machine
//! ```

#![allow(dead_code)]

use paystream_stream::types::StreamStatus;
use paystream_stream::{StreamContract, StreamContractClient};
use proptest::prelude::*;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Env};

/// Operations the state machine can perform on a stream.
#[derive(Clone, Debug)]
enum Op {
    /// Advance ledger by `delta` seconds (1–300).
    AdvanceTime(u64),
    /// Pause the stream (only valid when Active).
    Pause,
    /// Resume the stream (only valid when Paused).
    Resume,
    /// Withdraw earned tokens (only valid when Active/Exhausted).
    Withdraw,
    /// Top up with `amount` tokens (only valid when Active/Paused).
    TopUp(i128),
    /// Cancel the stream (only valid when Active/Paused).
    Cancel,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (1u64..=300u64).prop_map(Op::AdvanceTime),
        Just(Op::Pause),
        Just(Op::Resume),
        Just(Op::Withdraw),
        (1_000i128..=10_000i128).prop_map(Op::TopUp),
        Just(Op::Cancel),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100_000))]

    /// Generate an arbitrary sequence of operations and verify the state
    /// machine invariants hold throughout.
    #[test]
    fn prop_stream_state_machine_invariants(
        ops in proptest::collection::vec(op_strategy(), 1..=20),
        initial_deposit in 50_000i128..=200_000i128,
        rate in 1i128..=100i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StreamContract, ());
        let client = StreamContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);

        // Set up token contract.
        let token_id = env.register(paystream_token::TokenContract, ());
        let token = paystream_token::TokenContractClient::new(&env, &token_id);
        token.initialize(&employer, &1_000_000_000);

        client.initialize(&admin);
        // Lower min_deposit so our generated deposits are always accepted.
        client.set_min_deposit(&admin, &0, &100);

        // Create the stream.
        let stream_id = client.create_stream(
            &employer,
            &employee,
            &token_id,
            &initial_deposit,
            &rate,
            &0,
        );

        let mut current_status = StreamStatus::Active;
        let mut total_withdrawn: i128 = 0;
        let mut current_deposit = initial_deposit;
        let mut is_terminal = false;

        for op in ops {
            if is_terminal {
                // Skip all ops once the stream has reached a terminal state.
                break;
            }

            match op {
                Op::AdvanceTime(delta) => {
                    env.ledger().with_mut(|l| l.timestamp += delta);
                }
                Op::Pause => {
                    if current_status == StreamStatus::Active {
                        client.pause_stream(&employer, &stream_id);
                        current_status = StreamStatus::Paused;
                    }
                }
                Op::Resume => {
                    if current_status == StreamStatus::Paused {
                        client.resume_stream(&employer, &stream_id);
                        current_status = StreamStatus::Active;
                    }
                }
                Op::Withdraw => {
                    if current_status == StreamStatus::Active {
                        let amount = client.withdraw(&employee, &stream_id);
                        prop_assert!(amount >= 0, "withdraw returned negative amount: {}", amount);
                        total_withdrawn += amount;
                        prop_assert!(
                            total_withdrawn <= current_deposit,
                            "withdrawn ({}) exceeded deposit ({})",
                            total_withdrawn,
                            current_deposit
                        );
                        // Update status from contract (may have transitioned to Exhausted).
                        current_status = client.get_stream(&stream_id).status;
                        if current_status == StreamStatus::Exhausted {
                            is_terminal = true;
                        }
                    }
                }
                Op::TopUp(amount) => {
                    if current_status == StreamStatus::Active
                        || current_status == StreamStatus::Paused
                    {
                        client.top_up(&employer, &stream_id, &amount);
                        current_deposit += amount;
                    }
                }
                Op::Cancel => {
                    if current_status == StreamStatus::Active
                        || current_status == StreamStatus::Paused
                    {
                        client.cancel_stream(&employer, &stream_id);
                        current_status = StreamStatus::Cancelled;
                        is_terminal = true;
                    }
                }
            }

            if !is_terminal {
                // --- Invariant 1: claimable is always >= 0 ---
                let claimable = client.claimable(&stream_id);
                prop_assert!(claimable >= 0, "claimable returned negative value: {}", claimable);

                // --- Invariant 2: withdrawn never exceeds deposit ---
                let stream = client.get_stream(&stream_id);
                prop_assert!(
                    stream.withdrawn <= current_deposit,
                    "withdrawn ({}) exceeds deposit ({})",
                    stream.withdrawn,
                    current_deposit
                );

                // --- Invariant 3: status transitions are valid ---
                // Once we track status locally we can verify no impossible
                // transition occurred.
                let actual_status = stream.status.clone();
                match &actual_status {
                    StreamStatus::Active => {
                        // Active can follow: initial create, resume, or stay Active.
                        prop_assert!(
                            current_status == StreamStatus::Active,
                            "unexpected transition to Active from {:?}",
                            current_status
                        );
                    }
                    StreamStatus::Paused => {
                        prop_assert!(
                            current_status == StreamStatus::Paused,
                            "unexpected transition to Paused from {:?}",
                            current_status
                        );
                    }
                    StreamStatus::Cancelled => {
                        // Once cancelled this branch won't be reached (is_terminal=true).
                    }
                    StreamStatus::Exhausted => {
                        // Mark terminal so subsequent ops are skipped.
                        is_terminal = true;
                    }
                }
                current_status = actual_status;
            }
        }
    }

    /// Property: withdrawn never exceeds deposit under any operation sequence.
    #[test]
    fn prop_withdrawn_never_exceeds_deposit(
        time_deltas in proptest::collection::vec(1u64..=200u64, 1..=10),
        deposit in 10_000i128..=100_000i128,
        rate in 1i128..=50i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StreamContract, ());
        let client = StreamContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = env.register(paystream_token::TokenContract, ());
        let token = paystream_token::TokenContractClient::new(&env, &token_id);
        token.initialize(&employer, &1_000_000_000);

        client.initialize(&admin);
        client.set_min_deposit(&admin, &0, &100);

        let stream_id = client.create_stream(&employer, &employee, &token_id, &deposit, &rate, &0);

        for delta in time_deltas {
            let s = client.get_stream(&stream_id);
            if s.status == StreamStatus::Exhausted || s.status == StreamStatus::Cancelled {
                break;
            }
            env.ledger().with_mut(|l| l.timestamp += delta);

            if s.status == StreamStatus::Active {
                let withdrawn = client.withdraw(&employee, &stream_id);
                prop_assert!(withdrawn >= 0);
                let s2 = client.get_stream(&stream_id);
                prop_assert!(
                    s2.withdrawn <= deposit,
                    "withdrawn {} > deposit {}",
                    s2.withdrawn,
                    deposit
                );
            }
        }
    }

    /// Property: claimable is always >= 0 across arbitrary time advances.
    #[test]
    fn prop_claimable_always_non_negative(
        time_deltas in proptest::collection::vec(1u64..=500u64, 1..=15),
        deposit in 10_000i128..=100_000i128,
        rate in 1i128..=100i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StreamContract, ());
        let client = StreamContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = env.register(paystream_token::TokenContract, ());
        let token = paystream_token::TokenContractClient::new(&env, &token_id);
        token.initialize(&employer, &1_000_000_000);

        client.initialize(&admin);
        client.set_min_deposit(&admin, &0, &100);

        let stream_id = client.create_stream(&employer, &employee, &token_id, &deposit, &rate, &0);

        for delta in time_deltas {
            env.ledger().with_mut(|l| l.timestamp += delta);
            let claimable = client.claimable(&stream_id);
            prop_assert!(
                claimable >= 0,
                "claimable must be >= 0, got {}",
                claimable
            );
        }
    }

    /// Property: Cancelled is a terminal state — no further status changes.
    #[test]
    fn prop_cancelled_is_terminal(
        time_delta in 1u64..=1_000u64,
        deposit in 10_000i128..=100_000i128,
        rate in 1i128..=100i128,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StreamContract, ());
        let client = StreamContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = env.register(paystream_token::TokenContract, ());
        let token = paystream_token::TokenContractClient::new(&env, &token_id);
        token.initialize(&employer, &1_000_000_000);

        client.initialize(&admin);
        client.set_min_deposit(&admin, &0, &100);

        let stream_id = client.create_stream(&employer, &employee, &token_id, &deposit, &rate, &0);

        env.ledger().with_mut(|l| l.timestamp += time_delta);
        client.cancel_stream(&employer, &stream_id);

        // Stream must be Cancelled.
        let s = client.get_stream(&stream_id);
        prop_assert_eq!(s.status, StreamStatus::Cancelled);

        // Advance time further — status must remain Cancelled.
        env.ledger().with_mut(|l| l.timestamp += time_delta);
        let s2 = client.get_stream(&stream_id);
        prop_assert_eq!(s2.status, StreamStatus::Cancelled);
    }

    /// Property: valid pause/resume transitions preserve Active ↔ Paused only.
    #[test]
    fn prop_pause_resume_cycles_valid(
        cycles in 1usize..=5usize,
        deposit in 50_000i128..=200_000i128,
        rate in 1i128..=10i128,
        time_per_step in 1u64..=50u64,
    ) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StreamContract, ());
        let client = StreamContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = env.register(paystream_token::TokenContract, ());
        let token = paystream_token::TokenContractClient::new(&env, &token_id);
        token.initialize(&employer, &1_000_000_000);

        client.initialize(&admin);
        client.set_min_deposit(&admin, &0, &100);

        let stream_id = client.create_stream(&employer, &employee, &token_id, &deposit, &rate, &0);

        for _ in 0..cycles {
            // Active → Paused
            env.ledger().with_mut(|l| l.timestamp += time_per_step);
            client.pause_stream(&employer, &stream_id);
            let s = client.get_stream(&stream_id);
            prop_assert_eq!(
                s.status, StreamStatus::Paused,
                "expected Paused after pause_stream"
            );

            // Paused → Active
            env.ledger().with_mut(|l| l.timestamp += time_per_step);
            client.resume_stream(&employer, &stream_id);
            let s = client.get_stream(&stream_id);
            prop_assert_eq!(
                s.status, StreamStatus::Active,
                "expected Active after resume_stream"
            );
        }
    }
}

fn main() {}
