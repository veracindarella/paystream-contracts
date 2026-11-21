//! Fuzz target for `settle_stream`.
//!
//! Verifies the SC-73 invariant: after settling a stream, the amount withdrawn
//! equals the pre-settle claimable amount, the stream transitions to the
//! `Exhausted` status, and no double counting occurs. Also exercises the edge
//! case where the claimable amount is zero.

#![no_main]

use libfuzzer_sys::fuzz_target;

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, Address, Env,
};

use stream_contract::{StreamContract, StreamContractClient, StreamStatus};

/// Build a fresh environment with a funded token and a created stream.
///
/// Returns the env, the client, the stream id, the token address, the sender,
/// the recipient, and the total deposit amount.
fn setup(
    stop_time: u64,
    deposit: i128,
) -> (
    Env,
    StreamContractClient<'static>,
    u64,
    Address,
    Address,
    Address,
    i128,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, StreamContract);
    let client = StreamContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract(token_admin.clone());
    let token_client = token::StellarAssetClient::new(&env, &token_id);

    let sender = Address::generate(&env);
    let recipient = Address::generate(&env);

    let deposit = if deposit <= 0 { 1 } else { deposit };
    token_client.mint(&sender, &deposit);

    let start_time = env.ledger().timestamp();
    let stream_id = client.create_stream(
        &sender,
        &recipient,
        &token_id,
        &deposit,
        &start_time,
        &stop_time,
    );

    (env, client, stream_id, token_id, sender, recipient, deposit)
}

fuzz_target!(|data: (u64, u64, i128)| {
    let (stop_time_raw, now_raw, deposit_raw) = data;

    // Keep the deposit positive and bounded so the token math stays sane.
    let deposit: i128 = (deposit_raw.unsigned_abs() as i128).max(1);

    // Ensure stop_time is strictly after the start time so the stream is valid.
    let start_time: u64 = 1_000;
    let stop_time: u64 = start_time.saturating_add(stop_time_raw % 1_000_000).max(start_time + 1);

    let (env, client, stream_id, token_id, _sender, recipient, deposit) =
        setup(stop_time, deposit);

    // Advance the ledger to an arbitrary point relative to the stream window.
    let now = start_time.saturating_add(now_raw % (stop_time.saturating_sub(start_time) + 2));
    env.ledger().set_timestamp(now);

    // Capture the pre-settle claimable amount.
    let pre_claimable = client.get_claimable(&stream_id);

    // Record the recipient balance before settling to detect double counting.
    let token_client = token::Client::new(&env, &token_id);
    let balance_before = token_client.balance(&recipient);

    // Settle the stream.
    client.settle_stream(&stream_id);

    // Property: withdrawn == pre-claimable.
    let withdrawn = client.get_withdrawn(&stream_id);
    assert_eq!(
        withdrawn, pre_claimable,
        "withdrawn must equal pre-settle claimable"
    );

    // Property: status transitions to Exhausted.
    let status = client.get_status(&stream_id);
    assert_eq!(
        status,
        StreamStatus::Exhausted,
        "status must be Exhausted after settle"
    );

    // No double counting: the recipient balance increased by exactly the
    // pre-settle claimable amount.
    let balance_after = token_client.balance(&recipient);
    assert_eq!(
        balance_after - balance_before,
        pre_claimable,
        "recipient must receive exactly the pre-settle claimable amount"
    );

    // Edge: claimable == 0 must be handled gracefully (settle succeeds and
    // withdraws nothing).
    if pre_claimable == 0 {
        assert_eq!(withdrawn, 0, "zero claimable must withdraw zero");
        assert_eq!(
            balance_after, balance_before,
            "zero claimable must not move tokens"
        );
    }

    // Settling again must not double count (idempotent / rejected).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.settle_stream(&stream_id);
    }));
    if result.is_ok() {
        // If a second settle is permitted, it must not move any more tokens.
        let balance_after_second = token_client.balance(&recipient);
        assert_eq!(
            balance_after_second, balance_after,
            "second settle must not double count"
        );
        assert_eq!(
            client.get_withdrawn(&stream_id),
            pre_claimable,
            "withdrawn must remain stable after second settle"
        );
    }
});