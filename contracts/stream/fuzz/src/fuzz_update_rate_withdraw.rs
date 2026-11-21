//! Fuzz target: random interleavings of `update_rate` and `withdraw`.
//!
//! Properties asserted:
//! - `claimable` never exceeds `deposit`.
//! - `claimable` is monotonic non-decreasing across operations.
//! - No panics occur across random rate changes.
//! - `pending_accrual` never double-counts (claimable + withdrawn <= deposit).

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use stream_contract::{StreamContract, StreamContractClient};

/// A single fuzz operation.
#[derive(Arbitrary, Debug, Clone)]
enum Op {
    /// Update the stream rate to a random value.
    UpdateRate { rate: i128 },
    /// Withdraw a random amount.
    Withdraw { amount: i128 },
    /// Advance the ledger timestamp by a random delta.
    Advance { delta: u64 },
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    deposit: i128,
    initial_rate: i128,
    start_time: u64,
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    // Bound the deposit to a sane, positive range so the stream can be created.
    let deposit = input.deposit.rem_euclid(1_000_000_000) + 1;
    let initial_rate = input.initial_rate.rem_euclid(1_000_000) + 1;

    let env = soroban_sdk::Env::default();
    let contract_id = env.register_contract(None, StreamContract);
    let client = StreamContractClient::new(&env, &contract_id);

    let sender = soroban_sdk::Address::generate(&env);
    let recipient = soroban_sdk::Address::generate(&env);

    // Create the stream. If creation fails for any reason, skip this input.
    let start_time = input.start_time;
    if client
        .try_create_stream(
            &sender,
            &recipient,
            &deposit,
            &initial_rate,
            &start_time,
        )
        .is_err()
    {
        return;
    }

    let mut current_time = start_time;
    let mut last_claimable: i128 = 0;
    let mut total_withdrawn: i128 = 0;

    for op in input.ops.iter() {
        match op {
            Op::UpdateRate { rate } => {
                // Normalize rate to a positive value.
                let new_rate = rate.rem_euclid(1_000_000) + 1;
                // Must not panic across random rate changes.
                let _ = client.try_update_rate(&sender, &new_rate);
            }
            Op::Withdraw { amount } => {
                // Normalize withdrawal amount to a non-negative value.
                let withdraw_amount = amount.rem_euclid(deposit + 1);
                let result = client.try_withdraw(&recipient, &withdraw_amount);
                if let Ok(Ok(withdrawn)) = result {
                    total_withdrawn = total_withdrawn.saturating_add(withdrawn);
                }
            }
            Op::Advance { delta } => {
                current_time = current_time.saturating_add(*delta);
                env.ledger().set_timestamp(current_time);
            }
        }

        // Read the current claimable amount.
        let claimable = client.claimable(&recipient);

        // Property: claimable never exceeds deposit.
        assert!(
            claimable <= deposit,
            "claimable {} exceeds deposit {}",
            claimable,
            deposit
        );

        // Property: claimable is monotonic non-decreasing.
        assert!(
            claimable >= last_claimable,
            "claimable decreased from {} to {}",
            last_claimable,
            claimable
        );
        last_claimable = claimable;

        // Property: pending_accrual never double-counts.
        // The sum of what has been withdrawn and what remains claimable must
        // never exceed the original deposit.
        assert!(
            total_withdrawn.saturating_add(claimable) <= deposit,
            "withdrawn {} + claimable {} exceeds deposit {}",
            total_withdrawn,
            claimable,
            deposit
        );
    }
});