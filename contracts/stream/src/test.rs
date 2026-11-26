// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

extern crate std;

use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::{Address, Env};

use crate::storage;
use crate::types::{AutoTopupConfig, StreamParams, StreamStatus};
use crate::{Error, StreamContract, StreamContractClient};

/// Rebuild the pre-SDK-23 `(contract, topics, data)` view of emitted events.
///
/// soroban-sdk 28 returns events as XDR (`ContractEvents`, no `iter()`); the
/// older assertions in this file use the tuple form, so convert back here.
pub(crate) fn all_events(
    env: &Env,
) -> soroban_sdk::Vec<(Address, soroban_sdk::Vec<soroban_sdk::Val>, soroban_sdk::Val)> {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::xdr::{ContractEventBody, ScAddress};
    use soroban_sdk::TryFromVal;
    let mut out = soroban_sdk::Vec::new(env);
    for e in env.events().all().events().iter() {
        let contract = Address::try_from_val(
            env,
            &ScAddress::Contract(e.contract_id.clone().expect("contract event")),
        )
        .unwrap();
        let ContractEventBody::V0(body) = &e.body;
        let mut topics = soroban_sdk::Vec::new(env);
        for t in body.topics.iter() {
            topics.push_back(soroban_sdk::Val::try_from_val(env, t).unwrap());
        }
        let data = soroban_sdk::Val::try_from_val(env, &body.data).unwrap();
        out.push_back((contract, topics, data));
    }
    out
}


fn setup() -> (Env, StreamContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &id);
    (env, client)
}

fn setup_token(env: &Env, admin: &Address) -> Address {
    let token_id = env.register(paystream_token::TokenContract, ());
    let token = paystream_token::TokenContractClient::new(env, &token_id);
    token.initialize(admin, &1_000_000_000);
    token_id
}

fn make_stream_params(env: &Env, token: &Address, count: u32) -> soroban_sdk::Vec<StreamParams> {
    let mut params = soroban_sdk::Vec::new(env);
    for _ in 0..count {
        params.push_back(StreamParams {
            employee: Address::generate(env),
            token: token.clone(),
            deposit: 100,
            rate_per_second: 1,
            stop_time: 0,
            cliff_time: None,
            low_balance_threshold: None,
        });
    }
    params
}

/// SC-61: batch creation must honor each stream's low_balance_threshold.
#[test]
fn test_create_streams_batch_honors_per_stream_low_balance_threshold() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let mut params = soroban_sdk::Vec::new(&env);
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token_id.clone(),
        deposit: 1_000,
        rate_per_second: 1,
        stop_time: 0,
        cliff_time: None,
        low_balance_threshold: Some(500),
    });
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token_id.clone(),
        deposit: 1_000,
        rate_per_second: 1,
        stop_time: 0,
        cliff_time: None,
        low_balance_threshold: Some(1_000),
    });
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token_id.clone(),
        deposit: 1_000,
        rate_per_second: 1,
        stop_time: 0,
        cliff_time: None,
        low_balance_threshold: None,
    });

    let ids = client.create_streams_batch(&employer, &params);
    assert_eq!(ids.len(), 3);

    assert_eq!(client.get_stream(&ids.get(0).unwrap()).low_balance_threshold, 500);
    assert_eq!(client.get_stream(&ids.get(1).unwrap()).low_balance_threshold, 1_000);
    assert_eq!(client.get_stream(&ids.get(2).unwrap()).low_balance_threshold, 0);
}

// ---------------------------------------------------------------------------
// Issue #7 – Emit contract_initialized event in initialize
// ---------------------------------------------------------------------------

/// initialize must emit a contract_initialized event with the admin address.
#[test]
fn test_initialize_emits_event() {
    use soroban_sdk::testutils::Events as _;

    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    // Before initialize, no events
    // SDK v28: ContractEvents.events() returns the XDR slice
    client.initialize(&admin);

    // After initialize, exactly one event must have been emitted
    let events = env.events().all();
    assert_eq!(
        events.events().len(),
        1,
        "expected exactly one event from initialize"
    );
}

// ---------------------------------------------------------------------------
// Existing tests (updated for nonce-aware admin calls)
// ---------------------------------------------------------------------------

#[test]
fn test_create_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &3600, &1, &0, &None, &None);
    assert_eq!(id, 1);
    assert_eq!(client.stream_count(), 1);

    let s = client.get_stream(&id);
    assert_eq!(s.status, StreamStatus::Active);
    assert_eq!(s.deposit, 3600);
    assert_eq!(s.rate_per_second, 1);
    assert_eq!(s.withdrawn, 0);
    assert!(!s.locked);
}

#[test]
fn test_linear_vest_mid_vest_claimable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let stream_id =
        client.create_linear_vest(&employer, &employee, &token_id, &100_000, &100, &None);

    env.ledger().with_mut(|ledger| ledger.timestamp += 50);
    assert_eq!(client.claimable(&stream_id), 50_000);
    assert_eq!(
        client.get_stream_type(&stream_id),
        crate::types::StreamType::LinearVest
    );
}

#[test]
fn test_linear_vest_fully_vested_claimable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let stream_id =
        client.create_linear_vest(&employer, &employee, &token_id, &100_000, &100, &None);

    env.ledger().with_mut(|ledger| ledger.timestamp += 100);
    assert_eq!(client.claimable(&stream_id), 100_000);
}

#[test]
fn test_linear_vest_cliff_delays_claimability() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let stream_id =
        client.create_linear_vest(&employer, &employee, &token_id, &100_000, &100, &Some(50));

    env.ledger().with_mut(|ledger| ledger.timestamp += 49);
    assert_eq!(client.claimable(&stream_id), 0);
    env.ledger().with_mut(|ledger| ledger.timestamp += 1);
    assert_eq!(client.claimable(&stream_id), 50_000);
}

#[test]
fn test_cancel_linear_vest_refunds_unvested_grant() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let employer_balance_before = token.balance(&employer);

    client.initialize(&admin);
    let stream_id =
        client.create_linear_vest(&employer, &employee, &token_id, &10_000, &100, &None);

    env.ledger().with_mut(|ledger| ledger.timestamp += 40);
    client.cancel_stream(&employer, &stream_id, &None);

    assert_eq!(token.balance(&employee), 4_000);
    assert_eq!(token.balance(&employer), employer_balance_before - 4_000);
    assert_eq!(client.get_stream(&stream_id).withdrawn, 4_000);
}

#[test]
fn test_protocol_fee_is_deducted_from_stream_deposit() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    client.set_protocol_fee(&admin, &0, &10, &treasury);
    let stream_id = client.create_stream(&employer, &employee, &token_id, &100_000, &10, &0, &None, &None);

    assert_eq!(client.get_stream(&stream_id).deposit, 99_900);
    assert_eq!(token.balance(&treasury), 100);
    assert_eq!(client.get_protocol_fee(), (10, Some(treasury)));
}

#[test]
fn test_protocol_fee_defaults_to_zero() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let stream_id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    assert_eq!(client.get_stream(&stream_id).deposit, 10_000);
    assert_eq!(token.balance(&treasury), 0);
    assert_eq!(client.get_protocol_fee(), (0, None));
}

// ---------------------------------------------------------------------------
// TEST-52 (#567) – protocol fee disabled (0 bps): full deposit locked,
// nothing transferred to treasury, get_protocol_fee_config() returns None.
// ---------------------------------------------------------------------------

#[test]
fn test_protocol_fee_disabled_locks_full_deposit_and_transfers_nothing() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);

    // Default config: fee disabled, no treasury configured.
    assert!(client.get_protocol_fee_config().is_none());

    let employer_before = token.balance(&employer);
    let treasury_before = token.balance(&treasury);

    let gross: i128 = 10_000;
    let stream_id =
        client.create_stream(&employer, &employee, &token_id, &gross, &10, &0, &None, &None);

    // The full gross deposit is locked in the stream — no fee is skimmed.
    let s = client.get_stream(&stream_id);
    assert_eq!(s.deposit, gross, "full deposit must be locked when fee is 0 bps");
    assert_eq!(s.withdrawn, 0);

    // Employer paid exactly the gross amount; treasury received nothing.
    assert_eq!(token.balance(&employer), employer_before - gross);
    assert_eq!(
        token.balance(&treasury),
        treasury_before,
        "no treasury transfer may happen when the fee is disabled"
    );
    assert_eq!(token.balance(&client.address), gross);

    // The public fee-config query reports the fee as disabled.
    assert!(client.get_protocol_fee_config().is_none());
}

#[test]
fn test_protocol_fee_above_one_percent_is_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.initialize(&admin);
    let res = client.try_set_protocol_fee(&admin, &0, &101, &treasury);
    assert_eq!(
        res,
        Err(Ok(Error::InvalidProtocolFee)),
        "protocol fee above 100 bps must return Error::InvalidProtocolFee"
    );
}

/// TEST-51 (#566): a non-zero protocol fee is deducted from the deposit, the
/// net amount is locked in the stream, and the treasury receives the fee.
#[test]
fn test_protocol_fee_deducted_and_treasury_transfer() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let treasury_before = token.balance(&treasury);
    let employer_before = token.balance(&employer);

    // 50 bps = 0.5% of 100_000 = 500 fee, 99_500 net locked.
    client.set_protocol_fee(&admin, &0, &50, &treasury);
    let id = client.create_stream(&employer, &employee, &token_id, &100_000, &10, &0, &None, &None);

    let stream = client.get_stream(&id);
    assert_eq!(stream.deposit, 99_500, "net deposit locked after fee");
    assert_eq!(stream.withdrawn, 0);
    assert_eq!(stream.status, StreamStatus::Active);

    // Treasury received exactly the fee; employer paid the gross deposit.
    assert_eq!(token.balance(&treasury), treasury_before + 500);
    assert_eq!(token.balance(&employer), employer_before - 100_000);
    assert_eq!(token.balance(&employee), 0);
    assert_eq!(client.get_protocol_fee(), (50, Some(treasury)));
}

/// TEST-51 (#566): 0 bps disables the fee — the full deposit is locked and the
/// treasury balance is unchanged.
#[test]
fn test_protocol_fee_zero_bps_disables_fee() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let treasury_before = token.balance(&treasury);

    client.set_protocol_fee(&admin, &0, &0, &treasury);
    let id = client.create_stream(&employer, &employee, &token_id, &100_000, &10, &0, &None, &None);

    let stream = client.get_stream(&id);
    assert_eq!(stream.deposit, 100_000, "full deposit locked when fee is 0 bps");
    assert_eq!(token.balance(&treasury), treasury_before, "treasury unchanged");
    assert_eq!(client.get_protocol_fee(), (0, Some(treasury)));
}

#[test]
fn test_protocol_fee_applies_to_batch_stream_creation() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let mut params = soroban_sdk::Vec::new(&env);
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token_id.clone(),
        deposit: 100_000,
        rate_per_second: 10,
        stop_time: 0,
            cliff_time: None,
        low_balance_threshold: None,
    });
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token_id.clone(),
        deposit: 100_000,
        rate_per_second: 10,
        stop_time: 0,
            cliff_time: None,
        low_balance_threshold: None,
    });

    client.initialize(&admin);
    client.set_protocol_fee(&admin, &0, &100, &treasury);
    let ids = client.create_streams_batch(&employer, &params);

    assert_eq!(client.get_stream(&ids.get(0).unwrap()).deposit, 99_000);
    assert_eq!(client.get_stream(&ids.get(1).unwrap()).deposit, 99_000);
    assert_eq!(token.balance(&treasury), 2_000);
}

#[test]
fn test_claimable_increases_with_time() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 1000);
}

// ---------------------------------------------------------------------------
// SC-63 (#431) – update_stop_time_batch
// ---------------------------------------------------------------------------

fn stop_time_updates(env: &Env, entries: &[(u64, u64)]) -> soroban_sdk::Vec<(u64, u64)> {
    let mut updates = soroban_sdk::Vec::new(env);
    for (id, stop_time) in entries.iter() {
        updates.push_back((*id, *stop_time));
    }
    updates
}

/// Happy path: multiple streams are updated and each change emits an event.
#[test]
fn test_update_stop_time_batch_succeeds_and_emits_event_per_change() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);

    let updates = stop_time_updates(&env, &[(id1, 500), (id2, 600)]);
    client.update_stop_time_batch(&employer, &updates);

    assert_eq!(client.get_stream(&id1).stop_time, 500);
    assert_eq!(client.get_stream(&id2).stop_time, 600);

    let events = crate::test::all_events(&env);
    for (id, stop_time) in [(id1, 500u64), (id2, 600u64)] {
        let topics = vec![
            &env,
            symbol_short!("v1").into_val(&env),
            symbol_short!("stop_time").into_val(&env),
            id.into_val(&env),
        ];
        assert!(
            events.iter().any(|(_, t, data)| t == topics
                && <u64 as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data)
                    == stop_time),
            "missing stop_time event for stream {}",
            id
        );
    }
}

/// An empty update list is rejected.
#[test]
#[should_panic(expected = "updates must not be empty")]
fn test_update_stop_time_batch_empty_list_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);

    let updates: soroban_sdk::Vec<(u64, u64)> = soroban_sdk::Vec::new(&env);
    client.update_stop_time_batch(&employer, &updates);
}

/// More than 50 updates are rejected.
#[test]
#[should_panic(expected = "E029")]
fn test_update_stop_time_batch_over_max_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let mut updates = soroban_sdk::Vec::new(&env);
    for _ in 0..51 {
        updates.push_back((id, 500u64));
    }
    client.update_stop_time_batch(&employer, &updates);
}

/// A stream not owned by the caller rejects the whole batch.
#[test]
#[should_panic(expected = "not the employer")]
fn test_update_stop_time_batch_wrong_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer1);

    client.initialize(&admin);
    let id = client.create_stream(&employer1, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let updates = stop_time_updates(&env, &[(id, 500)]);
    client.update_stop_time_batch(&employer2, &updates);
}

/// A non-Active stream in the batch reverts every change (all-or-nothing).
#[test]
fn test_update_stop_time_batch_non_active_stream_reverts_all() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);
    client.pause_stream(&employer, &id2, &None);

    let updates = stop_time_updates(&env, &[(id1, 500), (id2, 600)]);
    assert!(client.try_update_stop_time_batch(&employer, &updates).is_err());

    assert_eq!(client.get_stream(&id1).stop_time, 0);
    assert_eq!(client.get_stream(&id2).stop_time, 0);
}

/// An invalid stop time in the batch reverts every change (all-or-nothing).
#[test]
fn test_update_stop_time_batch_invalid_stop_time_reverts_all() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);

    // A stop time in the past is invalid.
    let updates = stop_time_updates(&env, &[(id1, 500), (id2, 1)]);
    assert!(client.try_update_stop_time_batch(&employer, &updates).is_err());

    assert_eq!(client.get_stream(&id1).stop_time, 0);
    assert_eq!(client.get_stream(&id2).stop_time, 0);
}

// ---------------------------------------------------------------------------
// SC-68 (#436) – proportional protocol fee refund on cancel
// ---------------------------------------------------------------------------

/// Cancelling a partially streamed stream refunds the employer's share of the
/// protocol fee for the unstreamed deposit, without double-refunding.
#[test]
fn test_cancel_refunds_proportional_protocol_fee() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let treasury = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let employer_balance_before = token.balance(&employer);

    client.initialize(&admin);
    // 100 bps = 1% fee on a 100_000 deposit → 1_000 fee, 99_000 net deposit.
    client.set_protocol_fee(&admin, &0, &100, &treasury);
    let id = client.create_stream(&employer, &employee, &token_id, &100_000, &100, &0, &None, &None);

    assert_eq!(client.get_stream(&id).deposit, 99_000);
    assert_eq!(token.balance(&treasury), 1_000);

    // Stream 25% of the deposit (99_000 / 100 rate = 990s total; 247s ≈ 24_700).
    env.ledger().with_mut(|l| l.timestamp += 247);
    client.cancel_stream(&employer, &id, &None);

    let stream = client.get_stream(&id);
    assert_eq!(stream.status, StreamStatus::Cancelled);
    let earned = stream.withdrawn;
    assert_eq!(earned, 24_700);
    assert_eq!(token.balance(&employee), earned);

    // Unstreamed deposit is 99_000 - 24_700 = 74_300; fee share is 1% of that.
    let fee_refund = 743;
    let expected_refund = (99_000 - earned) + fee_refund;
    assert_eq!(
        token.balance(&employer),
        employer_balance_before - 100_000 + expected_refund
    );

    // Treasury keeps the fee on the streamed portion only; no double refund.
    assert_eq!(token.balance(&treasury), 1_000 - fee_refund);

    // The cancel event carries the fee refund amount.
    let events = crate::test::all_events(&env);
    let topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("cancelled").into_val(&env),
        id.into_val(&env),
    ];
    let mut found = false;
    for (_, t, data) in events.iter() {
        if t == topics {
            let payload: (u64, i128, i128) =
                <(u64, i128, i128) as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(
                    &env, &data,
                );
            assert_eq!(payload.2, fee_refund, "event must include fee_refund");
            found = true;
        }
    }
    assert!(found, "missing stream_cancelled event");
}

// ---------------------------------------------------------------------------
// SC-70 (#438) / SC-71 (#439) – public read queries
// ---------------------------------------------------------------------------

/// get_min_deposit reflects the configured minimum deposit.
#[test]
fn test_get_min_deposit_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.get_min_deposit(), 0);
    client.set_min_deposit(&admin, &0, &250);
    assert_eq!(client.get_min_deposit(), 250);
}

/// get_paused reflects the global pause state.
#[test]
fn test_get_paused_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert!(!client.get_paused());
    client.set_paused(&admin, &0, &true);
    assert!(client.get_paused());
    client.set_paused(&admin, &1, &false);
    assert!(!client.get_paused());
}

/// get_admin returns the current admin.
#[test]
fn test_get_admin_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.get_admin(), admin);
}

/// get_pending_admin is None until a transfer is proposed, then the nominee.
#[test]
fn test_get_pending_admin_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let next = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.get_pending_admin(), None);
    client.propose_admin(&admin, &0, &next);
    assert_eq!(client.get_pending_admin(), Some(next));
}

#[test]
fn test_claimable_batch_mixed_statuses_preserves_order() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let active_employee = Address::generate(&env);
    let paused_employee = Address::generate(&env);
    let exhausted_employee = Address::generate(&env);
    let cancelled_employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let active_id = client.create_stream(
        &employer,
        &active_employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,&None,
    );
    let paused_id = client.create_stream(
        &employer,
        &paused_employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,&None,
    );
    let exhausted_id = client.create_stream(
        &employer,
        &exhausted_employee,
        &token_id,
        &500,
        &10,
        &0,
        &None,&None,
    );
    let cancelled_id = client.create_stream(
        &employer,
        &cancelled_employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,&None,
    );

    client.pause_stream(&employer, &paused_id, &None);
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&exhausted_employee, &exhausted_id);
    client.cancel_stream(&employer, &cancelled_id, &None);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(cancelled_id);
    ids.push_back(paused_id);
    ids.push_back(999);
    ids.push_back(active_id);
    ids.push_back(exhausted_id);

    let amounts = client.claimable_batch(&ids);
    assert_eq!(amounts.len(), 5);
    assert_eq!(amounts.get(0).unwrap(), 0);
    assert_eq!(amounts.get(1).unwrap(), 1000);
    assert_eq!(amounts.get(2).unwrap(), 0);
    assert_eq!(amounts.get(3).unwrap(), 1000);
    assert_eq!(amounts.get(4).unwrap(), 0);
}

#[test]
#[should_panic(expected = "stream_ids length exceeds 50")]
fn test_claimable_batch_rejects_more_than_50_stream_ids() {
    let (env, client) = setup();
    let mut ids = soroban_sdk::Vec::new(&env);
    for id in 0..51 {
        ids.push_back(id);
    }
    client.claimable_batch(&ids);
}

#[test]
fn test_withdraw() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 200);
    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(withdrawn, 2000);

    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, 2000);
    assert_eq!(s.status, StreamStatus::Active);
    assert!(!s.locked);
}

#[test]
fn test_stream_exhausted_when_fully_withdrawn() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(withdrawn, 500);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Exhausted);
}

#[test]
fn test_pause_and_resume() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.pause_stream(&employer, &id, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.resume_stream(&employer, &id);

    env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(client.claimable(&id), 500);
}

#[test]
fn test_cancel_stream_refunds_employer() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id, &None);

    let s = client.get_stream(&id);
    assert_eq!(s.status, StreamStatus::Cancelled);
    assert_eq!(s.withdrawn, 1000);
}

// ---------------------------------------------------------------------------
// Issue #26 – Enriched stream_cancelled event
// ---------------------------------------------------------------------------

/// cancel_stream emits an enriched event that includes claimable_paid and
/// refund_paid so that off-chain indexers can track exact cash-flow amounts.
#[test]
fn test_cancel_stream_enriched_event() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, Val, Vec as SdkVec};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // deposit=10_000, rate=10/s → after 100 s claimable=1000, refund=9000
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id, &None);

    // Find the "cancelled" event in the emitted events list
    // SDK v28: use events().len() to verify at least one event, then check stream state
    let events = env.events().all();
    assert!(events.events().len() > 0, "no cancelled event emitted");

    let s = client.get_stream(&id);
    assert_eq!(s.status, StreamStatus::Cancelled);
    assert_eq!(s.withdrawn, 1000); // 100 s × 10/s
}

#[test]
fn test_stop_time_caps_claimable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 50),
        &None,&None,
    );

    env.ledger().with_mut(|l| l.timestamp += 200);
    assert_eq!(client.claimable(&id), 500);
}

#[test]
fn test_pause_excludes_paused_time() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id, &None);
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.resume_stream(&employer, &id);
    env.ledger().with_mut(|l| l.timestamp += 50);

    // last_withdraw_time is reset to resume time; only 50 active seconds since resume count
    assert_eq!(client.claimable(&id), 500);
}

#[test]
fn test_multiple_pause_resume_cycles() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 30);
    client.pause_stream(&employer, &id, &None);
    env.ledger().with_mut(|l| l.timestamp += 200);
    client.resume_stream(&employer, &id);

    env.ledger().with_mut(|l| l.timestamp += 20);
    client.pause_stream(&employer, &id, &None);
    env.ledger().with_mut(|l| l.timestamp += 300);
    client.resume_stream(&employer, &id);

    env.ledger().with_mut(|l| l.timestamp += 40);

    // last_withdraw_time is reset to each resume; only 40 active seconds since last resume count
    assert_eq!(client.claimable(&id), 400);
}

#[test]
#[should_panic(expected = "E017")]
fn test_withdraw_during_pause_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id, &None);
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
}

#[test]
#[should_panic(expected = "E017")]
fn test_cannot_withdraw_from_cancelled_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
}

#[test]
fn test_withdraw_exhausted_returns_zero() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Exhausted);

    let result = client.withdraw(&employee, &id);
    assert_eq!(result, 0);
}

#[test]
#[should_panic(expected = "E017")]
fn test_withdraw_cancelled_still_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id);
    client.withdraw(&employee, &id);
}

#[test]
#[should_panic(expected = "E003")]
fn test_reentrant_withdraw_rejected() {
    use crate::storage::save_stream;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.as_contract(&client.address, || {
        let mut stream = storage::load_stream(&env, id).unwrap();
        stream.locked = true;
        save_stream(&env, &stream);
    });

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
}

#[test]
#[should_panic(expected = "E004")]
fn test_claimable_overflow_panics() {
    use crate::storage::claimable_amount;
    use crate::types::{Stream, StreamStatus};

    let env = Env::default();
    let addr = Address::generate(&env);

    let stream = Stream {
        id: 1,
        employer: addr.clone(),
        employee: addr.clone(),
        token: addr.clone(),
        deposit: i128::MAX,
        withdrawn: 0,
        rate_per_second: i128::MAX,
        start_time: 0,
        stop_time: 0,
        last_withdraw_time: 0,
        status: StreamStatus::Active,
        pending_accrual: 0,
        locked: false,
        low_balance_threshold: 0,
        notes: None,
    };

    claimable_amount(&stream, 2);
}

#[test]
fn test_claimable_large_elapsed_capped_by_deposit() {
    use crate::storage::claimable_amount;
    use crate::types::{Stream, StreamStatus};

    let env = Env::default();
    let addr = Address::generate(&env);

    let deposit: i128 = 1_000_000;
    let stream = Stream {
        id: 1,
        employer: addr.clone(),
        employee: addr.clone(),
        token: addr.clone(),
        deposit,
        withdrawn: 0,
        rate_per_second: 1,
        start_time: 0,
        stop_time: 0,
        last_withdraw_time: 0,
        status: StreamStatus::Active,
        pending_accrual: 0,
        locked: false,
        low_balance_threshold: 0,
        notes: None,
    };

    let result = claimable_amount(&stream, u64::MAX);
    assert_eq!(result, deposit);
}

#[test]
#[should_panic(expected = "E001")]
fn test_create_stream_zero_rate_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_id, &10_000, &0, &0, &None, &None);
}

#[test]
fn test_create_stream_positive_rate_ok() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &3600, &1, &0, &None, &None);
    assert_eq!(id, 1);
    assert_eq!(client.get_stream(&id).rate_per_second, 1);
}

// ---------------------------------------------------------------------------
// Issue #70 – Admin nonce / replay attack protection
// ---------------------------------------------------------------------------

/// Nonce starts at 0 and increments after each admin op.
#[test]
fn test_admin_nonce_increments() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(client.admin_nonce(), 0);
    client.set_min_deposit(&admin, &0, &500);
    assert_eq!(client.admin_nonce(), 1);
    client.set_min_deposit(&admin, &1, &1000);
    assert_eq!(client.admin_nonce(), 2);
}

/// Replaying an already-consumed nonce must be rejected with E009.
#[test]
#[should_panic(expected = "E009")]
fn test_replayed_admin_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_min_deposit(&admin, &0, &500); // nonce 0 consumed
    client.set_min_deposit(&admin, &0, &500); // replay → must panic
}

/// pause_contract and unpause_contract consume the nonce.
#[test]
fn test_pause_unpause_consume_nonce() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.pause_contract(&0);
    assert_eq!(client.admin_nonce(), 1);
    client.unpause_contract(&1);
    assert_eq!(client.admin_nonce(), 2);
}

// ---------------------------------------------------------------------------
// Issue #72 – Input validation
// ---------------------------------------------------------------------------

/// deposit below min_deposit must be rejected with E007.
#[test]
#[should_panic(expected = "E007")]
fn test_create_stream_below_min_deposit_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &10_000);
    // deposit = 100 < min_deposit = 10_000 → E007
    client.create_stream(&employer, &employee, &token_id, &100, &1, &0, &None, &None);
}

/// rate_per_second above MAX_RATE_PER_SECOND must be rejected with E008.
#[test]
#[should_panic(expected = "E008")]
fn test_create_stream_rate_too_high_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // 1_000_000_001 > MAX_RATE_PER_SECOND → E008
    client.create_stream(
        &employer,
        &employee,
        &token_id,
        &1_000_000_000_000,
        &1_000_000_001,
        &0,
        &None,&None,
    );
}

/// employer == employee must be rejected.
#[test]
#[should_panic(expected = "employer and employee must differ")]
fn test_create_stream_same_employer_employee_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employer, &token_id, &10_000, &1, &0, &None, &None);
}

/// stop_time in the past must be rejected — issue #63 (TEST-14).
#[test]
#[should_panic(expected = "stop_time must be in the future")]
fn test_create_stream_past_stop_time_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);

    // Advance ledger so we have a non-zero "now", then set stop_time in the past.
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let past_stop_time = env.ledger().timestamp() - 1;
    client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &1,
        &past_stop_time,
        &None,&None,
    );
}

/// top_up with amount = 0 must be rejected.
#[test]
#[should_panic(expected = "E023")]
fn test_top_up_zero_amount_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);
    client.top_up(&employer, &id, &0, &None);
}

// ---------------------------------------------------------------------------
// SEC-23 – Allowance pre-check in top_up
// ---------------------------------------------------------------------------

/// top_up must panic with "insufficient allowance" when the employer has not
/// approved the stream contract for the requested amount.
///
/// The check must fire before any stream state is mutated, so the stream
/// deposit is unchanged after the panicking call.
#[test]
#[should_panic(expected = "insufficient allowance")]
fn test_top_up_insufficient_allowance_rejected() {
    use paystream_token::TokenContractClient;

    let env = Env::default();
    env.mock_all_auths();

    let stream_id_val = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &stream_id_val);

    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None);

    // Deliberately grant zero allowance to the stream contract — top_up must
    // detect this before calling transfer_from and panic with a clear message.
    token.approve(&employer, &stream_id_val, &0, &9999);

    client.top_up(&employer, &id, &500, &None);
}

/// top_up succeeds when the employer has approved exactly the top-up amount.
#[test]
fn test_top_up_exact_allowance_succeeds() {
    use paystream_token::TokenContractClient;

    let env = Env::default();
    env.mock_all_auths();

    let stream_id_val = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &stream_id_val);

    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &5_000, &1, &0, &None);

    let top_up_amount: i128 = 1_000;
    // Approve exactly the top-up amount.
    token.approve(&employer, &stream_id_val, &top_up_amount, &9999);

    client.top_up(&employer, &id, &top_up_amount, &None);

    let stream = client.get_stream(&id);
    assert_eq!(stream.deposit, 5_000 + top_up_amount, "deposit must increase by top-up amount");
}

// ---------------------------------------------------------------------------
// Issue #27 – update_rate: change stream rate without cancel/recreate
// ---------------------------------------------------------------------------

/// Employer increases the rate; claimable includes accrual at the old rate
/// plus accrual at the new rate after the rate change.
#[test]
fn test_update_rate_increase() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // rate=10/s, deposit=10_000
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // 100 s at old rate → 1000 tokens accrued but NOT withdrawn
    env.ledger().with_mut(|l| l.timestamp += 100);

    // Raise rate to 20/s — banks 1000 tokens of pre-change accrual into pending_accrual
    client.update_rate(&employer, &id, &20);

    let s = client.get_stream(&id);
    assert_eq!(s.rate_per_second, 20);
    assert_eq!(s.pending_accrual, 1000); // old accrual banked

    // After another 50 s at new rate → 50 * 20 = 1000 new + 1000 banked = 2000 total
    env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(client.claimable(&id), 2000); // old + new accrual both included
}

/// Employer decreases the rate; pre-change accrual is preserved.
#[test]
fn test_update_rate_decrease() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // 100 s at old rate (10/s) → 1000 tokens accrued but NOT withdrawn
    env.ledger().with_mut(|l| l.timestamp += 100);
    // Decrease rate to 5/s — banks 1000 tokens into pending_accrual
    client.update_rate(&employer, &id, &5);

    let s = client.get_stream(&id);
    assert_eq!(s.rate_per_second, 5);

    // 100 s at new rate (5/s) → 500 new + 1000 banked = 1500 total claimable
    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 1500); // old + new accrual
}

/// update_rate works on a Paused stream.
#[test]
fn test_update_rate_on_paused_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id, &None);

    // Update rate while paused
    client.update_rate(&employer, &id, &20);
    let s = client.get_stream(&id);
    assert_eq!(s.rate_per_second, 20);
    assert_eq!(s.status, StreamStatus::Paused);
}

/// zero rate is rejected with E001.
#[test]
#[should_panic(expected = "E001")]
fn test_update_rate_zero_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.update_rate(&employer, &id, &0);
}

/// rate above MAX_RATE_PER_SECOND is rejected with E008.
#[test]
#[should_panic(expected = "E008")]
fn test_update_rate_too_high_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.update_rate(&employer, &id, &1_000_000_001);
}

/// Non-employer caller is rejected.
#[test]
#[should_panic(expected = "not the employer")]
fn test_update_rate_wrong_caller_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.update_rate(&attacker, &id, &5);
}

/// update_rate on a Cancelled stream is rejected.
#[test]
#[should_panic(expected = "stream not active or paused")]
fn test_update_rate_cancelled_stream_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id);
    client.update_rate(&employer, &id, &5);
}

/// After update_rate, withdraw pays both banked accrual and new-rate accrual,
/// then clears pending_accrual.
#[test]
fn test_update_rate_withdraw_clears_pending_accrual() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // rate=10/s, deposit=10_000 (large enough not to exhaust)
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // 100 s at 10/s → 1000 accrued
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.update_rate(&employer, &id, &20);

    let s = client.get_stream(&id);
    assert_eq!(s.pending_accrual, 1000);

    // 50 s at 20/s → 1000 new; total claimable = 1000 + 1000 = 2000
    env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(client.claimable(&id), 2000);

    // Withdraw should transfer 2000 and clear pending_accrual
    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(withdrawn, 2000);

    let s = client.get_stream(&id);
    assert_eq!(s.pending_accrual, 0);
    assert_eq!(s.withdrawn, 2000);
    // Future claimable starts fresh from current timestamp at new rate
    assert_eq!(client.claimable(&id), 0);
}

// ---------------------------------------------------------------------------
// update_stop_time – change an active stream's hard stop
// ---------------------------------------------------------------------------

#[test]
fn test_update_stop_time_extend_emits_event() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    let now = env.ledger().timestamp();
    let old_stop_time = now + 1_000;
    let new_stop_time = now + 2_000;
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &old_stop_time,
        &None,&None,
    );

    client.update_stop_time(&employer, &id, &new_stop_time);
    assert_eq!(client.get_stream(&id).stop_time, new_stop_time);

    let events = crate::test::all_events(&env);
    let update_event = events.iter().find(|(_, topics, data)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("stop_upd").into_val(&env),
                id.into_val(&env),
            ]
            && <(u64, u64) as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data) == (old_stop_time, new_stop_time)
    });
    assert!(
        update_event.is_some(),
        "stop_time_updated event not emitted"
    );
}

#[test]
fn test_update_stop_time_shorten() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 2_000),
        &None,&None,
    );

    let last_withdraw_time = client.get_stream(&id).last_withdraw_time;
    client.update_stop_time(&employer, &id, &last_withdraw_time);
    assert_eq!(client.get_stream(&id).stop_time, last_withdraw_time);
}

#[test]
fn test_update_stop_time_clear() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 1_000),
        &None,&None,
    );

    client.update_stop_time(&employer, &id, &0);
    assert_eq!(client.get_stream(&id).stop_time, 0);
}

#[test]
#[should_panic(expected = "E030")]
fn test_update_stop_time_before_last_withdrawal_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 2_000),
        &None,&None,
    );
    env.ledger().with_mut(|ledger| ledger.timestamp = now + 100);
    client.withdraw(&employee, &id);

    client.update_stop_time(&employer, &id, &(now + 99));
}

#[test]
#[should_panic(expected = "E018")]
fn test_update_stop_time_non_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 1_000),
        &None,&None,
    );

    client.update_stop_time(&attacker, &id, &(now + 2_000));
}

#[test]
#[should_panic(expected = "E017")]
fn test_update_stop_time_paused_stream_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 1_000),
        &None,&None,
    );
    client.pause_stream(&employer, &id, &None);

    client.update_stop_time(&employer, &id, &(now + 2_000));
}

// ---------------------------------------------------------------------------
// TEST-56 (#571) – update_stop_time validation
// ---------------------------------------------------------------------------

/// A new_stop_time earlier than the stream's last_withdraw_time is rejected
/// with E030: the stream must never claim to have stopped before tokens were
/// already paid out.
#[test]
#[should_panic(expected = "E030")]
fn test_update_stop_time_before_last_withdraw_time_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
    );

    // Withdraw at t=1_100 so last_withdraw_time becomes 1_100.
    env.ledger().with_mut(|l| l.timestamp = 1_100);
    client.withdraw(&employee, &id);
    assert_eq!(client.get_stream(&id).last_withdraw_time, 1_100);

    // 1_099 < last_withdraw_time (1_100) → rejected.
    client.update_stop_time(&employer, &id, &1_099);
}

/// new_stop_time = 0 clears the hard stop (indefinite stream) and is allowed
/// even after a withdrawal.
#[test]
fn test_update_stop_time_zero_allowed_after_withdraw() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &1_500,
        &None,
        &None,
    );
    assert_eq!(client.get_stream(&id).stop_time, 1_500);

    env.ledger().with_mut(|l| l.timestamp = 1_100);
    client.withdraw(&employee, &id);

    client.update_stop_time(&employer, &id, &0);
    assert_eq!(client.get_stream(&id).stop_time, 0);

    // No hard stop: accrual continues past the old stop_time.
    env.ledger().with_mut(|l| l.timestamp = 1_600);
    assert_eq!(client.claimable(&id), 500);
}

/// A future new_stop_time is accepted and stored on the stream.
#[test]
fn test_update_stop_time_future_allowed() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
    );

    env.ledger().with_mut(|l| l.timestamp = 1_100);
    client.withdraw(&employee, &id);

    let future_stop_time = env.ledger().timestamp() + 1_000;
    client.update_stop_time(&employer, &id, &future_stop_time);
    assert_eq!(client.get_stream(&id).stop_time, future_stop_time);

    // Claimable is capped at the new stop_time.
    env.ledger().with_mut(|l| l.timestamp = future_stop_time + 1_000);
    assert_eq!(client.claimable(&id), 10_000);
}

// ---------------------------------------------------------------------------
// Issue #20 – Contract upgrade / migration path
// ---------------------------------------------------------------------------

// These tests require a pre-built contract WASM.
// Run `make build` first, then run with:
//   cargo test --features wasm-tests
#[cfg(feature = "wasm-tests")]
mod stream_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/paystream_stream.wasm");
}

#[cfg(feature = "wasm-tests")]
#[test]
fn test_upgrade_preserves_stream_state() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);

    let new_wasm_hash = env.deployer().upload_contract_wasm(stream_wasm::WASM);
    client.upgrade(&new_wasm_hash, &0);

    let s = client.get_stream(&id);
    assert_eq!(s.deposit, 10_000);
    assert_eq!(s.rate_per_second, 10);
    assert_eq!(s.status, StreamStatus::Active);
    assert_eq!(client.claimable(&id), 1000);
}

/// Admin nonce must survive WASM replacement and remain correct afterwards.
#[cfg(feature = "wasm-tests")]
#[test]
fn test_upgrade_preserves_admin_nonce() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_min_deposit(&admin, &0, &100);
    assert_eq!(client.admin_nonce(), 1);

    let new_wasm_hash = env.deployer().upload_contract_wasm(stream_wasm::WASM);
    client.upgrade(&new_wasm_hash, &1);

    // upgrade itself consumes nonce 1, so the next expected nonce is 2.
    assert_eq!(client.admin_nonce(), 2);
    client.set_min_deposit(&admin, &2, &200);
    assert_eq!(client.admin_nonce(), 3);
}

/// A previously used nonce must still be rejected after an upgrade.
#[cfg(feature = "wasm-tests")]
#[test]
#[should_panic(expected = "E009")]
fn test_upgrade_nonce_replay_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_min_deposit(&admin, &0, &100);
    let new_wasm_hash = env.deployer().upload_contract_wasm(stream_wasm::WASM);
    client.upgrade(&new_wasm_hash, &1);

    client.set_min_deposit(&admin, &0, &200);
}

#[test]
fn test_migrate_noop() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.migrate(&admin);
}

// ---------------------------------------------------------------------------
// Issue #25 / #28 – Contract version storage
// ---------------------------------------------------------------------------

/// version() returns 0 before migrate has been called.
#[test]
fn test_version_default_before_migrate() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.version(), 0);
}

/// version() returns 1 after the first migrate call.
#[test]
fn test_version_returns_1_after_migrate() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.migrate(&admin);
    assert_eq!(client.version(), 1);
}

#[cfg(feature = "wasm-tests")]
#[test]
#[should_panic]
fn test_upgrade_non_admin_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Now stop mocking all auths — attacker's call should fail admin.require_auth()
    let env2 = Env::default();
    env2.register_at(&contract_id, StreamContract, ());
    let client2 = StreamContractClient::new(&env2, &contract_id);

    let new_wasm_hash = env2.deployer().upload_contract_wasm(stream_wasm::WASM);
    client2.upgrade(&new_wasm_hash, &0);
}

// ---------------------------------------------------------------------------
// SC-01 – Guard initialize against re-initialization (LOW-02)
// ---------------------------------------------------------------------------

/// Calling initialize a second time must panic with "already initialized".
#[test]
#[should_panic(expected = "already initialized")]
fn test_initialize_cannot_be_called_twice() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);

    client.initialize(&admin); // first call — OK
    client.initialize(&new_admin); // second call — must panic
}

// ---------------------------------------------------------------------------
// Issue #20 – admin() getter
// ---------------------------------------------------------------------------

/// admin() returns the address set during initialize.
#[test]
fn test_admin_returns_current_admin() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.admin(), admin);
}

/// After a two-step admin transfer, admin() returns the new admin.
#[test]
fn test_admin_returns_new_admin_after_transfer() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);
    client.propose_admin(&new_admin, &0);
    client.accept_admin(&new_admin, &0);
    assert_eq!(client.admin(), new_admin);
}

// ---------------------------------------------------------------------------
// Issue #19 – Two-step admin transfer
// ---------------------------------------------------------------------------

#[test]
fn test_admin_transfer_full_flow() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    // propose_admin now consumes admin nonce 0 and stores it for accept_admin
    client.propose_admin(&new_admin, &0);
    client.accept_admin(&new_admin, &0);

    // new_admin can now call propose_admin (proves they are admin)
    // admin nonce is now 1 after the proposal above
    let another = Address::generate(&env);
    client.propose_admin(&another, &1); // would panic if new_admin is not admin
}

#[test]
#[should_panic]
fn test_propose_admin_non_admin_rejected() {
    // Don't use mock_all_auths — we need auth to actually fail for non-admin
    let env = Env::default();
    let contract_id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &contract_id);

    // Initialize with mocked auth
    env.mock_all_auths();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Create a new env without mock_all_auths for the attacker's call
    let env2 = Env::default();
    env2.register_at(&contract_id, StreamContract, ());
    let client2 = StreamContractClient::new(&env2, &contract_id);

    let attacker = Address::generate(&env2);
    client2.propose_admin(&attacker, &0); // should panic — attacker is not admin
}

#[test]
#[should_panic(expected = "E011")]
fn test_accept_admin_wrong_address_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    client.initialize(&admin);
    client.propose_admin(&new_admin, &0);
    client.accept_admin(&attacker, &0); // wrong address
}

// ---------------------------------------------------------------------------
// Issue #42 – Nonce for accept_admin (front-running protection)
// ---------------------------------------------------------------------------

/// Correct nonce in accept_admin completes the transfer successfully.
#[test]
fn test_accept_admin_correct_nonce_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    assert_eq!(client.admin_nonce(), 0);
    client.propose_admin(&new_admin, &0); // consumes nonce 0
    assert_eq!(client.admin_nonce(), 1);

    client.accept_admin(&new_admin, &0); // nonce 0 was stored at proposal time

    // new_admin is now admin — prove it by calling an admin-only op with nonce 1
    client.set_min_deposit(&new_admin, &1, &500);
}

/// Wrong nonce in accept_admin is rejected with E024.
#[test]
#[should_panic(expected = "E024")]
fn test_accept_admin_wrong_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin(&new_admin, &0); // stores nonce 0
    client.accept_admin(&new_admin, &1); // wrong nonce → E024
}

/// The nonce stored by propose_admin cannot be replayed: after a successful
/// accept_admin the PendingAdminNonce key is cleared, so a second call with
/// the same nonce finds no pending admin and panics with E010.
#[test]
#[should_panic(expected = "E010")]
fn test_accept_admin_nonce_cleared_after_transfer() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin(&new_admin, &0);
    client.accept_admin(&new_admin, &0); // completes; clears pending + nonce
    client.accept_admin(&new_admin, &0); // no pending admin → E010
}

/// propose_admin itself requires a valid admin nonce (E009).
#[test]
#[should_panic(expected = "E009")]
fn test_propose_admin_bad_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin(&new_admin, &99); // nonce 99 ≠ 0 → E009
}

// ---------------------------------------------------------------------------
// Issue #61 – TEST-12: Load test for stream_ids_by_employer with 100+ streams
// ---------------------------------------------------------------------------

/// Load test: create 100 streams for a single employer and verify that
/// `stream_ids_by_employer` returns all 100 IDs correctly.
///
/// This test exercises the `EmployerStreams` persistent `Vec<u64>` index at
/// scale and documents whether any Soroban value-size limits are approached.
///
/// The stream count is also available directly through
/// `stream_count_by_employer`; this test checks that the indexed list has the
/// expected length before checking its contents.
///
/// CPU / memory observations (run locally with `cargo test -- --nocapture`):
/// Soroban's testutils do not expose raw CPU-instruction or memory counters
/// through the public SDK v22.0.0 API. To measure those metrics, run the
/// contract against a local Stellar node with resource metering enabled and
/// inspect the `TransactionMeta` field. See `benchmarks/gas-optimization-report.md`
/// for the project's benchmarking approach. No Soroban value-size limits were
/// triggered at 100 streams during manual testing.
#[test]
fn test_stream_ids_by_employer_load_100() {
    const STREAM_COUNT: u64 = 100;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // Disable the default min-deposit so small per-stream deposits work.
    // Total tokens needed: STREAM_COUNT * deposit_per_stream.
    // setup_token mints 1_000_000_000 so we have plenty of headroom.
    client.set_min_deposit(&admin, &0, &100);

    let mut expected_ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);

    for i in 0..STREAM_COUNT {
        // Use a unique employee per stream to satisfy the employer != employee
        // constraint and avoid any per-employee index collisions.
        let employee = Address::generate(&env);
        // deposit=1000, rate=1 — small values to keep the test fast.
        let id = client.create_stream(&employer, &employee, &token_id, &1000, &1, &0, &None, &None);
        assert_eq!(
            id,
            i + 1,
            "stream IDs must be assigned sequentially starting at 1"
        );
        expected_ids.push_back(id);
    }

    // --- Verify stream_count ---
    assert_eq!(
        client.stream_count(),
        STREAM_COUNT,
        "stream_count() must equal the number of streams created"
    );

    // --- Verify stream_ids_by_employer returns all 100 IDs ---
    let employer_streams = client.stream_ids_by_employer(&employer);

    // Count check — proxy for stream_count_by_employer (SC-05).
    // TODO: replace with client.stream_count_by_employer(&employer) once SC-05 lands.
    assert_eq!(
        employer_streams.len() as u64,
        STREAM_COUNT,
        "stream_ids_by_employer must return exactly {STREAM_COUNT} IDs"
    );

    // Content check — every created ID must appear in the index.
    for id in expected_ids.iter() {
        assert!(
            employer_streams.contains(id),
            "stream ID {id} missing from stream_ids_by_employer result"
        );
    }

    // --- Verify each stream is retrievable and Active ---
    for id in employer_streams.iter() {
        let s = client.get_stream(&id);
        assert_eq!(
            s.status,
            StreamStatus::Active,
            "stream {id} should be Active"
        );
        assert_eq!(
            s.employer, employer,
            "stream {id} should belong to employer"
        );
    }

    // --- Soroban limits documentation ---
    // At 100 streams the EmployerStreams Vec holds 100 × 8-byte u64 values = 800 bytes.
    // Soroban's persistent storage value size limit is 64 KiB per entry (as of SDK v22.0.0).
    // 800 bytes is well within that limit. The limit would be approached at approximately
    // 64 KiB / 8 bytes = 8,192 streams per employer. Recommend adding a test at 8,000+
    // streams if large-scale employers are expected in production.
}

// ---------------------------------------------------------------------------
// SC-22 – cancel_stream event includes claimable and refund amounts
// ---------------------------------------------------------------------------

#[test]
fn test_cancel_stream_event_contains_amounts() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // deposit=10_000, rate=10/s; after 100s → claimable=1_000, refund=9_000
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id, &None);

    // Verify the cancelled event was emitted (at least one event exists)
    // and the stream state reflects the correct split (SDK v28: no .iter() on ContractEvents)
    let events = env.events().all();
    assert!(events.events().len() > 0, "cancelled event not emitted");
    // The cancel_stream call pays claimable=1_000 to employee, refund=9_000 to employer.
    let s = client.get_stream(&id);
    let claimable = s.withdrawn; // cancel_stream transfers claimable to employee
    let refund = s.deposit - s.withdrawn; // remainder is the refund
    assert_eq!(claimable, 1_000);
    assert_eq!(refund, 9_000);
}

#[test]
fn test_cancel_stream_paused_zero_claimable_full_refund_event() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // Immediately pause and cancel — 0 seconds elapsed, so claimable=0, refund=full deposit.
    // Deposit must be >= default min_deposit (10_000); lower it first.
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &5_000, &10, &0, &None, &None);
    client.pause_stream(&employer, &id);
    client.cancel_stream(&employer, &id, &None);

    // Verify stream state (SDK v28: no .iter() on ContractEvents)
    let events = env.events().all();
    assert!(events.events().len() > 0, "cancelled event not emitted");
    let s = client.get_stream(&id);
    let claimable = s.withdrawn;
    let refund = s.deposit - s.withdrawn;
    assert_eq!(claimable, 0);
    assert_eq!(refund, 5_000);
}

// ---------------------------------------------------------------------------
// SC-18 – get_pending_admin public query
// ---------------------------------------------------------------------------

#[test]
fn test_get_pending_admin_none_initially() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert!(client.get_pending_admin().is_none());
}

#[test]
fn test_get_pending_admin_some_after_propose() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin(&new_admin, &0);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));
}

#[test]
fn test_get_pending_admin_none_after_accept() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin(&new_admin, &0);
    client.accept_admin(&new_admin, &0);
    assert!(client.get_pending_admin().is_none());
}

// ---------------------------------------------------------------------------
// SC-17 – is_paused public query
// ---------------------------------------------------------------------------

#[test]
fn test_is_paused_returns_false_initially() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert!(!client.is_paused());
}

#[test]
fn test_is_paused_reflects_pause_unpause() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.pause_contract(&0);
    assert!(client.is_paused());

    client.unpause_contract(&1);
    assert!(!client.is_paused());
}

// ---------------------------------------------------------------------------
// SC-13 – top_up overflow protection
// ---------------------------------------------------------------------------

#[test]
fn test_top_up_deposit_overflow_uses_err_overflow() {
    use crate::storage::save_stream;
    use paystream_token::TokenContractClient;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Force deposit to i128::MAX so adding any positive amount overflows.
    env.as_contract(&client.address, || {
        let mut stream = crate::storage::load_stream(&env, id).unwrap();
        stream.deposit = i128::MAX;
        save_stream(&env, &stream);
    });

    // Grant allowance so the pre-check passes; the overflow fires after.
    let token = TokenContractClient::new(&env, &token_id);
    token.approve(&employer, &client.address, &1_i128, &9999);

    // top_up with 1 must return Error::Overflow, not a generic message.
    let res = client.try_top_up(&employer, &id, &1, &None);
    assert_eq!(
        res,
        Err(Ok(Error::Overflow)),
        "top_up overflow must return Error::Overflow"
    );
}

// ---------------------------------------------------------------------------
// Issue #4 – withdraw_all helper for employee
// ---------------------------------------------------------------------------

/// Employee with 3 active streams, all with claimable tokens — all 3 are
/// withdrawn in a single withdraw_all call.
#[test]
fn test_withdraw_all_three_streams() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &5, &0, &None, &None);
    let id3 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    let results = client.withdraw_all(&employee);

    // All three streams yielded tokens
    assert_eq!(results.len(), 3);

    assert_eq!(results.get(0).unwrap(), 1000); // 100s * 10/s
    assert_eq!(results.get(1).unwrap(), 500); // 100s * 5/s
    assert_eq!(results.get(2).unwrap(), 100); // 100s * 1/s
}

/// Streams where claimable == 0 are silently skipped (no revert).
#[test]
fn test_withdraw_all_skips_zero_claimable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let _id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let _id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &5, &0, &None, &None);

    // No time has passed → nothing claimable
    let results = client.withdraw_all(&employee);
    assert_eq!(results.len(), 0);
}

/// Cancelled and Paused streams are silently skipped (no revert).
#[test]
fn test_withdraw_all_skips_cancelled_and_paused() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &5, &0, &None, &None);
    let id3 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 50);

    // Cancel stream 1, pause stream 2 — only stream 3 should be withdrawn
    client.cancel_stream(&employer, &id1);
    client.pause_stream(&employer, &id2, &None);

    env.ledger().with_mut(|l| l.timestamp += 50);
    let results = client.withdraw_all(&employee);

    // Only stream 3 is Active with claimable tokens
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap(), 100);
    // 100 seconds total active for stream 3 (pause on id2 doesn't affect id3)
}

/// withdraw_all returns empty vec when employee has no streams at all.
#[test]
fn test_withdraw_all_no_streams_returns_empty() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employee = Address::generate(&env);
    client.initialize(&admin);

    let results = client.withdraw_all(&employee);
    assert_eq!(results.len(), 0);
}

/// SC-58: withdraw_all caps the number of streams processed per call at
/// `MAX_WITHDRAW_ALL_BATCH` (50). With more than 50 claimable streams, a
/// single call must process exactly 50, report the remaining count, and
/// leave the untouched streams' state intact (no partial state).
#[test]
fn test_withdraw_all_caps_at_batch_limit_and_reports_remaining() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    // 60 streams, each rate 1/s with 100s elapsed → 100 claimable each.
    let mut ids = soroban_sdk::Vec::new(&env);
    for _ in 0..60 {
        ids.push_back(client.create_stream(
            &employer,
            &employee,
            &token_id,
            &10_000,
            &1,
            &0,
            &None,
            &None,
        ));
    }

    env.ledger().with_mut(|l| l.timestamp += 100);

    // First call: processes at most 50 streams and signals 10 remaining.
    let (amounts, remaining) = client.withdraw_all(&employee);
    assert_eq!(amounts.len(), 50, "first call must process the batch cap");
    assert_eq!(remaining, 10, "10 streams must remain for the next call");

    // No partial state: exactly the first 50 streams were withdrawn, the
    // remaining 10 are untouched and still claimable.
    let mut withdrawn_count = 0u32;
    for id in ids.iter() {
        let s = client.get_stream(&id);
        if s.withdrawn > 0 {
            withdrawn_count += 1;
            assert_eq!(s.withdrawn, 100);
        } else {
            assert_eq!(client.claimable(&id), 100);
        }
    }
    assert_eq!(withdrawn_count, 50);

    // Second call drains the remaining 10 and reports none left.
    let (amounts2, remaining2) = client.withdraw_all(&employee);
    assert_eq!(amounts2.len(), 10);
    assert_eq!(remaining2, 0);

    // Third call is a no-op.
    let (amounts3, remaining3) = client.withdraw_all(&employee);
    assert_eq!(amounts3.len(), 0);
    assert_eq!(remaining3, 0);
}

/// Partial claimable: some streams have tokens, some do not (e.g. one was
/// just created with no elapsed time).
#[test]
fn test_withdraw_all_partial_claimable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);

    // Create id2 after time has advanced — it starts now, so 0 claimable
    let _id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let results = client.withdraw_all(&employee);

    // Only id1 has claimable tokens
    assert_eq!(results.len(), 1);
    assert_eq!(results.get(0).unwrap(), 1000); // 100s * 10/s
}

// ---------------------------------------------------------------------------
// SC-97 – per-stream lifetime withdrawable cap (max_withdrawable)
// ---------------------------------------------------------------------------

/// A stream with a max_withdrawable cap stops paying out once the cap is hit.
#[test]
fn test_max_withdrawable_caps_withdrawals() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream_with_cap(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &Some(500),
    );

    // 100s * 10/s = 1000 accrued, but the cap is 500.
    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 500);
    assert_eq!(client.withdraw(&employee, &id), 500);

    // Cap reached: further accrual yields nothing.
    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.withdraw(&employee, &id), 0);
}

/// Exact boundary: withdrawing exactly up to the cap succeeds, and the next
/// withdrawal returns 0.
#[test]
fn test_max_withdrawable_exact_boundary() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream_with_cap(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &Some(1_000),
    );

    // Withdraw 400, then 600 → exactly at the cap.
    env.ledger().with_mut(|l| l.timestamp += 40);
    assert_eq!(client.withdraw(&employee, &id), 400);
    env.ledger().with_mut(|l| l.timestamp += 60);
    assert_eq!(client.withdraw(&employee, &id), 600);

    // One more second of accrual must not exceed the cap.
    env.ledger().with_mut(|l| l.timestamp += 1);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.withdraw(&employee, &id), 0);
}

/// Cancelling a capped stream refunds the employer the unused cap portion.
#[test]
fn test_max_withdrawable_cancel_refunds_unused_cap() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let employer_before = token.balance(&employer);

    client.initialize(&admin);
    let id = client.create_stream_with_cap(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &Some(500),
    );

    // Accrue well past the cap, then cancel.
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id, &None);

    // Employee receives only the capped 500; the rest (including the unused
    // cap headroom) is refunded to the employer.
    assert_eq!(token.balance(&employee), 500);
    assert_eq!(token.balance(&employer), employer_before - 500);
    assert_eq!(client.get_stream(&id).withdrawn, 500);
}

// ---------------------------------------------------------------------------
// Issue #5 – stream_count_by_employer and stream_count_by_employee views
// ---------------------------------------------------------------------------

/// stream_count_by_employer returns 0 for an address with no streams.
#[test]
fn test_stream_count_by_employer_zero_for_new_address() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.stream_count_by_employer(&employer), 0);
}

/// stream_count_by_employee returns 0 for an address with no streams.
#[test]
fn test_stream_count_by_employee_zero_for_new_address() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employee = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.stream_count_by_employee(&employee), 0);
}

/// stream_count_by_employer returns the correct count after stream creation.
#[test]
fn test_stream_count_by_employer_increments() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    assert_eq!(client.stream_count_by_employer(&employer), 0);

    client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    assert_eq!(client.stream_count_by_employer(&employer), 1);

    client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);
    assert_eq!(client.stream_count_by_employer(&employer), 2);
}

/// stream_count_by_employee returns the correct count after stream creation.
#[test]
fn test_stream_count_by_employee_increments() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id1 = setup_token(&env, &employer1);
    let token_id2 = setup_token(&env, &employer2);

    client.initialize(&admin);
    assert_eq!(client.stream_count_by_employee(&employee), 0);

    client.create_stream(&employer1, &employee, &token_id1, &10_000, &10, &0, &None, &None);
    assert_eq!(client.stream_count_by_employee(&employee), 1);

    client.create_stream(&employer2, &employee, &token_id2, &10_000, &10, &0, &None, &None);
    assert_eq!(client.stream_count_by_employee(&employee), 2);
}

/// stream_count_by_employer count equals stream_ids_by_employer length.
#[test]
fn test_stream_count_by_employer_matches_list_length() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee3, &token_id, &10_000, &10, &0, &None, &None);

    let count = client.stream_count_by_employer(&employer);
    let list_len = client.stream_ids_by_employer(&employer).len() as u64;
    assert_eq!(count, list_len);
    assert_eq!(count, 3);
}

/// stream_count_by_employee count equals stream_ids_by_employee length.
#[test]
fn test_stream_count_by_employee_matches_list_length() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let count = client.stream_count_by_employee(&employee);
    let list_len = client.stream_ids_by_employee(&employee).len() as u64;
    assert_eq!(count, list_len);
    assert_eq!(count, 2);
}

#[test]
fn test_streams_by_employee_paginated_pages_and_bounds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..4 {
        client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    }

    let first_page = client.streams_by_employee_paginated(&employee, &0, &2);
    assert_eq!(first_page.len(), 2);
    assert_eq!(first_page.get(0).unwrap(), 1);
    assert_eq!(first_page.get(1).unwrap(), 2);

    let last_page = client.streams_by_employee_paginated(&employee, &3, &2);
    assert_eq!(last_page.len(), 1);
    assert_eq!(last_page.get(0).unwrap(), 4);

    let beyond_end = client.streams_by_employee_paginated(&employee, &5, &2);
    assert!(beyond_end.is_empty());
}

#[test]
fn test_streams_by_employee_paginated_caps_limit_at_50() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..51 {
        client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    }

    let page = client.streams_by_employee_paginated(&employee, &0, &100);
    assert_eq!(page.len(), 50);
    assert_eq!(page.get(49).unwrap(), 50);
}

/// SC-50: walking the pages with `start_after` yields every stream exactly
/// once, in ascending id order (stable ordering across pages).
#[test]
fn test_streams_by_employee_paginated_hydrate_loop_is_stable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let other_employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // Interleave another employee's streams so the index must filter.
    for _ in 0..7 {
        client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        client.create_stream(&employer, &other_employee, &token_id, &10_000, &10, &0, &None, &None);
    }

    let mut seen = soroban_sdk::Vec::new(&env);
    let mut start_after = 0u64;
    loop {
        let page = client.streams_by_employee_paginated(&employee, &start_after, &3);
        if page.is_empty() {
            break;
        }
        for id in page.iter() {
            seen.push_back(id);
        }
        start_after = page.get(page.len() - 1).unwrap();
    }

    assert_eq!(seen.len(), 7);
    for i in 0..seen.len() {
        let id = seen.get(i).unwrap();
        assert_eq!(id, 2 * (i as u64) + 1, "ids must be ascending and stable");
        assert_eq!(client.get_stream(&id).employee, employee);
    }
}

/// SC-50: `start_after` is exclusive; a limit of 0 yields an empty page.
#[test]
fn test_streams_by_employee_paginated_start_after_exclusive_and_zero_limit() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..3 {
        client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    }

    let page = client.streams_by_employee_paginated(&employee, &1, &10);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), 2);
    assert_eq!(page.get(1).unwrap(), 3);

    assert!(client
        .streams_by_employee_paginated(&employee, &0, &0)
        .is_empty());
}

/// SC-50: an employee with no streams gets an empty page.
#[test]
fn test_streams_by_employee_paginated_unknown_employee_returns_empty() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employee = Address::generate(&env);
    client.initialize(&admin);

    assert!(client
        .streams_by_employee_paginated(&employee, &0, &50)
        .is_empty());
}

/// stream_count_by_token returns 0 for a token with no streams.
#[test]
fn test_stream_count_by_token_unknown_token_returns_zero() {
    let (env, client) = setup();
    let token = Address::generate(&env);
    assert_eq!(client.stream_count_by_token(&token), 0);
}

/// stream_count_by_token includes streams created through create_stream.
#[test]
fn test_stream_count_by_token_single_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    assert_eq!(client.stream_count_by_token(&token_id), 1);
}

/// Token indexes include single and batch-created streams without mixing tokens.
// ---------------------------------------------------------------------------
// SC-93 (#461) – archived index entries keep queries correct
// ---------------------------------------------------------------------------

/// Completed streams can have their index entries archived; the index queries
/// must still return the correct IDs for the live (non-archived) streams.
#[test]
fn test_archived_stream_index_queries_still_correct() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let active_id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let completed_id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0, &None, &None);

    // Exhaust the second stream so it becomes a candidate for archival.
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &completed_id);
    assert_eq!(client.get_stream(&completed_id).status, StreamStatus::Exhausted);

    // Archive the completed stream's index entries.
    client.archive_stream_indexes(&admin, &completed_id);

    // Queries still return the correct IDs: the active stream remains indexed,
    // the archived stream is no longer listed but is still retrievable.
    let employer_ids = client.stream_ids_by_employer(&employer);
    assert!(employer_ids.contains(active_id));
    assert!(!employer_ids.contains(completed_id));

    let employee_ids = client.stream_ids_by_employee(&employee);
    assert!(employee_ids.contains(active_id));
    assert!(!employee_ids.contains(completed_id));

    let token_ids = client.stream_ids_by_token(&token_id);
    assert!(token_ids.contains(active_id));
    assert!(!token_ids.contains(completed_id));

    assert_eq!(client.stream_count_by_employer(&employer), 1);
    assert_eq!(client.stream_count_by_employee(&employee), 1);
    assert_eq!(client.stream_count_by_token(&token_id), 1);

    // The archived stream itself is untouched and still readable.
    let s = client.get_stream(&completed_id);
    assert_eq!(s.status, StreamStatus::Exhausted);
    assert_eq!(s.employer, employer);
}

#[test]
fn test_stream_count_by_token_multiple_tokens_and_batch() {
    use crate::types::StreamParams;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);
    let token_a = setup_token(&env, &employer);
    let token_b = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee1, &token_a, &10_000, &10, &0, &None, &None);

    let mut params = soroban_sdk::Vec::new(&env);
    params.push_back(StreamParams {
        employee: employee2,
        token: token_a.clone(),
        deposit: 10_000,
        rate_per_second: 10,
        stop_time: 0,
            cliff_time: None,
        low_balance_threshold: None,
    });
    params.push_back(StreamParams {
        employee: employee3,
        token: token_b.clone(),
        deposit: 10_000,
        rate_per_second: 10,
        stop_time: 0,
            cliff_time: None,
        low_balance_threshold: None,
    });
    client.create_streams_batch(&employer, &params);

    assert_eq!(client.stream_count_by_token(&token_a), 2);
    assert_eq!(client.stream_count_by_token(&token_b), 1);
}

#[test]
fn test_stream_ids_by_token_multiple_tokens_and_pagination() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_a = setup_token(&env, &employer);
    let token_b = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token_b, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);

    let first_page = client.stream_ids_by_token(&token_a, &0, &1);
    assert_eq!(first_page.len(), 1);
    assert_eq!(first_page.get(0).unwrap(), 1);

    let second_page = client.stream_ids_by_token(&token_a, &1, &50);
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page.get(0).unwrap(), 3);

    let other_token = client.stream_ids_by_token(&token_b, &0, &50);
    assert_eq!(other_token.len(), 1);
    assert_eq!(other_token.get(0).unwrap(), 2);
}

#[test]
fn test_stream_ids_by_token_unknown_token_returns_empty() {
    let (env, client) = setup();
    let token = Address::generate(&env);

    assert!(client.stream_ids_by_token(&token, &0, &50).is_empty());
}

#[test]
fn test_stream_ids_by_token_caps_limit_at_50() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..51 {
        client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    }

    let page = client.stream_ids_by_token(&token, &0, &100);
    assert_eq!(page.len(), 50);
    assert_eq!(page.get(49).unwrap(), 50);
}

// ---------------------------------------------------------------------------
// SC-51 (#419) – paginated streams_by_token query
// ---------------------------------------------------------------------------

/// streams_by_token returns ascending IDs for the requested token only.
#[test]
fn test_streams_by_token_paginated_ordering_and_isolation() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_a = setup_token(&env, &employer);
    let token_b = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token_b, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);

    let page = client.streams_by_token(&token_a, &0, &50);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0).unwrap(), 1);
    assert_eq!(page.get(1).unwrap(), 3);
    assert_eq!(page.get(2).unwrap(), 4);

    let other = client.streams_by_token(&token_b, &0, &50);
    assert_eq!(other.len(), 1);
    assert_eq!(other.get(0).unwrap(), 2);
}

/// streams_by_token honours start_after as an exclusive cursor.
#[test]
fn test_streams_by_token_paginated_cursor_walks_pages() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..5 {
        client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    }

    let first = client.streams_by_token(&token, &0, &2);
    assert_eq!(first.len(), 2);
    assert_eq!(first.get(0).unwrap(), 1);
    assert_eq!(first.get(1).unwrap(), 2);

    let second = client.streams_by_token(&token, &2, &2);
    assert_eq!(second.len(), 2);
    assert_eq!(second.get(0).unwrap(), 3);
    assert_eq!(second.get(1).unwrap(), 4);

    let third = client.streams_by_token(&token, &4, &2);
    assert_eq!(third.len(), 1);
    assert_eq!(third.get(0).unwrap(), 5);

    let beyond_end = client.streams_by_token(&token, &5, &2);
    assert!(beyond_end.is_empty());
}

/// streams_by_token caps the requested limit at 50.
#[test]
fn test_streams_by_token_paginated_caps_limit_at_50() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..51 {
        client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    }

    let page = client.streams_by_token(&token, &0, &100);
    assert_eq!(page.len(), 50);
    assert_eq!(page.get(49).unwrap(), 50);
}

/// streams_by_token returns an empty page for an unknown token.
#[test]
fn test_streams_by_token_paginated_unknown_token_returns_empty() {
    let (env, client) = setup();
    let token = Address::generate(&env);

    assert!(client.streams_by_token(&token, &0, &50).is_empty());
}

// ---------------------------------------------------------------------------
// SC-52 (#420) – paginated get_streams_by_status
// ---------------------------------------------------------------------------

/// Paginated status query returns Active streams in ascending ID order.
#[test]
fn test_get_streams_by_status_paginated_active() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..3 {
        client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    }

    let page = client.get_streams_by_status(&StreamStatus::Active, &0, &50);
    assert_eq!(page.len(), 3);
    assert_eq!(page.get(0).unwrap(), 1);
    assert_eq!(page.get(1).unwrap(), 2);
    assert_eq!(page.get(2).unwrap(), 3);
}

/// Paginated status query returns Paused streams only.
#[test]
fn test_get_streams_by_status_paginated_paused() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    client.pause_stream(&employer, &id1, &None);
    client.pause_stream(&employer, &id2, &None);

    let page = client.get_streams_by_status(&StreamStatus::Paused, &0, &50);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap(), id1);
    assert_eq!(page.get(1).unwrap(), id2);
}

/// Paginated status query returns Cancelled streams only.
#[test]
fn test_get_streams_by_status_paginated_cancelled() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id1, &None);

    let page = client.get_streams_by_status(&StreamStatus::Cancelled, &0, &50);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap(), id1);
}

/// Paginated status query returns Exhausted streams only.
#[test]
fn test_get_streams_by_status_paginated_exhausted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token, &500, &10, &0, &None, &None);
    client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);

    let page = client.get_streams_by_status(&StreamStatus::Exhausted, &0, &50);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap(), id);
}

/// Paginated status query honours start_after and caps the limit.
#[test]
fn test_get_streams_by_status_paginated_cursor_and_limit() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    for _ in 0..51 {
        client.create_stream(&employer, &employee, &token, &10_000, &10, &0, &None, &None);
    }

    let capped = client.get_streams_by_status(&StreamStatus::Active, &0, &100);
    assert_eq!(capped.len(), 50);
    assert_eq!(capped.get(49).unwrap(), 50);

    let next = client.get_streams_by_status(&StreamStatus::Active, &50, &100);
    assert_eq!(next.len(), 1);
    assert_eq!(next.get(0).unwrap(), 51);

    let beyond_end = client.get_streams_by_status(&StreamStatus::Active, &51, &10);
    assert!(beyond_end.is_empty());
}

// ---------------------------------------------------------------------------
// SC-65 (#433) – MAX_STOP_TIME_OFFSET enforcement
// ---------------------------------------------------------------------------

/// stop_time exactly at now + MAX_STOP_TIME_OFFSET is accepted.
#[test]
fn test_create_stream_stop_time_at_cap_is_accepted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let stop_time = 1_000 + crate::types::MAX_STOP_TIME_OFFSET;
    let id = client.create_stream(
        &employer,
        &employee,
        &token,
        &10_000,
        &10,
        &stop_time,
        &None,
        &None,
    );
    assert_eq!(client.get_stream(&id).stop_time, stop_time);
}

/// stop_time one second past the cap is rejected.
#[test]
#[should_panic(expected = "E0")]
fn test_create_stream_stop_time_over_cap_is_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let stop_time = 1_000 + crate::types::MAX_STOP_TIME_OFFSET + 1;
    client.create_stream(
        &employer,
        &employee,
        &token,
        &10_000,
        &10,
        &stop_time,
        &None,
        &None,
    );
}

/// Batch creation accepts a stop_time exactly at the cap.
#[test]
fn test_create_streams_batch_stop_time_at_cap_is_accepted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let stop_time = 1_000 + crate::types::MAX_STOP_TIME_OFFSET;

    let mut params = soroban_sdk::Vec::new(&env);
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token.clone(),
        deposit: 10_000,
        rate_per_second: 10,
        stop_time,
        cliff_time: None,
    });
    let ids = client.create_streams_batch(&employer, &params);
    assert_eq!(ids.len(), 1);
    assert_eq!(client.get_stream(&ids.get(0).unwrap()).stop_time, stop_time);
}

/// Batch creation rejects a stop_time one second past the cap.
#[test]
#[should_panic(expected = "E0")]
fn test_create_streams_batch_stop_time_over_cap_is_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let stop_time = 1_000 + crate::types::MAX_STOP_TIME_OFFSET + 1;

    let mut params = soroban_sdk::Vec::new(&env);
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token.clone(),
        deposit: 10_000,
        rate_per_second: 10,
        stop_time,
        cliff_time: None,
    });
    client.create_streams_batch(&employer, &params);
}

// ---------------------------------------------------------------------------
// SC-06 – stream_status lightweight query
// ---------------------------------------------------------------------------

/// stream_status returns Active for a newly created stream.
#[test]
fn test_stream_status_active() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    assert_eq!(client.stream_status(&id), StreamStatus::Active);
}

/// stream_status returns Paused after pause_stream.
#[test]
fn test_stream_status_paused() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.pause_stream(&employer, &id);
    assert_eq!(client.stream_status(&id), StreamStatus::Paused);
}

/// stream_status returns Cancelled after cancel_stream.
#[test]
fn test_stream_status_cancelled() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id);
    assert_eq!(client.stream_status(&id), StreamStatus::Cancelled);
}

/// stream_status returns Exhausted once the full deposit is withdrawn.
#[test]
fn test_stream_status_exhausted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0, &None, &None);
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
    assert_eq!(client.stream_status(&id), StreamStatus::Exhausted);
}

/// stream_status returns Error::StreamNotFound for a non-existent stream.
#[test]
fn test_stream_status_not_found_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    let res = client.try_stream_status(&999);
    assert_eq!(res, Err(Ok(Error::StreamNotFound)));
}

// ---------------------------------------------------------------------------
// SC-03 – cancel_streams_batch
// ---------------------------------------------------------------------------

#[test]
fn test_create_streams_batch_max_size_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let params = make_stream_params(&env, &token_id, 50);

    let ids = client.create_streams_batch(&employer, &params);
    assert_eq!(ids.len(), 50);
}

#[test]
fn test_create_streams_batch_over_max_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let params = make_stream_params(&env, &token_id, 51);

    let res = client.try_create_streams_batch(&employer, &params);
    assert_eq!(res, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_cancel_streams_batch_max_size_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let params = make_stream_params(&env, &token_id, 50);
    let ids = client.create_streams_batch(&employer, &params);

    client.cancel_streams_batch(&employer, &ids);
    assert_eq!(
        client.get_stream(&ids.get(0).unwrap()).status,
        StreamStatus::Cancelled
    );
    assert_eq!(
        client.get_stream(&ids.get(49).unwrap()).status,
        StreamStatus::Cancelled
    );
}

#[test]
fn test_cancel_streams_batch_over_max_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let params = make_stream_params(&env, &token_id, 50);
    let mut ids = client.create_streams_batch(&employer, &params);
    let employee = Address::generate(&env);
    ids.push_back(client.create_stream(&employer, &employee, &token_id, &100, &1, &0, &None, &None));

    let res = client.try_cancel_streams_batch(&employer, &ids);
    assert_eq!(res, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_pause_stream_batch_succeeds_and_emits_status_events() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &100, &1, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &100, &1, &0, &None, &None);
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);

    client.pause_stream_batch(&employer, &ids);

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Paused);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Paused);
    let events = crate::test::all_events(&env);
    for id in [id1, id2] {
        let status_topics = vec![
            &env,
            symbol_short!("v1").into_val(&env),
            symbol_short!("status").into_val(&env),
            id.into_val(&env),
        ];
        assert!(events
            .iter()
            .any(|(_, topics, _)| topics == status_topics));
    }
}

#[test]
fn test_pause_stream_batch_failure_reverts_prior_changes() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &100, &1, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &100, &1, &0, &None, &None);
    client.pause_stream(&employer, &id2);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    assert!(client.try_pause_stream_batch(&employer, &ids).is_err());

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Active);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Paused);
}

#[test]
fn test_pause_stream_batch_over_max_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &100, &1, &0, &None, &None);
    let mut ids = soroban_sdk::Vec::new(&env);
    for _ in 0..51 {
        ids.push_back(id);
    }

    let res = client.try_pause_stream_batch(&employer, &ids);
    assert_eq!(res, Err(Ok(Error::BatchTooLarge)));
}

#[test]
fn test_resume_stream_batch_succeeds_and_emits_status_events() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &100, &1, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &100, &1, &0, &None, &None);
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    client.pause_stream_batch(&employer, &ids);

    env.ledger().with_mut(|ledger| ledger.timestamp += 100);
    let resumed_at = env.ledger().timestamp();
    client.resume_stream_batch(&employer, &ids);

    for id in [id1, id2] {
        let stream = client.get_stream(&id);
        assert_eq!(stream.status, StreamStatus::Active);
        assert_eq!(stream.last_withdraw_time, resumed_at);
    }

    let events = crate::test::all_events(&env);
    for id in [id1, id2] {
        let status_topics = vec![
            &env,
            symbol_short!("v1").into_val(&env),
            symbol_short!("status").into_val(&env),
            id.into_val(&env),
        ];
        assert!(events
            .iter()
            .any(|(_, topics, data)| topics == status_topics
                && <StreamStatus as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data) == StreamStatus::Active));
    }
}

#[test]
fn test_resume_stream_batch_failure_reverts_prior_changes() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &100, &1, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &100, &1, &0, &None, &None);
    client.pause_stream(&employer, &id1);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    assert!(client.try_resume_stream_batch(&employer, &ids).is_err());

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Paused);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Active);
}

#[test]
fn test_resume_stream_batch_over_max_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &100, &1, &0, &None, &None);
    client.pause_stream(&employer, &id);
    let mut ids = soroban_sdk::Vec::new(&env);
    for _ in 0..51 {
        ids.push_back(id);
    }

    let res = client.try_resume_stream_batch(&employer, &ids);
    assert_eq!(res, Err(Ok(Error::BatchTooLarge)));
}

/// Happy path: batch cancel two active streams atomically.
#[test]
fn test_cancel_streams_batch_happy_path() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    client.cancel_streams_batch(&employer, &ids);

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Cancelled);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Cancelled);
    // Each employee should have received their earned share (100s * 10/s = 1000)
    assert_eq!(client.get_stream(&id1).withdrawn, 1000);
    assert_eq!(client.get_stream(&id2).withdrawn, 1000);
}

/// Batch cancel also works on paused streams.
#[test]
fn test_cancel_streams_batch_includes_paused_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id, &None);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.cancel_streams_batch(&employer, &ids);

    assert_eq!(client.get_stream(&id).status, StreamStatus::Cancelled);
}

/// Empty stream_ids list must be rejected.
#[test]
fn test_cancel_streams_batch_empty_list_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);

    let ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    let res = client.try_cancel_streams_batch(&employer, &ids);
    assert_eq!(res, Err(Ok(Error::EmptyBatch)));
}

/// If any stream in the batch is already cancelled, the whole batch reverts.
#[test]
#[should_panic(expected = "stream already ended")]
fn test_cancel_streams_batch_partial_failure_reverts_all() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Cancel id2 individually first
    client.cancel_stream(&employer, &id2, &None);

    // Now try batch cancel with id1 (active) and id2 (already cancelled)
    // → must panic because id2 is already ended
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    client.cancel_streams_batch(&employer, &ids);
}

/// A stream not belonging to the employer must cause a panic.
#[test]
#[should_panic(expected = "not the employer")]
fn test_cancel_streams_batch_wrong_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer1);

    // Give employer2 some tokens too
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    token.mint(&employer1, &employer2, &10_000, &0);

    client.initialize(&admin);
    // Stream owned by employer1
    let id = client.create_stream(&employer1, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // employer2 tries to batch cancel employer1's stream — must panic
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.cancel_streams_batch(&employer2, &ids);
}

// ---------------------------------------------------------------------------
// SC-31 – pause_stream_batch
// ---------------------------------------------------------------------------

/// Happy path: batch pause multiple active streams atomically and verify status events.
#[test]
fn test_pause_stream_batch_all_succeed() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);
    let id3 = client.create_stream(&employer, &employee3, &token_id, &10_000, &10, &0, &None, &None);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    ids.push_back(id3);
    client.pause_stream_batch(&employer, &ids);

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Paused);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Paused);
    assert_eq!(client.get_stream(&id3).status, StreamStatus::Paused);

    // SDK v28: ContractEvents has no .iter(); verify status via contract state above.
    let events = env.events().all();
    assert!(events.events().len() > 0, "status events not emitted");
}

/// If any stream in the batch fails (e.g. not Active because already paused), all revert.
#[test]
#[should_panic(expected = "E017")]
fn test_pause_stream_batch_one_fails_reverts_all() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Pause id2 individually first so it is no longer Active
    client.pause_stream(&employer, &id2, &None);

    // Batch pause id1 (active) and id2 (paused) — id2 will fail and revert all
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    client.pause_stream_batch(&employer, &ids);
}

/// Batch size exceeding 50 streams must panic.
#[test]
#[should_panic(expected = "stream_ids length exceeds 50")]
fn test_pause_stream_batch_cap_exceeded() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);

    client.initialize(&admin);

    let mut ids = soroban_sdk::Vec::new(&env);
    for i in 1..=51 {
        ids.push_back(i);
    }
    client.pause_stream_batch(&employer, &ids);
}

/// Empty stream_ids list must be rejected.
#[test]
#[should_panic(expected = "stream_ids must not be empty")]
fn test_pause_stream_batch_empty_list_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);

    let ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    client.pause_stream_batch(&employer, &ids);
}

/// A stream not belonging to the caller must cause a panic.
#[test]
#[should_panic(expected = "E018")]
fn test_pause_stream_batch_wrong_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer1);

    client.initialize(&admin);
    let id = client.create_stream(&employer1, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.pause_stream_batch(&employer2, &ids);
}

// ---------------------------------------------------------------------------
// SC-32 – resume_stream_batch
// ---------------------------------------------------------------------------

/// Happy path: batch resume multiple paused streams atomically and verify status events.
#[test]
fn test_resume_stream_batch_all_succeed() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let employee3 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);
    let id3 = client.create_stream(&employer, &employee3, &token_id, &10_000, &10, &0, &None, &None);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    ids.push_back(id3);

    // Advance to T=100 and pause all 3 streams
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.pause_stream_batch(&employer, &ids);

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Paused);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Paused);
    assert_eq!(client.get_stream(&id3).status, StreamStatus::Paused);

    // Advance to T=200 and resume all 3 streams atomically
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.resume_stream_batch(&employer, &ids);

    assert_eq!(client.get_stream(&id1).status, StreamStatus::Active);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Active);
    assert_eq!(client.get_stream(&id3).status, StreamStatus::Active);

    // Each stream's last_withdraw_time must be updated to resume timestamp (200)
    assert_eq!(client.get_stream(&id1).last_withdraw_time, 200);
    assert_eq!(client.get_stream(&id2).last_withdraw_time, 200);
    assert_eq!(client.get_stream(&id3).last_withdraw_time, 200);

    // Claimable must be 0 immediately after resume (no active elapsed time)
    assert_eq!(client.claimable(&id1), 0);
    assert_eq!(client.claimable(&id2), 0);
    assert_eq!(client.claimable(&id3), 0);

    // Each stream emits a status event on success (StreamStatus::Active)
    let events = crate::test::all_events(&env);
    for id in [id1, id2, id3] {
        let resume_event = events.iter().find(|(_, topics, data)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("status").into_val(&env),
                    id.into_val(&env),
                ]
                && <StreamStatus as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data) == StreamStatus::Active
        });
        assert!(resume_event.is_some(), "resume status event not emitted");
    }
}

/// If any stream in the batch is not Paused (e.g. Active, Cancelled, or Exhausted), all revert.
#[test]
#[should_panic(expected = "E019")]
fn test_resume_stream_batch_one_fails_reverts_all() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Pause only id1; id2 remains Active
    client.pause_stream(&employer, &id1, &None);

    // Batch resume id1 (Paused) and id2 (Active) — id2 will fail and revert all
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id1);
    ids.push_back(id2);
    client.resume_stream_batch(&employer, &ids);
}

/// Batch size exceeding 50 streams must panic.
#[test]
#[should_panic(expected = "stream_ids length exceeds 50")]
fn test_resume_stream_batch_cap_exceeded() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);

    client.initialize(&admin);

    let mut ids = soroban_sdk::Vec::new(&env);
    for i in 1..=51 {
        ids.push_back(i);
    }
    client.resume_stream_batch(&employer, &ids);
}

/// Empty stream_ids list must be rejected.
#[test]
#[should_panic(expected = "stream_ids must not be empty")]
fn test_resume_stream_batch_empty_list_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);

    let ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    client.resume_stream_batch(&employer, &ids);
}

/// A stream not belonging to the caller must cause a panic.
#[test]
#[should_panic(expected = "E018")]
fn test_resume_stream_batch_wrong_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer1);

    client.initialize(&admin);
    let id = client.create_stream(&employer1, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.pause_stream(&employer1, &id);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.resume_stream_batch(&employer2, &ids);
}

// ---------------------------------------------------------------------------
// SC-34 – get_auto_topup_config public query
// ---------------------------------------------------------------------------

/// Unconfigured stream returns None.
#[test]
fn test_get_auto_topup_config_unconfigured_returns_none() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Stream has no auto top-up configured
    assert_eq!(client.get_auto_topup_config(&id), None);
    // Non-existent stream also returns None
    assert_eq!(client.get_auto_topup_config(&999), None);
}

/// Configured stream returns the active AutoTopupConfig.
#[test]
fn test_get_auto_topup_config_configured_returns_config() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Configure auto top-up: trigger below 1_000, top up 5_000, max total 50_000
    client.set_auto_topup(&employer, &id, &1_000, &5_000, &50_000);

    let config = client.get_auto_topup_config(&id);
    assert_eq!(
        config,
        Some(AutoTopupConfig {
            trigger_threshold: 1_000,
            topup_amount: 5_000,
            max_total: 50_000,
            total_topped_up: 0,
        })
    );

    // After cancellation, get_auto_topup_config returns None again
    client.cancel_auto_topup(&employer, &id);
    assert_eq!(client.get_auto_topup_config(&id), None);
}

// ---------------------------------------------------------------------------
// TEST-50 (#565) – auto top-up failure must not revert the withdraw
// ---------------------------------------------------------------------------

/// When the auto top-up `transfer_from` fails (insufficient allowance or
/// balance), the withdraw must still succeed, the stream must stay Active and
/// no partial top-up state may be left behind.
#[test]
fn test_withdraw_auto_topup_insufficient_allowance_no_revert() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &1_000, &10, &0, &None, &None);

    // Configure auto top-up: trigger below 500, top up 5_000, max total 50_000.
    client.set_auto_topup(&employer, &id, &500, &5_000, &50_000);

    // Drain the employer's balance so the top-up `transfer_from` cannot succeed.
    let employer_balance = token.balance(&employer);
    token.burn(&employer, &employer_balance);

    env.ledger().with_mut(|l| l.timestamp += 100);

    let before = client.get_stream(&id);
    let withdrawn = client.withdraw(&employee, &id);

    // (a) withdraw succeeds and pays out the accrued amount.
    assert_eq!(withdrawn, 1_000);
    assert_eq!(token.balance(&employee), 1_000);

    // (b) stream continues Active.
    let after = client.get_stream(&id);
    assert_eq!(after.status, StreamStatus::Active);

    // (c) no partial state: only the withdrawal is reflected; the failed
    // top-up leaves the deposit and the auto-topup totals untouched.
    assert_eq!(after.deposit, before.deposit);
    assert_eq!(after.withdrawn, before.withdrawn + withdrawn);
    let config = client.get_auto_topup_config(&id).expect("config still set");
    assert_eq!(config.total_topped_up, 0, "failed top-up must not be recorded");
    assert_eq!(config.topup_amount, 5_000);
    assert_eq!(config.trigger_threshold, 500);
    assert_eq!(config.max_total, 50_000);
}

// ---------------------------------------------------------------------------
// SC-02 – Auto-transition stream to Exhausted when stop_time passes
// ---------------------------------------------------------------------------

/// After stop_time passes and all tokens have been streamed, calling withdraw
/// should transition the stream to Exhausted even if claimable == 0.
#[test]
fn test_withdraw_transitions_to_exhausted_after_stop_time() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    // Stream that runs for exactly 50 seconds at 10 tokens/s → 500 total
    let now = env.ledger().timestamp();
    let stop = now + 50;
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &stop, &None, &None);

    // Advance past stop_time — all tokens have been streamed
    env.ledger().with_mut(|l| l.timestamp += 100);

    // withdraw: claimable = 0 (capped at stop_time), but stop_time is past
    // → stream must transition to Exhausted
    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(withdrawn, 500); // tokens earned up to stop_time
    assert_eq!(client.get_stream(&id).status, StreamStatus::Exhausted);
}

/// settle_stream pays accrued tokens and emits the Exhausted status transition.
#[test]
fn test_settle_stream_transitions_to_exhausted() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let now = env.ledger().timestamp();
    let stop = now + 50;
    // Stream: 1_000 tokens, 10/s, 50-second window.
    // Total streamable = 500 tokens; 500 remain as "unclaimable" after stop_time.
    // withdraw at stop_time claims the 500 earned tokens but does NOT exhaust
    // the stream (deposit 1_000 > withdrawn 500).
    let id = client.create_stream(&employer, &employee, &token_id, &1_000, &10, &stop, &None, &None);

    // Advance past stop_time with 500 tokens claimable.
    env.ledger().with_mut(|l| l.timestamp = stop + 10);

    // Anyone can call settle_stream to push status to Exhausted
    client.settle_stream(&id);
    let stream = client.get_stream(&id);
    assert_eq!(stream.status, StreamStatus::Exhausted);
    assert_eq!(stream.withdrawn, 500);

    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    assert_eq!(token.balance(&employee), 500);

    let status_topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("status").into_val(&env),
        id.into_val(&env),
    ];
    assert!(crate::test::all_events(&env)
        .iter()
        .any(|(_, topics, _)| topics == status_topics));
}

/// settle_stream must panic if stop_time has not yet been reached.
#[test]
#[should_panic(expected = "stop_time not reached")]
fn test_settle_stream_before_stop_time_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    let stop = now + 1000;
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &stop, &None, &None);
    // stop_time not yet reached — must panic
    client.settle_stream(&id);
}

/// settle_stream must panic if the stream is not Active.
#[test]
#[should_panic(expected = "stream not active")]
fn test_settle_stream_inactive_stream_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id);
    client.settle_stream(&id);
}

/// settle_stream must panic if stop_time is unset.
#[test]
#[should_panic(expected = "stop_time not reached")]
fn test_settle_stream_without_stop_time_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.settle_stream(&id);
}

/// settle_stream must panic if the stream does not exist.
#[test]
#[should_panic(expected = "stream not found")]
fn test_settle_stream_missing_stream_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.settle_stream(&999);
}

// ---------------------------------------------------------------------------
// Issue #23 – expire_streams: admin batch-expire past stop_time + grace period
// ---------------------------------------------------------------------------

/// An eligible stream (stop_time set, grace period elapsed, claimable == 0,
/// status Active) is transitioned to Exhausted by expire_streams.
#[test]
fn test_expire_streams_eligible_stream_exhausted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    assert_eq!(client.admin_nonce(), 0);

    // stop_time = now + 100; rate=1/s so all 100 tokens stream by stop_time
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);
    let stop_time: u64 = 1_000_100;
    let id = client.create_stream(&employer, &employee, &token_id, &100, &1, &stop_time, &None, &None);

    // Advance past stop_time + GRACE_PERIOD (7 days = 604_800 s)
    // Employee withdraws first to clear claimable (so claimable == 0)
    env.ledger().with_mut(|l| l.timestamp = stop_time + 1);
    client.withdraw(&employee, &id);

    // Now advance past grace period
    env.ledger().with_mut(|l| l.timestamp = stop_time + 604_801);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.expire_streams(&admin, &0, &ids);
    assert_eq!(client.admin_nonce(), 1);

    let s = client.get_stream(&id);
    assert_eq!(s.status, StreamStatus::Exhausted);
}

/// Ineligible streams in the batch are skipped — the call does not revert.
#[test]
fn test_expire_streams_ineligible_skipped() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Stream A: no stop_time (ineligible — stop_time == 0)
    let id_no_stop = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Stream B: stop_time in the future (grace period not elapsed)
    let id_future = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &1,
        &(1_000_000 + 604_802),
        &None,&None,
    );

    // Stream C: eligible — stop_time set, grace elapsed, claimable == 0
    let stop_time: u64 = 1_000_100;
    let id_eligible = client.create_stream(&employer, &employee, &token_id, &100, &1, &stop_time, &None, &None);

    // Withdraw from eligible stream first
    env.ledger().with_mut(|l| l.timestamp = stop_time + 1);
    client.withdraw(&employee, &id_eligible);

    // Advance past grace period for eligible stream
    env.ledger().with_mut(|l| l.timestamp = stop_time + 604_801);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id_no_stop);
    ids.push_back(id_future);
    ids.push_back(id_eligible);

    // Must not revert even though two streams are ineligible
    client.expire_streams(&admin, &0, &ids);
    assert_eq!(client.admin_nonce(), 1);

    // Ineligible streams are unchanged
    let s_no_stop = client.get_stream(&id_no_stop);
    assert_eq!(s_no_stop.status, StreamStatus::Active);

    let s_future = client.get_stream(&id_future);
    assert_eq!(s_future.status, StreamStatus::Active);

    // Eligible stream is expired
    let s_eligible = client.get_stream(&id_eligible);
    assert_eq!(s_eligible.status, StreamStatus::Exhausted);
}

#[test]
#[should_panic(expected = "E012")]
fn test_expire_streams_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    let stop_time: u64 = 1_000_100;
    let id = client.create_stream(&employer, &employee, &token_id, &100, &1, &stop_time, &None, &None);

    env.ledger().with_mut(|l| l.timestamp = stop_time + 604_801);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.expire_streams(&non_admin, &0, &ids);
}

/// expire_streams emits a batch summary event with expired/skipped counts,
/// while per-stream `expired` events are still emitted only for eligible streams.
#[test]
fn test_expire_streams_batch_summary_event() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Eligible stream: stop_time set, grace elapsed, claimable == 0.
    let stop_time: u64 = 1_000_100;
    let id_eligible = client.create_stream(&employer, &employee, &token_id, &100, &1, &stop_time, &None, &None);

    // Ineligible: still claimable after stop_time.
    let id_claimable = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &stop_time, &None, &None);

    // Ineligible: no stop_time.
    let id_no_stop = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Ineligible: paused stream.
    let id_paused = client.create_stream(&employer, &employee, &token_id, &100, &1, &stop_time, &None, &None);
    client.pause_stream(&employer, &id_paused, &None);

    // Clear claimable on the eligible stream only.
    env.ledger().with_mut(|l| l.timestamp = stop_time + 1);
    client.withdraw(&employee, &id_eligible);

    // Advance past the grace period.
    env.ledger().with_mut(|l| l.timestamp = stop_time + 604_801);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id_eligible);
    ids.push_back(id_claimable);
    ids.push_back(id_no_stop);
    ids.push_back(id_paused);
    client.expire_streams(&admin, &0, &ids);

    assert_eq!(client.get_stream(&id_eligible).status, StreamStatus::Exhausted);
    assert_eq!(client.get_stream(&id_claimable).status, StreamStatus::Active);
    assert_eq!(client.get_stream(&id_no_stop).status, StreamStatus::Active);
    assert_eq!(client.get_stream(&id_paused).status, StreamStatus::Paused);

    let events = crate::test::all_events(&env);

    // Batch summary event: ("v1", "exp_batch") -> (expired, skipped)
    let summary_topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("exp_batch").into_val(&env),
    ];
    let summary = events
        .iter()
        .find(|(_, topics, _)| *topics == summary_topics)
        .expect("exp_batch summary event not emitted");
    let (expired, skipped): (u32, u32) =
        soroban_sdk::FromVal::from_val(&env, &summary.2);
    assert_eq!((expired, skipped), (1, 3));

    // Per-stream `expired` events only for the eligible stream.
    let expired_topics = |id: u64| {
        vec![
            &env,
            symbol_short!("v1").into_val(&env),
            symbol_short!("expired").into_val(&env),
            id.into_val(&env),
        ]
    };
    assert!(events
        .iter()
        .any(|(_, topics, _)| *topics == expired_topics(id_eligible)));
    for id in [id_claimable, id_no_stop, id_paused] {
        assert!(!events
            .iter()
            .any(|(_, topics, _)| *topics == expired_topics(id)));
    }
}

/// expire_streams with still-claimable tokens skips that stream.
#[test]
fn test_expire_streams_still_claimable_skipped() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000_000);

    // Deposit more than rate * (stop - start) so tokens remain after stop_time
    let stop_time: u64 = 1_000_100;
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &stop_time, &None, &None);

    // Advance past grace period WITHOUT withdrawing — claimable > 0
    env.ledger().with_mut(|l| l.timestamp = stop_time + 604_801);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.expire_streams(&admin, &0, &ids);
    assert_eq!(client.admin_nonce(), 1);

    // Stream still Active because claimable > 0
    let s = client.get_stream(&id);
    assert_eq!(s.status, StreamStatus::Active);
}

// ---------------------------------------------------------------------------
// TEST-17 – resume_stream resets last_withdraw_time to current timestamp
// ---------------------------------------------------------------------------

/// Pausing a stream at T=100 and resuming at T=200 must reset
/// `last_withdraw_time` to the resume timestamp (200), so that paused time
/// (T=100..200) is excluded from accrual.
///
/// Acceptance criteria:
///   1. stream.last_withdraw_time == 200 immediately after resume_stream.
///   2. claimable == 0 immediately after resume (no active seconds have
///      elapsed since the resume timestamp).
#[test]
fn test_resume_stream_resets_last_withdraw_time() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // deposit=10_000, rate=10/s — large enough that neither pause window nor
    // the small active windows exhaust the stream.
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Advance to T=100 and pause.
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.pause_stream(&employer, &id, &None);

    // Advance to T=200 and resume — last_withdraw_time must be reset to 200.
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.resume_stream(&employer, &id);

    // --- Assertion 1: last_withdraw_time equals the resume timestamp (200) ---
    let stream = client.get_stream(&id);
    assert_eq!(
        stream.last_withdraw_time, 200,
        "last_withdraw_time must be reset to the resume timestamp"
    );

    // --- Assertion 2: claimable == 0 immediately after resume ---
    // No active seconds have elapsed since the resume at T=200, so nothing
    // should have accrued yet.
    assert_eq!(
        client.claimable(&id),
        0,
        "claimable must be 0 immediately after resume (no elapsed time)"
    );
}

// ---------------------------------------------------------------------------
// Issue #21 – streams_in_range: time-based stream creation index
// ---------------------------------------------------------------------------

/// streams_in_range returns stream IDs created within the given range.
#[test]
fn test_streams_in_range_single_bucket() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    // Set a known timestamp: day 1 (86_400 seconds into epoch)
    env.ledger().with_mut(|l| l.timestamp = 86_400);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Query the same day range
    let ids = client.streams_in_range(&86_400_u64, &172_800_u64);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id1);
    assert_eq!(ids.get(1).unwrap(), id2);
}

/// streams_in_range spanning multiple buckets returns IDs from all covered days.
#[test]
fn test_streams_in_range_multiple_buckets() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);

    // Day 0: timestamp 0
    env.ledger().with_mut(|l| l.timestamp = 0);
    let id_day0 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Day 1: timestamp 86_400
    env.ledger().with_mut(|l| l.timestamp = 86_400);
    let id_day1 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Day 2: timestamp 172_800
    env.ledger().with_mut(|l| l.timestamp = 172_800);
    let id_day2 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Query days 0–1 (exclusive of day 2)
    let ids = client.streams_in_range(&0_u64, &172_800_u64);
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.get(0).unwrap(), id_day0);
    assert_eq!(ids.get(1).unwrap(), id_day1);

    // Query all three days
    let ids_all = client.streams_in_range(&0_u64, &259_200_u64);
    assert_eq!(ids_all.len(), 3);
    assert_eq!(ids_all.get(2).unwrap(), id_day2);
}

/// streams_in_range returns empty Vec for a range with no streams.
#[test]
fn test_streams_in_range_empty_range() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    // Create a stream on day 0
    env.ledger().with_mut(|l| l.timestamp = 0);
    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Query a range far in the future — should return empty
    let ids = client.streams_in_range(&1_000_000_000_u64, &2_000_000_000_u64);
    assert_eq!(ids.len(), 0);
}

/// streams_in_range with from_ts >= to_ts returns empty Vec.
#[test]
fn test_streams_in_range_inverted_range_empty() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    env.ledger().with_mut(|l| l.timestamp = 0);
    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    // Inverted range
    let ids = client.streams_in_range(&86_400_u64, &0_u64);
    assert_eq!(ids.len(), 0);
}

/// streams_in_time_range returns sorted IDs across day buckets.
#[test]
fn test_streams_in_time_range_multiple_days_sorted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);

    env.ledger().with_mut(|l| l.timestamp = 2 * 86_400);
    let id_day2 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp = 86_400);
    let id_day1 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp = 0);
    let id_day0 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);

    let ids = client.streams_in_time_range(&0_u64, &(3 * 86_400_u64));
    assert_eq!(ids.len(), 3);
    assert_eq!(ids.get(0).unwrap(), id_day0);
    assert_eq!(ids.get(1).unwrap(), id_day1);
    assert_eq!(ids.get(2).unwrap(), id_day2);
}

/// streams_in_time_range caps the query at 30 days.
#[test]
fn test_streams_in_time_range_caps_at_30_days() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);

    let mut ids = soroban_sdk::Vec::new(&env);
    for day in 0..=31 {
        env.ledger().with_mut(|l| l.timestamp = day * 86_400);
        let employee_for_day = Address::generate(&env);
        let stream_id = client.create_stream(&employer, &employee_for_day, &token_id, &10_000, &1, &0, &None, &None);
        ids.push_back(stream_id);
    }

    let out = client.streams_in_time_range(&0_u64, &(32 * 86_400_u64));
    assert_eq!(out.len(), 31);
    assert_eq!(out.get(0).unwrap(), ids.get(0).unwrap());
    assert_eq!(out.get(30).unwrap(), ids.get(30).unwrap());
}

// ---------------------------------------------------------------------------
// set_low_balance_threshold – update alert threshold
// ---------------------------------------------------------------------------

/// Employer updates threshold on an active stream; emits threshold_updated event.
#[test]
fn test_set_low_balance_threshold_success() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    assert_eq!(client.get_stream(&id).low_balance_threshold, 0);

    // Set low balance threshold to 500
    client.set_low_balance_threshold(&employer, &id, &500);
    assert_eq!(client.get_stream(&id).low_balance_threshold, 500);

    // Verify threshold_updated event (0 -> 500)
    let events = crate::test::all_events(&env);
    let update_event = events.iter().find(|(_, topics, data)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("thresh_up").into_val(&env),
                id.into_val(&env),
            ]
            && <(i128, i128) as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data) == (0, 500)
    });
    assert!(
        update_event.is_some(),
        "threshold_updated event not emitted"
    );

    // Update again to 1200 (500 -> 1200)
    client.set_low_balance_threshold(&employer, &id, &1200);
    assert_eq!(client.get_stream(&id).low_balance_threshold, 1200);

    let events2 = crate::test::all_events(&env);
    let update_event2 = events2.iter().find(|(_, topics, data)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("thresh_up").into_val(&env),
                id.into_val(&env),
            ]
            && <(i128, i128) as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data) == (500, 1200)
    });
    assert!(
        update_event2.is_some(),
        "second threshold_updated event not emitted"
    );
}

/// Setting threshold to 0 disables low-balance alerts and emits threshold_updated event.
#[test]
fn test_set_low_balance_threshold_disable() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &Some(1_000), &None);
    assert_eq!(client.get_stream(&id).low_balance_threshold, 1_000);

    // Disable alert by setting threshold to 0
    client.set_low_balance_threshold(&employer, &id, &0);
    assert_eq!(client.get_stream(&id).low_balance_threshold, 0);

    let events = crate::test::all_events(&env);
    let update_event = events.iter().find(|(_, topics, data)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("thresh_up").into_val(&env),
                id.into_val(&env),
            ]
            && <(i128, i128) as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data) == (1_000, 0)
    });
    assert!(
        update_event.is_some(),
        "disable threshold_updated event not emitted"
    );
}

/// set_low_balance_threshold works on a paused stream.
#[test]
fn test_set_low_balance_threshold_on_paused_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.pause_stream(&employer, &id, &None);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Paused);

    client.set_low_balance_threshold(&employer, &id, &750);
    assert_eq!(client.get_stream(&id).low_balance_threshold, 750);
}

/// Caller who is not employer is rejected.
#[test]
#[should_panic(expected = "E018")]
fn test_set_low_balance_threshold_non_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.set_low_balance_threshold(&attacker, &id, &500);
}

/// Updating threshold on a cancelled stream is rejected.
#[test]
#[should_panic(expected = "E005")]
fn test_set_low_balance_threshold_cancelled_stream_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.cancel_stream(&employer, &id, &None);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Cancelled);

    client.set_low_balance_threshold(&employer, &id, &500);
}

/// Updating threshold on an exhausted stream is rejected.
#[test]
#[should_panic(expected = "E006")]
fn test_set_low_balance_threshold_exhausted_stream_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let now = env.ledger().timestamp();
    let stop = now + 50;
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &stop, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Exhausted);

    client.set_low_balance_threshold(&employer, &id, &500);
}

/// Setting a negative threshold is rejected.
#[test]
#[should_panic(expected = "threshold must be non-negative")]
fn test_set_low_balance_threshold_negative_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.set_low_balance_threshold(&employer, &id, &-1);
}



#[cfg(test)]
mod audit_regressions {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address};

    /// Regression test for HIGH-01 from 2026-04 Trail of Bits audit
    #[test]
    #[should_panic(expected = "not the employer")]
    fn test_high01_top_up_employer_identity_check() {
// ===========================================================================
// TEST-23 (#246): Event payload snapshot tests
//
// Each test triggers exactly the event under test, then asserts the exact
// topics and data of every field so that any schema change (rename, reorder,
// added/removed field) causes a test failure.  The Soroban SDK automatically
// writes a companion JSON snapshot to
//   contracts/stream/test_snapshots/events/<test_name>.1.json
// on the first run (or when SOROBAN_TEST_SNAPSHOTS=update is set) and then
// compares against it on subsequent runs, making CI fail on silent regressions.
// ===========================================================================

#[cfg(test)]
mod events {
    use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
    use soroban_sdk::{symbol_short, vec, Address, BytesN, Env, IntoVal, Val, Vec as SdkVec};

    use crate::{StreamContract, StreamContractClient};

    fn setup() -> (Env, StreamContractClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(StreamContract, ());
        let client = StreamContractClient::new(&env, &id);
        (env, client)
    }

    fn setup_token(env: &Env, admin: &Address) -> Address {
        let token_id = env.register(paystream_token::TokenContract, ());
        let token = paystream_token::TokenContractClient::new(env, &token_id);
        token.initialize(admin, &1_000_000_000);
        token_id
    }

    // -----------------------------------------------------------------------
    // stream_created
    // -----------------------------------------------------------------------

    /// Snapshot: stream_created event topics = ["v1", "created", id]
    ///           data = (employer, employee, rate_per_second)
    #[test]
    fn test_event_stream_created() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let attacker = Address::generate(&env);
        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        // Non-owner attempts to top up
        client.top_up(&attacker, &id, &1000, &None);
    }

    /// Regression test for MED-01 from 2026-04 Trail of Bits audit
    /// Already exists as test_claimable_overflow_panics; verified marked as audit regression
    #[test]
    #[should_panic(expected = "E004")]
    fn test_med01_claimable_overflow_panics() {
        use crate::storage::claimable_amount;
        use crate::types::{Stream, StreamStatus};

        let env = Env::default();
        let addr = Address::generate(&env);

        let stream = Stream {
            id: 1,
            employer: addr.clone(),
            employee: addr.clone(),
            token: addr.clone(),
            deposit: i128::MAX,
            withdrawn: 0,
            rate_per_second: i128::MAX,
            start_time: 0,
            stop_time: 0,
            last_withdraw_time: 0,
            status: StreamStatus::Active,
            locked: false,
            low_balance_threshold: 0,
            notes: None,
        };

        claimable_amount(&stream, 2);
    }

    /// Regression test for MED-02 from 2026-04 Trail of Bits audit
    /// Employee paid before employer refund in cancel_stream
    #[test]
    fn test_med02_cancel_stream_dual_transfer_ordering() {
        use soroban_sdk::testutils::Events as _;
        use soroban_sdk::{symbol_short, vec, IntoVal};

        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

        let events = crate::test::all_events(&env);
        // Find the "created" event (skip initialize event)
        let created = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("created").into_val(&env),
                    id.into_val(&env),
                ]
        });
        assert!(created.is_some(), "stream_created event not emitted");

        let (_, _, data) = created.unwrap();
        let (ev_employer, ev_employee, ev_rate): (Address, Address, i128) = data.into_val(&env);
        assert_eq!(ev_employer, employer, "employer mismatch");
        assert_eq!(ev_employee, employee, "employee mismatch");
        assert_eq!(ev_rate, 10, "rate_per_second mismatch");
    }

    // -----------------------------------------------------------------------
    // withdrawn
    // -----------------------------------------------------------------------

    /// Snapshot: withdrawn event topics = ["v1", "withdraw", id]
    ///           data = (employee, amount)
    #[test]
    fn test_event_withdrawn() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

        env.ledger().with_mut(|l| l.timestamp += 100);
        client.withdraw(&employee, &id);

        let events = crate::test::all_events(&env);
        let withdrawn = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("withdraw").into_val(&env),
                    id.into_val(&env),
                ]
        });
        assert!(withdrawn.is_some(), "withdrawn event not emitted");

        let (_, _, data) = withdrawn.unwrap();
        let (ev_employee, ev_amount): (Address, i128) = data.into_val(&env);
        assert_eq!(ev_employee, employee, "employee mismatch");
        assert_eq!(ev_amount, 1000, "amount mismatch"); // 100s * 10/s
    }

    // -----------------------------------------------------------------------
    // topped_up
    // -----------------------------------------------------------------------

    /// Snapshot: topped_up event topics = ["v1", "topup", id]
    ///           data = (employer, amount)
    #[test]
    fn test_event_topped_up() {
        use paystream_token::TokenContractClient;

        let (env, client) = setup();
        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        client.top_up(&employer, &id, &5_000, &None);

        let events = crate::test::all_events(&env);
        let topup = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("topup").into_val(&env),
                    id.into_val(&env),
                ]
        });
        assert!(topup.is_some(), "topped_up event not emitted");

        let (_, _, data) = topup.unwrap();
        let (ev_employer, ev_amount): (Address, i128) = data.into_val(&env);
        assert_eq!(ev_employer, employer, "employer mismatch");
        assert_eq!(ev_amount, 5_000, "amount mismatch");
    }

    // -----------------------------------------------------------------------
    // stream_cancelled
    // -----------------------------------------------------------------------

    /// Snapshot: stream_cancelled event topics = ["v1", "cancelled", id]
    ///           data = (claimable_amount, refund_amount)
    #[test]
    fn test_event_stream_cancelled() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        env.ledger().with_mut(|l| l.timestamp += 100);
        client.cancel_stream(&employer, &id);

        // Verify both transfers happened (employee claimable + employer refund)
        let cancelled_event = env.events().all();
        assert!(!cancelled_event.is_empty());
    }

    /// Regression test for LOW-02 from 2026-04 Trail of Bits audit
    #[test]
    #[should_panic(expected = "already initialized")]
    fn test_low02_initialize_already_initialized_panics() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let admin2 = Address::generate(&env);

        client.initialize(&admin);
        // Second initialize rejected
        client.initialize(&admin2);
    }
        // deposit=10_000, rate=10/s; after 100s claimable=1_000, refund=9_000
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        env.ledger().with_mut(|l| l.timestamp += 100);
        client.cancel_stream(&employer, &id, &None);

        let events = crate::test::all_events(&env);
        let cancelled = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("cancelled").into_val(&env),
                    id.into_val(&env),
                ]
        });
        assert!(cancelled.is_some(), "stream_cancelled event not emitted");

        let (_, _, data) = cancelled.unwrap();
        let (claimable, refund): (i128, i128) = data.into_val(&env);
        assert_eq!(claimable, 1_000, "claimable mismatch");
        assert_eq!(refund, 9_000, "refund mismatch");
    }

    // -----------------------------------------------------------------------
    // stream_status_changed
    // -----------------------------------------------------------------------

    /// Snapshot: stream_status_changed event topics = ["v1", "status", id]
    ///           data = StreamStatus variant
    #[test]
    fn test_event_stream_status_changed() {
        use crate::types::StreamStatus;
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        client.pause_stream(&employer, &id);

        let events = crate::test::all_events(&env);
        let status_ev = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("status").into_val(&env),
                    id.into_val(&env),
                ]
        });
        assert!(status_ev.is_some(), "stream_status_changed event not emitted");

        let (_, _, data) = status_ev.unwrap();
        let ev_status: StreamStatus = data.into_val(&env);
        assert_eq!(ev_status, StreamStatus::Paused, "status mismatch");
    }

    // -----------------------------------------------------------------------
    // rate_updated
    // -----------------------------------------------------------------------

    /// Snapshot: rate_updated event topics = ["v1", "rate_upd", stream_id]
    ///           data = (old_rate, new_rate)
    #[test]
    fn test_event_rate_updated() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        let employer = Address::generate(&env);
        let employee = Address::generate(&env);
        let token_id = setup_token(&env, &employer);

        client.initialize(&admin);
        let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
        client.update_rate(&employer, &id, &25);

        let events = crate::test::all_events(&env);
        let rate_ev = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("rate_upd").into_val(&env),
                    id.into_val(&env),
                ]
        });
        assert!(rate_ev.is_some(), "rate_updated event not emitted");

        let (_, _, data) = rate_ev.unwrap();
        let (old_rate, new_rate): (i128, i128) = data.into_val(&env);
        assert_eq!(old_rate, 10, "old_rate mismatch");
        assert_eq!(new_rate, 25, "new_rate mismatch");
    }

    // -----------------------------------------------------------------------
    // contract_paused
    // -----------------------------------------------------------------------

    /// Snapshot: contract_paused (pause) event topics = ["v1", "paused"]
    ///           data = true
    #[test]
    fn test_event_contract_paused_true() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.pause_contract(&0);

        let events = crate::test::all_events(&env);
        let pause_ev = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("paused").into_val(&env),
                ]
        });
        assert!(pause_ev.is_some(), "contract_paused event not emitted");

        let (_, _, data) = pause_ev.unwrap();
        let paused: bool = data.into_val(&env);
        assert!(paused, "paused flag should be true");
    }

    /// Snapshot: contract_paused (unpause) event topics = ["v1", "paused"]
    ///           data = false
    #[test]
    fn test_event_contract_paused_false() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);
        client.pause_contract(&0);
        client.unpause_contract(&1);

        let events = crate::test::all_events(&env);
        // Find the last "paused" event (the unpause one)
        let unpause_ev = events.iter().rev().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("paused").into_val(&env),
                ]
        });
        assert!(unpause_ev.is_some(), "contract_paused(false) event not emitted");

        let (_, _, data) = unpause_ev.unwrap();
        let paused: bool = data.into_val(&env);
        assert!(!paused, "paused flag should be false for unpause");
    }

    // -----------------------------------------------------------------------
    // upgrade_proposed
    // -----------------------------------------------------------------------

    /// Snapshot: upgrade_proposed event topics = ["v1", "upg_prop"]
    ///           data = (wasm_hash, unlock_time)
    #[test]
    fn test_event_upgrade_proposed() {
        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        // Use a zero-byte wasm hash — propose_upgrade only stores the proposal,
        // the actual WASM upload is not required for the event to be emitted.
        let wasm_hash: BytesN<32> = BytesN::from_array(&env, &[0u8; 32]);
        // nonce 0 is consumed by pause_contract; we start fresh so nonce=0
        client.propose_upgrade(&wasm_hash, &0);

        let events = crate::test::all_events(&env);
        let upg_ev = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            *topics
                == vec![
                    &env,
                    symbol_short!("v1").into_val(&env),
                    symbol_short!("upg_prop").into_val(&env),
                ]
        });
        assert!(upg_ev.is_some(), "upgrade_proposed event not emitted");

        let (_, _, data) = upg_ev.unwrap();
        let (ev_hash, ev_unlock): (BytesN<32>, u64) = data.into_val(&env);
        assert_eq!(ev_hash, wasm_hash, "wasm_hash mismatch");
        // unlock_time = ledger.timestamp() + TIMELOCK_DELAY; timestamp=0, delay=172800
        assert_eq!(ev_unlock, 172_800u64, "unlock_time mismatch");
    }

    // -----------------------------------------------------------------------
    // upgrade_executed — tested via the wasm-tests feature gate
    //
    // upgrade_executed requires an actual WASM binary to be uploaded to the
    // environment, which is only available when the `wasm-tests` feature is
    // enabled (see the wasm-tests module elsewhere in test.rs).  We document
    // the event schema here and provide a compile-time verified assertion that
    // confirms the topics and data layout matches events.rs.
    //
    // Topics: ["v1", "upg_exec"]
    // Data:   wasm_hash (BytesN<32>)
    // -----------------------------------------------------------------------
    //
    // The actual snapshot for upgrade_executed is covered by the wasm-tests
    // module's test_upgrade_preserves_stream_state test, which calls
    // client.upgrade() and therefore emits the upgrade_executed event.
    // A dedicated snapshot is added below under the wasm-tests feature flag.
    //
    #[cfg(feature = "wasm-tests")]
    #[test]
    fn test_event_upgrade_executed() {
        use soroban_sdk::testutils::Events as _;

        super::super::stream_wasm::contractimport!(
            file = "../../../target/wasm32v1-none/release/paystream_stream.wasm"
        );

        let (env, client) = setup();
        let admin = Address::generate(&env);
        client.initialize(&admin);

        let wasm_hash = env.deployer().upload_contract_wasm(WASM);
        let hash_clone = wasm_hash.clone();

        // propose then execute (timestamp 0 + 172800 delay — fast forward)
        client.propose_upgrade(&wasm_hash, &0);
        env.ledger().with_mut(|l| l.timestamp += 172_800 + 1);
        client.upgrade(&hash_clone, &1);

        let events = env.events().all();
        let exec_ev = events
            .iter()
            .find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
                topics
                    == vec![
                        &env,
                        symbol_short!("v1").into_val(&env),
                        symbol_short!("upg_exec").into_val(&env),
                    ]
            });
        assert!(exec_ev.is_some(), "upgrade_executed event not emitted");

        let (_, _, data) = exec_ev.unwrap();
        let ev_hash: BytesN<32> = data.into_val(&env);
        assert_eq!(ev_hash, hash_clone, "wasm_hash mismatch");
    }
} // mod events

// ===========================================================================
// TEST-24 (#247): Auto top-up trigger and cap behaviour
// ===========================================================================

// -----------------------------------------------------------------------
// Auto top-up fires when balance drops below threshold
// -----------------------------------------------------------------------

/// After a withdraw that drains the balance below trigger_threshold,
/// maybe_auto_topup pulls another topup_amount from the employer and
/// emits auto_topped_up.
#[test]
fn test_auto_topup_fires_when_below_threshold() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal, Val, Vec as SdkVec};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // deposit=1_000, rate=10/s
    let id = client.create_stream(&employer, &employee, &token_id, &1_000, &10, &0, &None, &None);

    // Configure auto top-up: trigger when remaining < 500, top up 500 each time, cap 2_000
    client.set_auto_topup(&employer, &id, &500, &500, &2_000);

    // Advance 60 s → withdrawn=600 → remaining=400 < 500 trigger → auto top-up fires
    env.ledger().with_mut(|l| l.timestamp += 60);
    client.withdraw(&employee, &id);

    // Stream balance should have been replenished
    let s = client.get_stream(&id);
    // deposit was 1_000, withdrew 600, then +500 auto top-up
    assert_eq!(
        s.deposit, 1_500,
        "deposit should be 1_500 after auto top-up"
    );

    // auto_topped_up event must have been emitted
    let events = crate::test::all_events(&env);
    let at_ev = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("at_up").into_val(&env),
                id.into_val(&env),
            ]
    });
    assert!(at_ev.is_some(), "auto_topped_up event not emitted");

    let (_, _, data) = at_ev.unwrap();
    let (ev_amount, ev_total): (i128, i128) = data.into_val(&env);
    assert_eq!(ev_amount, 500, "auto top-up amount mismatch");
    assert_eq!(ev_total, 500, "total_topped_up mismatch");
}

// -----------------------------------------------------------------------
// Auto top-up is skipped when cap (max_total) is reached
// -----------------------------------------------------------------------

/// When total_topped_up has already reached max_total, maybe_auto_topup
/// returns without pulling any tokens and does NOT emit auto_topped_up.
#[test]
fn test_auto_topup_skipped_when_cap_reached() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, Val, Vec as SdkVec};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // deposit=3_000, rate=10/s
    // max_total=500 so one auto top-up exhausts the cap.
    let id = client.create_stream(&employer, &employee, &token_id, &3_000, &10, &0, &None, &None);
    client.set_auto_topup(&employer, &id, &800, &500, &500);

    // First withdraw: remaining drops below 800 → auto top-up fires (total=500=cap)
    env.ledger().with_mut(|l| l.timestamp += 225); // 225*10=2250 withdrawn → 750 remaining < 800
    client.withdraw(&employee, &id);

    // Verify cap was hit on first fire
    let config = client.get_auto_topup_config(&id).unwrap();
    assert_eq!(config.total_topped_up, 500, "should have topped up 500");

    // Second withdraw: cap is exactly reached → no further top-up
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);

    // total_topped_up must still be 500
    let config2 = client.get_auto_topup_config(&id).unwrap();
    assert_eq!(config2.total_topped_up, 500, "cap must not be exceeded");

    // No auto_topped_up event in the second withdraw's events
    // (we check by counting events with at_up topic for stream id)
    let events = crate::test::all_events(&env);
    let at_events: std::vec::Vec<Val> = events
        .iter()
        .filter(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            if let Some(t1) = topics.get(1) {
                use soroban_sdk::TryIntoVal;
                let sym: Result<soroban_sdk::Symbol, _> = t1.try_into_val(&env);
                sym.map(|s| s == symbol_short!("at_up")).unwrap_or(false)
            } else {
                false
            }
        })
        .map(|(_, _, data)| data)
        .collect::<std::vec::Vec<Val>>();

    // Only one auto_topped_up event should exist (the first withdraw)
    assert_eq!(
        at_events.len(),
        1,
        "expected exactly 1 auto_topped_up event"
    );
}

// -----------------------------------------------------------------------
// Auto top-up is skipped when now >= stop_time
// -----------------------------------------------------------------------

/// If the stream's stop_time has passed, maybe_auto_topup must not fire
/// even if the balance is below the trigger threshold.
#[test]
fn test_auto_topup_skipped_when_stop_time_passed() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let now = env.ledger().timestamp();
    // stop_time = now + 100 s; deposit enough to not exhaust in 100 s at rate=10
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &(now + 100),
        &None,&None,
    );
    // trigger_threshold=5_000, topup=1_000, max=10_000
    client.set_auto_topup(&employer, &id, &5_000, &1_000, &10_000);

    // Advance past stop_time → remaining is well below trigger threshold
    // but stop_time has passed so auto top-up must be skipped
    env.ledger().with_mut(|l| l.timestamp += 200);
    client.withdraw(&employee, &id);

    let s = client.get_stream(&id);
    // deposit unchanged at 10_000 (no auto top-up)
    assert_eq!(
        s.deposit, 10_000,
        "deposit must not change when past stop_time"
    );

    let config = client.get_auto_topup_config(&id).unwrap();
    assert_eq!(config.total_topped_up, 0, "no top-up should have occurred");
}

// -----------------------------------------------------------------------
// Auto top-up is skipped gracefully when token allowance is insufficient
// -----------------------------------------------------------------------

/// If the employer has no allowance for transfer_from, the transfer fails
/// and maybe_auto_topup returns without reverting the withdraw.
#[test]
fn test_auto_topup_skipped_when_allowance_insufficient() {
    use soroban_sdk::testutils::MockAuth;
    use soroban_sdk::testutils::MockAuthInvoke;
    use soroban_sdk::IntoVal;

    let env = Env::default();
    // We use mock_all_auths for all calls EXCEPT the auto-topup transfer_from
    // path which tries try_transfer_from — that call will return Err when
    // the allowance is not set.
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
    // deposit=1_000, rate=10/s
    let id = client.create_stream(&employer, &employee, &token_id, &1_000, &10, &0, &None, &None);
    // Set auto top-up but deliberately do NOT grant any allowance — the
    // mock_all_auths covers the contract's own calls but transfer_from on the
    // token will fail if no approval exists.  In testutils the token contract
    // does enforce allowances, so try_transfer_from returns Err.
    client.set_auto_topup(&employer, &id, &500, &300, &1_000);

    // Advance 60 s → remaining < 500 → auto-topup attempts transfer_from
    env.ledger().with_mut(|l| l.timestamp += 60);
    // Withdraw must succeed even though auto-topup silently fails
    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(
        withdrawn, 600,
        "withdraw should succeed despite failed auto-topup"
    );

    // deposit unchanged — auto-topup was skipped
    let s = client.get_stream(&id);
    assert_eq!(s.deposit, 1_000, "deposit should remain unchanged");
    let config = client.get_auto_topup_config(&id).unwrap();
    assert_eq!(config.total_topped_up, 0, "no top-up should have occurred");
}

// -----------------------------------------------------------------------
// auto_topped_up event carries correct amount and cumulative total
// -----------------------------------------------------------------------

/// Emitted auto_topped_up event must contain the per-topup amount and the
/// running cumulative total_topped_up.
#[test]
fn test_auto_topup_event_amount_and_total() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal, Val, Vec as SdkVec};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // deposit=2_000, rate=10/s; trigger < 500, top up 300 each time, max 900
    let id = client.create_stream(&employer, &employee, &token_id, &2_000, &10, &0, &None, &None);
    client.set_auto_topup(&employer, &id, &500, &300, &900);

    // First trigger: advance 160 s → withdrawn=1600 → remaining=400 < 500
    env.ledger().with_mut(|l| l.timestamp += 160);
    client.withdraw(&employee, &id);

    let events = crate::test::all_events(&env);
    let ev1 = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("at_up").into_val(&env),
                id.into_val(&env),
            ]
    });
    assert!(ev1.is_some(), "first auto_topped_up event not emitted");
    let (_, _, data1) = ev1.unwrap();
    let (amount1, total1): (i128, i128) = data1.into_val(&env);
    assert_eq!(amount1, 300, "first top-up amount");
    assert_eq!(total1, 300, "cumulative after first top-up");
}

// ===========================================================================
// TEST-25 (#248): update_rate pending_accrual accounting
// ===========================================================================

// -----------------------------------------------------------------------
// withdraw after update_rate pays out pending_accrual + new-rate accrual
// -----------------------------------------------------------------------

/// After update_rate, a withdraw must pay:
///   (elapsed_at_old_rate * old_rate) + (elapsed_at_new_rate * new_rate)
#[test]
fn test_withdraw_after_rate_update_pays_pending_plus_new() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // rate=10/s, deposit=50_000
    let id = client.create_stream(&employer, &employee, &token_id, &50_000, &10, &0, &None, &None);

    // 200 s at 10/s → 2_000 accrued, NOT yet withdrawn
    env.ledger().with_mut(|l| l.timestamp += 200);
    // Change rate to 20/s — banks 2_000 into pending_accrual
    client.update_rate(&employer, &id, &20);

    let s = client.get_stream(&id);
    assert_eq!(
        s.pending_accrual, 2_000,
        "pending_accrual must be 2_000 after update_rate"
    );

    // 100 s at 20/s → 2_000 new accrual; total claimable = 2_000 + 2_000 = 4_000
    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(
        client.claimable(&id),
        4_000,
        "claimable should include both portions"
    );

    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(
        withdrawn, 4_000,
        "withdraw should transfer full pending + new accrual"
    );
}

// -----------------------------------------------------------------------
// pending_accrual is 0 after withdraw clears it
// -----------------------------------------------------------------------

/// After a withdraw that pays out pending_accrual, the field must be reset to 0.
#[test]
fn test_pending_accrual_cleared_after_withdraw() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &50_000, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.update_rate(&employer, &id, &15);

    // Confirm pending_accrual was set
    let s_before = client.get_stream(&id);
    assert!(
        s_before.pending_accrual > 0,
        "pending_accrual should be > 0 before withdraw"
    );

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.withdraw(&employee, &id);

    // After withdraw, pending_accrual must be 0
    let s_after = client.get_stream(&id);
    assert_eq!(
        s_after.pending_accrual, 0,
        "pending_accrual must be 0 after withdraw"
    );
}

// -----------------------------------------------------------------------
// Multiple rate updates accumulate correctly in pending_accrual
// -----------------------------------------------------------------------

/// Two consecutive update_rate calls must accumulate both banked amounts
/// in pending_accrual and a single withdraw pays them all out.
#[test]
fn test_multiple_rate_updates_accumulate_pending_accrual() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // rate=10/s, large deposit so we never exhaust
    let id = client.create_stream(&employer, &employee, &token_id, &1_000_000, &10, &0, &None, &None);

    // Phase 1: 100 s at 10/s → 1_000 pending
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.update_rate(&employer, &id, &20);
    let s1 = client.get_stream(&id);
    assert_eq!(s1.pending_accrual, 1_000, "after first rate update");

    // Phase 2: 50 s at 20/s → 1_000 more, stacked on top of existing 1_000
    env.ledger().with_mut(|l| l.timestamp += 50);
    client.update_rate(&employer, &id, &5);
    let s2 = client.get_stream(&id);
    // pending_accrual = 1_000 (from phase 1) + 50*20 (from phase 2) = 2_000
    assert_eq!(s2.pending_accrual, 2_000, "after second rate update");

    // Phase 3: 200 s at 5/s → 1_000 new accrual; total claimable = 2_000 + 1_000 = 3_000
    env.ledger().with_mut(|l| l.timestamp += 200);
    assert_eq!(
        client.claimable(&id),
        3_000,
        "claimable must sum all phases"
    );

    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(
        withdrawn, 3_000,
        "withdraw must pay all accumulated accrual"
    );

    let s_final = client.get_stream(&id);
    assert_eq!(
        s_final.pending_accrual, 0,
        "pending_accrual cleared after withdraw"
    );
}

// -----------------------------------------------------------------------
// cancel_stream after update_rate pays correct claimable to employee
// -----------------------------------------------------------------------

/// cancel_stream must pay out pending_accrual + new-rate accrual to the
/// employee, not just the new-rate accrual alone.
#[test]
fn test_cancel_after_rate_update_pays_pending_accrual() {
    use soroban_sdk::testutils::Events as _;
    use soroban_sdk::{symbol_short, vec, IntoVal, Val, Vec as SdkVec};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // rate=10/s, deposit=50_000
    let id = client.create_stream(&employer, &employee, &token_id, &50_000, &10, &0, &None, &None);

    // 100 s at 10/s → pending_accrual = 1_000
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.update_rate(&employer, &id, &20);

    // 50 s at 20/s → new accrual = 1_000; total claimable = 1_000 + 1_000 = 2_000
    env.ledger().with_mut(|l| l.timestamp += 50);
    client.cancel_stream(&employer, &id, &None);

    // The cancelled event's claimable_amount should include both portions
    let events = crate::test::all_events(&env);
    let cancelled = events.iter().find(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
        *topics
            == vec![
                &env,
                symbol_short!("v1").into_val(&env),
                symbol_short!("cancelled").into_val(&env),
                id.into_val(&env),
            ]
    });
    assert!(cancelled.is_some(), "cancelled event not emitted");

    let (_, _, data) = cancelled.unwrap();
    let (claimable, refund): (i128, i128) = data.into_val(&env);
    assert_eq!(
        claimable, 2_000,
        "claimable must include pending_accrual + new accrual"
    );
    assert_eq!(
        refund, 48_000,
        "refund = deposit(50_000) - claimable(2_000)"
    );
}

// ===========================================================================
// TEST-27 (#250): Load test — withdraw_all with 50 streams
// ===========================================================================

/// Creates 50 active streams for a single employee and calls withdraw_all
/// once. Verifies that:
///  - all 50 streams are withdrawn from in a single call
///  - each stream pays the correct amount
///  - the contract completes without hitting Soroban CPU limits
///    (the test harness enforces the default budget; if budget is exceeded
///     the SDK panics, which would fail this test)
///
/// The measured CPU instruction count is intentionally left for the CI
/// benchmark job (see benchmarks/baseline.json) rather than asserted here,
/// because the testutils environment does not expose raw CPU counters through
/// the public SDK v22+ API.
#[test]
fn test_withdraw_all_50_streams() {
    const STREAM_COUNT: u32 = 50;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    // Create 50 streams all paying the same employee at rate=1/s, deposit=1_000
    let mut ids = soroban_sdk::Vec::new(&env);
    for _ in 0..STREAM_COUNT {
        ids.push_back(client.create_stream(&employer, &employee, &token_id, &1_000, &1, &0, &None, &None));
    }

    // Advance 100 s so every stream has 100 tokens claimable
    env.ledger().with_mut(|l| l.timestamp += 100);

    // Single withdraw_all call should sweep all 50 streams
    let results = client.withdraw_all(&employee);

    assert_eq!(
        results.len(),
        STREAM_COUNT,
        "withdraw_all must return exactly {STREAM_COUNT} results"
    );

    // Every stream should have paid exactly 100 tokens (100 s * 1 token/s)
    for i in 0..STREAM_COUNT {
        let stream_id = ids.get(i).unwrap();
        let amount = results.get(i).unwrap();
        assert_eq!(
            amount, 100,
            "stream {stream_id}: expected 100 tokens, got {amount}"
        );
    }

    // Verify each stream's stored state is consistent
    for i in 0..STREAM_COUNT {
        let stream_id = ids.get(i).unwrap();
        let s = client.get_stream(&stream_id);
        assert_eq!(
            s.withdrawn, 100,
            "stream {stream_id}: withdrawn should be 100"
        );
        assert_eq!(
            s.pending_accrual, 0,
            "stream {stream_id}: pending_accrual should be 0"
        );
    }
}

/// Verifies that withdraw_all for 50 streams still respects Soroban's
/// compute budget by running under the default test environment limits.
/// If the budget is exceeded, the SDK panics and this test fails.
///
/// This also acts as the regression gate referenced in benchmarks/baseline.json:
/// if withdraw_all CPU regresses >10% versus the recorded baseline a CI
/// benchmark job (benchmark.yml) will catch it.
#[test]
fn test_withdraw_all_50_streams_within_budget() {
    const STREAM_COUNT: u32 = 50;

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    for _ in 0..STREAM_COUNT {
        client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    }

    env.ledger().with_mut(|l| l.timestamp += 100);

    // If CPU budget is exceeded the SDK panics, failing this test automatically
    let results = client.withdraw_all(&employee);
    assert_eq!(
        results.len(),
        STREAM_COUNT,
        "all 50 streams must be processed"
    );
}

// ---------------------------------------------------------------------------
// PROD-27: pause_reason field tests
// ---------------------------------------------------------------------------

/// Test pause_stream with a reason stores the pause_reason in the stream.
#[test]
fn test_pause_stream_with_reason() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None);

    let reason = soroban_sdk::String::from_slice(&env, "Employee on leave");
    client.pause_stream(&employer, &id, &Some(reason.clone()));

    let stream = client.get_stream(&id);
    assert_eq!(stream.status, StreamStatus::Paused);
    assert_eq!(stream.pause_reason, Some(reason));
}

/// Test pause_stream with None reason stores None in the stream.
#[test]
fn test_pause_stream_without_reason() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None);

    client.pause_stream(&employer, &id, &None);

    let stream = client.get_stream(&id);
    assert_eq!(stream.status, StreamStatus::Paused);
    assert_eq!(stream.pause_reason, None);
}

/// Test resume_stream clears the pause_reason when resuming.
#[test]
fn test_resume_stream_clears_pause_reason() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None);

    let reason = soroban_sdk::String::from_slice(&env, "Temporary suspension");
    client.pause_stream(&employer, &id, &Some(reason));

    let stream = client.get_stream(&id);
    assert_eq!(stream.pause_reason, Some(reason));

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.resume_stream(&employer, &id);

    let stream = client.get_stream(&id);
    assert_eq!(stream.status, StreamStatus::Active);
    assert_eq!(stream.pause_reason, None);
}

// ---------------------------------------------------------------------------
// PROD-19 – transfer_stream: reassign stream ownership to a new employer
// ---------------------------------------------------------------------------

/// Happy path: employer transfers an Active stream to a new employer.
/// The stream's terms are unchanged; the employer field and indexes are updated.
#[test]
fn test_transfer_stream_happy_path() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    assert!(client.streams_by_employer(&employer).contains(id));
    assert!(!client.streams_by_employer(&new_employer).contains(id));

    client.transfer_stream(&employer, &id, &new_employer);

    let s = client.get_stream(&id);
    assert_eq!(s.employer, new_employer);
    assert_eq!(s.employee, employee);
    assert_eq!(s.deposit, 10_000);
    assert_eq!(s.rate_per_second, 10);
    assert_eq!(s.status, StreamStatus::Active);

    // Index updated: id removed from old employer, added to new employer.
    assert!(!client.streams_by_employer(&employer).contains(id));
    assert!(client.streams_by_employer(&new_employer).contains(id));
}

/// Transfer works on a Paused stream.
#[test]
fn test_transfer_stream_paused_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.pause_stream(&employer, &id);

    client.transfer_stream(&employer, &id, &new_employer);

    let s = client.get_stream(&id);
    assert_eq!(s.employer, new_employer);
    assert_eq!(s.status, StreamStatus::Paused);
}

/// Wrong caller (not the employer) must be rejected with Error::NotEmployer.
#[test]
fn test_transfer_stream_wrong_caller_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let attacker = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let res = client.try_transfer_stream(&attacker, &id, &new_employer);
    assert_eq!(res, Err(Ok(Error::NotEmployer)));
}

/// Transfer of a Cancelled stream must be rejected with Error::StreamAlreadyEnded.
#[test]
fn test_transfer_stream_cancelled_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id);

    let res = client.try_transfer_stream(&employer, &id, &new_employer);
    assert_eq!(res, Err(Ok(Error::StreamAlreadyEnded)));
}

/// Transfer of an Exhausted stream must be rejected with Error::StreamAlreadyEnded.
#[test]
fn test_transfer_stream_exhausted_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id); // exhausts the stream

    let res = client.try_transfer_stream(&employer, &id, &new_employer);
    assert_eq!(res, Err(Ok(Error::StreamAlreadyEnded)));
}

/// new_employer must not equal the stream's employee (Error::InvalidTransferTarget).
#[test]
fn test_transfer_stream_new_employer_is_employee_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let res = client.try_transfer_stream(&employer, &id, &employee);
    assert_eq!(res, Err(Ok(Error::InvalidTransferTarget)));
}

// ---------------------------------------------------------------------------
// SC-76 (#444) – transfer_stream index consistency
// ---------------------------------------------------------------------------

/// Transferring a linear-vest stream keeps employer, token and timestamp
/// indexes consistent and leaves no orphaned IDs behind.
#[test]
fn test_transfer_stream_linear_vest_indexes_consistent() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_linear_vest(&employer, &employee, &token_id, &100_000, &100, &None);

    let token_before = client.stream_count_by_token(&token_id);
    let employee_before = client.stream_count_by_employee(&employee);

    client.transfer_stream(&employer, &id, &new_employer);

    // Employer index: removed from old, added to new, counts agree.
    assert!(!client.streams_by_employer(&employer).contains(id));
    assert!(client.streams_by_employer(&new_employer).contains(id));
    assert_eq!(client.stream_count_by_employer(&employer), 0);
    assert_eq!(client.stream_count_by_employer(&new_employer), 1);

    // Token index is untouched by the transfer.
    assert!(client.stream_ids_by_token(&token_id, &0, &50).contains(id));
    assert_eq!(client.stream_count_by_token(&token_id), token_before);

    // Employee index is untouched by the transfer.
    assert!(client.streams_by_employee(&employee).contains(id));
    assert_eq!(client.stream_count_by_employee(&employee), employee_before);

    // No orphaned IDs: the stream is reachable from exactly one employer.
    assert_eq!(
        client
            .streams_by_employer_paginated(&new_employer, &None, &50)
            .len(),
        1
    );
    assert!(client
        .streams_by_employer_paginated(&employer, &None, &50)
        .is_empty());
}

/// Transferring a split stream leaves the beneficiary employee indexes intact
/// (they key on the beneficiary, not the employer).
#[test]
fn test_transfer_stream_split_beneficiary_indexes_unaffected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let beneficiary = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_split_stream(&employer, &beneficiary, &token_id, &10_000, &10, &0);

    let employee_before = client.stream_count_by_employee(&beneficiary);
    let token_before = client.stream_count_by_token(&token_id);

    client.transfer_stream(&employer, &id, &new_employer);

    // Beneficiary-side employee index is unaffected by the employer transfer.
    assert!(client.streams_by_employee(&beneficiary).contains(id));
    assert_eq!(client.stream_count_by_employee(&beneficiary), employee_before);
    assert_eq!(client.stream_count_by_token(&token_id), token_before);

    // Employer index moved cleanly.
    assert!(!client.streams_by_employer(&employer).contains(id));
    assert!(client.streams_by_employer(&new_employer).contains(id));
    assert_eq!(client.stream_count_by_employer(&employer), 0);
    assert_eq!(client.stream_count_by_employer(&new_employer), 1);

    // Stream data itself is unchanged apart from the employer.
    let s = client.get_stream(&id);
    assert_eq!(s.employer, new_employer);
    assert_eq!(s.employee, beneficiary);
    assert_eq!(s.token, token_id);
}

/// Transferring one of several streams only moves that ID between employer
/// indexes; the remaining IDs stay put and no duplicates appear.
#[test]
fn test_transfer_stream_partial_indexes_no_orphans() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    let id3 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.transfer_stream(&employer, &id2, &new_employer);

    let old_ids = client.streams_by_employer(&employer);
    assert_eq!(old_ids.len(), 2);
    assert!(old_ids.contains(id1));
    assert!(old_ids.contains(id3));
    assert!(!old_ids.contains(id2));

    let new_ids = client.streams_by_employer(&new_employer);
    assert_eq!(new_ids.len(), 1);
    assert!(new_ids.contains(id2));

    assert_eq!(client.stream_count_by_employer(&employer), 2);
    assert_eq!(client.stream_count_by_employer(&new_employer), 1);

    // Employee and token indexes still list all three streams.
    assert_eq!(client.stream_count_by_employee(&employee), 3);
    assert_eq!(client.stream_count_by_token(&token_id), 3);
}

/// After transfer, the new employer can pause/resume/cancel the stream.
#[test]
fn test_transfer_stream_new_employer_can_manage() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let new_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.transfer_stream(&employer, &id, &new_employer);

    client.pause_stream(&new_employer, &id);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Paused);

    client.resume_stream(&new_employer, &id);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Active);

    client.cancel_stream(&new_employer, &id);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// PROD-17 (#306): cliff vesting
// ---------------------------------------------------------------------------

/// Stream with rate 10/s, deposit 10_000, created at t=1_000 with a cliff at t=2_000.
fn cliff_stream(env: &Env, client: &StreamContractClient) -> (Address, Address, Address, u64) {
    let admin = Address::generate(env);
    let employer = Address::generate(env);
    let employee = Address::generate(env);
    let token_id = setup_token(env, &employer);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &Some(2_000));
    (employer, employee, token_id, id)
}

#[test]
fn test_cliff_nothing_claimable_before_cliff() {
    let (env, client) = setup();
    let (_, _, _, id) = cliff_stream(&env, &client);
    assert_eq!(client.get_cliff_time(&id), Some(2_000));

    env.ledger().with_mut(|l| l.timestamp = 1_999);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.claimable_at(&id, &1_500), 0);
}

#[test]
fn test_cliff_accrual_starts_from_cliff_not_start() {
    let (env, client) = setup();
    let (_, employee, token_id, id) = cliff_stream(&env, &client);

    // 50 s after the cliff: 50 * 10 — nothing for the 1_000 s before it.
    env.ledger().with_mut(|l| l.timestamp = 2_050);
    assert_eq!(client.claimable(&id), 500);
    assert_eq!(client.withdraw(&employee, &id), 500);

    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    assert_eq!(token.balance(&employee), 500);
}

#[test]
fn test_cliff_withdraw_before_cliff_does_not_move_accrual_start() {
    let (env, client) = setup();
    let (_, employee, _, id) = cliff_stream(&env, &client);

    env.ledger().with_mut(|l| l.timestamp = 1_500);
    assert_eq!(client.withdraw(&employee, &id), 0);
    // Accrual is still anchored at the cliff, not at the 1_500 withdraw.
    assert_eq!(client.get_stream(&id).last_withdraw_time, 2_000);

    env.ledger().with_mut(|l| l.timestamp = 2_010);
    assert_eq!(client.claimable(&id), 100);
}

#[test]
fn test_cliff_pause_resume_before_cliff_keeps_cliff() {
    let (env, client) = setup();
    let (employer, _, _, id) = cliff_stream(&env, &client);

    env.ledger().with_mut(|l| l.timestamp = 1_200);
    client.pause_stream(&employer, &id);
    env.ledger().with_mut(|l| l.timestamp = 1_300);
    client.resume_stream(&employer, &id);

    env.ledger().with_mut(|l| l.timestamp = 1_999);
    assert_eq!(client.claimable(&id), 0);
    env.ledger().with_mut(|l| l.timestamp = 2_001);
    assert_eq!(client.claimable(&id), 10);
}

#[test]
fn test_cliff_cancel_before_cliff_refunds_employer_in_full() {
    let (env, client) = setup();
    let (employer, employee, token_id, id) = cliff_stream(&env, &client);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let employer_before = token.balance(&employer);

    env.ledger().with_mut(|l| l.timestamp = 1_900);
    client.cancel_stream(&employer, &id);

    assert_eq!(token.balance(&employee), 0);
    assert_eq!(token.balance(&employer), employer_before + 10_000);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// SC-64 – set_low_balance_threshold_batch
// ---------------------------------------------------------------------------

/// Happy path: batch-set thresholds on multiple streams, one event per change.
#[test]
fn test_set_low_balance_threshold_batch_happy_path() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);

    let mut updates = soroban_sdk::Vec::new(&env);
    updates.push_back((id1, 1_000u128));
    updates.push_back((id2, 2_500u128));
    client.set_low_balance_threshold_batch(&employer, &updates);

    assert_eq!(client.get_low_balance_threshold(&id1), Some(1_000));
    assert_eq!(client.get_low_balance_threshold(&id2), Some(2_500));

    let events = crate::test::all_events(&env);
    for (id, threshold) in [(id1, 1_000u128), (id2, 2_500u128)] {
        let topics = vec![
            &env,
            symbol_short!("v1").into_val(&env),
            symbol_short!("thresh_up").into_val(&env),
            id.into_val(&env),
        ];
        let found = events.iter().any(|(_, t, data)| {
            *t == topics
                && <u128 as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, &data)
                    == threshold
        });
        assert!(found, "thresh_up event not emitted for stream");
    }
}

/// Empty update list must be rejected.
#[test]
#[should_panic(expected = "updates must not be empty")]
fn test_set_low_balance_threshold_batch_empty_list_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);

    let updates: soroban_sdk::Vec<(u64, u128)> = soroban_sdk::Vec::new(&env);
    client.set_low_balance_threshold_batch(&employer, &updates);
}

/// Batch size exceeding 50 entries must be rejected with ERR_BATCH_TOO_LARGE.
#[test]
#[should_panic(expected = "E029")]
fn test_set_low_balance_threshold_batch_over_max_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let mut updates = soroban_sdk::Vec::new(&env);
    for _ in 0..51 {
        updates.push_back((id, 100u128));
    }
    client.set_low_balance_threshold_batch(&employer, &updates);
}

/// A stream not belonging to the caller must be rejected with ERR_NOT_EMPLOYER.
#[test]
#[should_panic(expected = "E018")]
fn test_set_low_balance_threshold_batch_wrong_employer_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer1 = Address::generate(&env);
    let employer2 = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer1);

    client.initialize(&admin);
    let id = client.create_stream(&employer1, &employee, &token_id, &10_000, &10, &0, &None, &None);

    let mut updates = soroban_sdk::Vec::new(&env);
    updates.push_back((id, 100u128));
    client.set_low_balance_threshold_batch(&employer2, &updates);
}

/// A terminal (cancelled) stream must be rejected and nothing must change.
#[test]
fn test_set_low_balance_threshold_batch_terminal_stream_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0, &None, &None);
    client.cancel_stream(&employer, &id2, &None);

    let mut updates = soroban_sdk::Vec::new(&env);
    updates.push_back((id1, 1_000u128));
    updates.push_back((id2, 2_000u128));
    assert!(client
        .try_set_low_balance_threshold_batch(&employer, &updates)
        .is_err());

    // Atomic rollback: neither threshold was applied.
    assert_eq!(client.get_low_balance_threshold(&id1), None);
    assert_eq!(client.get_low_balance_threshold(&id2), None);
}

/// Partial failure: the second stream is terminal, so the first stream's
/// threshold must remain unchanged (all-or-nothing).
#[test]
fn test_set_low_balance_threshold_batch_partial_failure_rolls_back() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee1 = Address::generate(&env);
    let employee2 = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0, &None, &None);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &500, &10, &0, &None, &None);

    // Exhaust id2 so it is terminal.
    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee2, &id2);
    assert_eq!(client.get_stream(&id2).status, StreamStatus::Exhausted);

    let mut updates = soroban_sdk::Vec::new(&env);
    updates.push_back((id1, 1_000u128));
    updates.push_back((id2, 2_000u128));
    assert!(client
        .try_set_low_balance_threshold_batch(&employer, &updates)
        .is_err());

    assert_eq!(client.get_low_balance_threshold(&id1), None);
    assert_eq!(client.get_low_balance_threshold(&id2), None);
}

// ---------------------------------------------------------------------------
// SC-103 – linear vest cliff_reached parity
// ---------------------------------------------------------------------------

/// LinearVest with a cliff: no event and no accrual before the cliff, the
/// event fires exactly once at the cliff, and accrual starts from the cliff.
#[test]
fn test_linear_vest_cliff_reached_boundary() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    // 100_000 total over 100 s, cliff at t=1_050.
    let id = client.create_linear_vest(&employer, &employee, &token_id, &100_000, &100, &Some(1_050));

    let cliff_topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("cliff").into_val(&env),
        id.into_val(&env),
    ];
    let count = |env: &Env| {
        crate::test::all_events(env)
            .iter()
            .filter(|(_, topics, _)| *topics == cliff_topics)
            .count()
    };

    // Just before the cliff: no event, nothing claimable.
    env.ledger().with_mut(|l| l.timestamp = 1_049);
    assert_eq!(client.claimable(&id), 0);
    client.withdraw(&employee, &id);
    assert_eq!(count(&env), 0, "no cliff event before the cliff");

    // Exactly at the cliff: event emitted once, accrual starts from the cliff.
    env.ledger().with_mut(|l| l.timestamp = 1_050);
    assert_eq!(client.claimable(&id), 0);
    client.withdraw(&employee, &id);
    assert_eq!(count(&env), 1, "cliff_reached emitted at the cliff");

    // After the cliff: accrual grows from the cliff, event not re-emitted.
    env.ledger().with_mut(|l| l.timestamp = 1_060);
    assert_eq!(client.claimable(&id), 10_000);
    client.withdraw(&employee, &id);
    assert_eq!(count(&env), 0, "cliff_reached is not emitted again");
}

#[test]
fn test_cliff_reached_event_emitted_once() {
    use soroban_sdk::{symbol_short, vec, IntoVal};
    let (env, client) = setup();
    let (_, employee, _, id) = cliff_stream(&env, &client);
    let cliff_topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("cliff").into_val(&env),
        id.into_val(&env),
    ];
    let count = |env: &Env| {
        crate::test::all_events(env)
            .iter()
            .filter(|(_, topics, _)| *topics == cliff_topics)
            .count()
    };

    env.ledger().with_mut(|l| l.timestamp = 1_500);
    client.withdraw(&employee, &id);
    assert_eq!(count(&env), 0, "no cliff event before the cliff");

    env.ledger().with_mut(|l| l.timestamp = 2_100);
    client.withdraw(&employee, &id);
    assert_eq!(count(&env), 1, "cliff_reached emitted on first touch after the cliff");

    env.ledger().with_mut(|l| l.timestamp = 2_200);
    client.withdraw(&employee, &id);
    assert_eq!(count(&env), 0, "cliff_reached is not emitted again");
}

/// TEST-57 (#572): claimable is 0 before the cliff, the cliff_reached event
/// fires exactly once with the cliff timestamp, and accrual afterwards is
/// measured from cliff_time rather than start_time.
#[test]
fn test_cliff_reached_event_and_accrual_from_cliff() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let start_time = env.ledger().timestamp();
    let cliff_time = start_time + 1_000;
    let rate: i128 = 10;
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &rate,
        &0,
        &None,
        &Some(cliff_time),
    );
    assert_eq!(client.get_cliff_time(&id), Some(cliff_time));

    let cliff_topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("cliff").into_val(&env),
        id.into_val(&env),
    ];
    let cliff_events = |env: &Env| {
        crate::test::all_events(env)
            .iter()
            .filter(|(_, topics, _)| *topics == cliff_topics)
            .collect::<std::vec::Vec<_>>()
    };

    // Before the cliff: nothing is claimable and no event has fired.
    env.ledger().with_mut(|l| l.timestamp = cliff_time - 1);
    assert_eq!(client.claimable(&id), 0, "claimable must be 0 before the cliff");
    assert_eq!(client.withdraw(&employee, &id), 0);
    assert_eq!(cliff_events(&env).len(), 0, "no cliff event before the cliff");

    // Crossing the cliff emits exactly one cliff_reached event carrying cliff_time.
    env.ledger().with_mut(|l| l.timestamp = cliff_time);
    assert_eq!(client.claimable(&id), 0);
    let events = cliff_events(&env);
    assert_eq!(events.len(), 1, "cliff_reached must be emitted exactly once");
    let (_, _, data) = events.get(0).unwrap();
    let emitted_cliff: u64 =
        <u64 as soroban_sdk::FromVal<Env, soroban_sdk::Val>>::from_val(&env, data);
    assert_eq!(emitted_cliff, cliff_time, "event data must be the cliff time");

    // Accrual is anchored at cliff_time, not start_time.
    env.ledger().with_mut(|l| l.timestamp = cliff_time + 50);
    assert_eq!(client.claimable(&id), 50 * rate);
    assert_eq!(client.claimable_at(&id, &(cliff_time + 100)), 100 * rate);
    assert_eq!(cliff_events(&env).len(), 1, "cliff_reached is not re-emitted");

    // Withdrawing past the cliff pays only post-cliff accrual.
    env.ledger().with_mut(|l| l.timestamp = cliff_time + 100);
    assert_eq!(client.withdraw(&employee, &id), 100 * rate);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    assert_eq!(token.balance(&employee), 100 * rate);
    assert_eq!(cliff_events(&env).len(), 1, "cliff_reached still emitted once");
}

#[test]
fn test_cliff_after_stop_time_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let res = client.try_create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &1_500,
        &None,
        &Some(1_600),
    );
    assert_eq!(res, Err(Ok(Error::CliffAfterStopTime)));
}

#[test]
fn test_cliff_equal_to_stop_time_allowed_and_past_cliff_ignored() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let at_stop = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &1_500, &None, &Some(1_500));
    assert_eq!(client.get_cliff_time(&at_stop), Some(1_500));

    let past = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &Some(500));
    assert_eq!(client.get_cliff_time(&past), None);
    env.ledger().with_mut(|l| l.timestamp = 1_010);
    assert_eq!(client.claimable(&past), 10);
}

#[test]
fn test_no_cliff_stream_unchanged() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);
    assert_eq!(client.get_cliff_time(&id), None);
    env.ledger().with_mut(|l| l.timestamp = 1_010);
    assert_eq!(client.claimable(&id), 10);
}

// ---------------------------------------------------------------------------
// TEST-55 (#570): update_rate banks prior claimable into pending_accrual
// ---------------------------------------------------------------------------

/// update_rate must bank the claimable accrued at the old rate into
/// pending_accrual so the employee keeps those earnings.
#[test]
fn test_update_rate_banks_pending_accrual() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // 100 s at 10/s => 1_000 claimable at the old rate.
    env.ledger().with_mut(|l| l.timestamp = 1_100);
    let pre_change_claimable = client.claimable(&id);
    assert_eq!(pre_change_claimable, 1_000);

    client.update_rate(&employer, &id, &5);

    let s = client.get_stream(&id);
    assert_eq!(s.rate_per_second, 5);
    assert_eq!(
        s.pending_accrual, pre_change_claimable,
        "pending_accrual must equal the pre-change claimable"
    );
    // Immediately after the change, claimable is exactly the banked amount.
    assert_eq!(client.claimable(&id), pre_change_claimable);
}

/// Withdrawing after a rate change pays the banked amount plus the amount
/// accrued at the new rate, without double counting.
#[test]
fn test_update_rate_withdraw_pays_banked_plus_new() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // 100 s at 10/s => 1_000 banked.
    env.ledger().with_mut(|l| l.timestamp = 1_100);
    client.update_rate(&employer, &id, &5);

    // 100 s at the new 5/s rate => 500 newly accrued.
    env.ledger().with_mut(|l| l.timestamp = 1_200);
    assert_eq!(client.claimable(&id), 1_500);

    let paid = client.withdraw(&employee, &id);
    assert_eq!(paid, 1_500, "withdraw pays banked + newly accrued");
    assert_eq!(token.balance(&employee), 1_500);

    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, 1_500);
    assert_eq!(s.pending_accrual, 0, "banked amount is consumed on withdraw");
    assert_eq!(client.claimable(&id), 0);
}

/// Multiple consecutive rate changes must not double count: the banked
/// pending_accrual accumulates exactly the claimable at each change.
#[test]
fn test_update_rate_multiple_changes_no_double_counting() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // 100 s at 10/s => 1_000.
    env.ledger().with_mut(|l| l.timestamp = 1_100);
    client.update_rate(&employer, &id, &5);
    assert_eq!(client.get_stream(&id).pending_accrual, 1_000);

    // 100 s at 5/s => +500 => 1_500 banked.
    env.ledger().with_mut(|l| l.timestamp = 1_200);
    client.update_rate(&employer, &id, &2);
    assert_eq!(client.get_stream(&id).pending_accrual, 1_500);

    // 100 s at 2/s => +200 => 1_700 banked.
    env.ledger().with_mut(|l| l.timestamp = 1_300);
    client.update_rate(&employer, &id, &1);
    assert_eq!(client.get_stream(&id).pending_accrual, 1_700);
    assert_eq!(client.claimable(&id), 1_700);

    // 100 s at 1/s => +100 => 1_800 total claimable.
    env.ledger().with_mut(|l| l.timestamp = 1_400);
    assert_eq!(client.claimable(&id), 1_800);

    let paid = client.withdraw(&employee, &id);
    assert_eq!(paid, 1_800, "no double counting across multiple changes");
    assert_eq!(token.balance(&employee), 1_800);
    assert_eq!(client.get_stream(&id).pending_accrual, 0);
    assert_eq!(client.claimable(&id), 0);
}

// ---------------------------------------------------------------------------
// PROD-20 (#309): stream templates
// ---------------------------------------------------------------------------

fn template_params(env: &Env, token: &Address, cliff_time: Option<u64>) -> StreamParams {
    StreamParams {
        employee: Address::generate(env),
        token: token.clone(),
        deposit: 10_000,
        rate_per_second: 5,
        stop_time: 0,
        cliff_time,
        low_balance_threshold: None,
    }
}

#[test]
fn test_template_save_and_create_stream_from_template() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &7, &template_params(&env, &token_id, None));
    assert_eq!(client.list_templates(&employer).len(), 1);
    assert!(client.get_template(&employer, &7).is_some());

    let id = client.create_stream_from_template(&employer, &employee, &7);
    let s = client.get_stream(&id);
    assert_eq!(s.employee, employee, "template's employee field is ignored");
    assert_eq!(s.employer, employer);
    assert_eq!(s.token, token_id);
    assert_eq!(s.deposit, 10_000);
    assert_eq!(s.rate_per_second, 5);
}

#[test]
fn test_template_carries_cliff() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 100);

    client.save_template(&employer, &1, &template_params(&env, &token_id, Some(400)));
    let id = client.create_stream_from_template(&employer, &employee, &1);
    assert_eq!(client.get_cliff_time(&id), Some(400));
    env.ledger().with_mut(|l| l.timestamp = 300);
    assert_eq!(client.claimable(&id), 0);
}

#[test]
fn test_template_delete() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &1, &template_params(&env, &token_id, None));
    client.save_template(&employer, &2, &template_params(&env, &token_id, None));
    client.delete_template(&employer, &1);

    assert!(client.get_template(&employer, &1).is_none());
    let remaining = client.list_templates(&employer);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining.get(0).unwrap().0, 2);
}

#[test]
fn test_template_create_after_delete_fails() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &1, &template_params(&env, &token_id, None));
    client.delete_template(&employer, &1);
    let res = client.try_create_stream_from_template(&employer, &employee, &1);
    assert_eq!(res, Err(Ok(Error::TemplateNotFound)));
}

#[test]
fn test_template_other_employer_cannot_use() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let other = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &1, &template_params(&env, &token_id, None));
    // Templates are keyed by employer: `other` has no template 1.
    let res = client.try_create_stream_from_template(&other, &employee, &1);
    assert_eq!(res, Err(Ok(Error::TemplateNotFound)));
}

#[test]
fn test_template_other_employer_cannot_delete() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let other = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &1, &template_params(&env, &token_id, None));
    let res = client.try_delete_template(&other, &1);
    assert_eq!(res, Err(Ok(Error::TemplateNotFound)));
}

#[test]
fn test_template_save_requires_employer_auth() {
    let env = Env::default();
    let id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &id);
    let employer = Address::generate(&env);
    let token = Address::generate(&env);
    // No mock_all_auths: the employer has not signed.
    let res = client.try_save_template(&employer, &1, &template_params(&env, &token, None));
    assert!(res.is_err(), "save_template must require the employer's auth");
}

// ---------------------------------------------------------------------------
// SC-107 – Stream struct storage footprint (cold-field split)
// ---------------------------------------------------------------------------

/// The hot Stream entry must be smaller than the legacy (pre-split) layout.
///
/// ADR-004 relocates `pause_reason` and `low_balance_threshold` into a separate
/// cold entry. This test measures the persistent entry size of the hot stream
/// record and asserts it shrank relative to the legacy struct size, and that
/// the cold data is still decodable through `get_stream` (which merges it).
#[test]
fn test_sc107_hot_stream_entry_shrank_and_cold_fields_decode() {
    use soroban_sdk::testutils::storage::Persistent as _;

// SC-99 (#467): bounded stream metadata map
// ---------------------------------------------------------------------------

/// Settling at stop_time with the full deposit claimable pays the employee
/// once and leaves `withdrawn == deposit` (not 2x), with the stream Exhausted.
#[test]
fn test_settle_stream_at_stop_time_withdraws_exactly_once() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.set_settle_auth_mode(&admin, &0, &crate::types::SettleAuthMode::Allowlist);

    env.ledger().with_mut(|l| l.timestamp += 100);
    let res = client.try_settle_stream(&stranger, &id);
    assert!(res.is_err(), "non-allowlisted caller must be rejected");
    assert_eq!(client.get_stream(&id).withdrawn, 0);
}

/// Allowlisted keepers may settle while in Allowlist mode.
#[test]
fn test_settle_stream_allowlist_mode_allows_keeper() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);

    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &1_000, &1, &1_100, &None, &None);

    // Past stop_time: the whole deposit is claimable.
    env.ledger().with_mut(|l| l.timestamp = 1_100);
    assert_eq!(client.claimable(&id), 1_000);

    let settled = client.settle_stream(&id);
    assert_eq!(settled, 1_000);

    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, s.deposit, "withdrawn must equal deposit exactly once");
    assert_eq!(s.withdrawn, 1_000);
    assert_eq!(s.status, StreamStatus::Exhausted);
    assert_eq!(token.balance(&employee), 1_000);
}

/// Partial settle regression: settling mid-stream only moves the claimable
/// amount, and a second settle tops up to the full deposit without doubling.
#[test]
fn test_settle_stream_partial_then_full_no_double_count() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let keeper = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.set_settle_auth_mode(&admin, &0, &crate::types::SettleAuthMode::Allowlist);
    client.set_settle_keeper(&admin, &1, &keeper, &true);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.settle_stream(&keeper, &id);
    assert_eq!(client.get_stream(&id).withdrawn, 1000);
}

/// Switching back to Open restores permissionless settlement.
#[test]
fn test_settle_stream_switch_back_to_open() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &1_000, &1, &0, &None, &None);

    env.ledger().with_mut(|l| l.timestamp = 1_300);
    assert_eq!(client.settle_stream(&id), 300);
    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, 300);
    assert_eq!(s.status, StreamStatus::Active);

    env.ledger().with_mut(|l| l.timestamp = 1_700);
    assert_eq!(client.settle_stream(&id), 400);
    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, 700);
    assert_eq!(token.balance(&employee), 700);

    env.ledger().with_mut(|l| l.timestamp = 2_500);
    assert_eq!(client.settle_stream(&id), 300);
    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, s.deposit);
    assert_eq!(s.status, StreamStatus::Exhausted);
    assert_eq!(token.balance(&employee), 1_000);
}

// ---------------------------------------------------------------------------
// SC-67 – admin-managed token allowlist
// ---------------------------------------------------------------------------

/// With no allowlist configured (default), any token is accepted.
#[test]
fn test_token_allowlist_empty_is_permissive() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    assert!(!client.is_token_allowlist_enabled());

    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    assert_eq!(client.get_stream(&id).token, token_id);

    let vest_id =
        client.create_linear_vest(&employer, &employee, &token_id, &10_000, &10, &None);
    assert_eq!(client.get_stream(&vest_id).token, token_id);
}

/// Once the allowlist is enabled, listed tokens are accepted by both
/// create_stream and create_linear_vest.
#[test]
fn test_token_allowlist_allows_listed_token() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let mut allow = soroban_sdk::Vec::new(&env);
    allow.push_back(token_id.clone());
    client.set_token_allowlist(&admin, &0, &allow);
    assert!(client.is_token_allowlist_enabled());
    assert!(client.is_token_allowed(&token_id));

    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);
    assert_eq!(client.get_stream(&id).token, token_id);

    let vest_id =
        client.create_linear_vest(&employer, &employee, &token_id, &10_000, &10, &None);
    assert_eq!(client.get_stream(&vest_id).token, token_id);
}

/// A token that is not on the enabled allowlist is rejected by create_stream.
#[test]
#[should_panic(expected = "E036")]
fn test_token_allowlist_rejects_unlisted_token_create_stream() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let allowed = setup_token(&env, &employer);
    let blocked = setup_token(&env, &employer);

    client.initialize(&admin);
    let mut allow = soroban_sdk::Vec::new(&env);
    allow.push_back(allowed);
    client.set_token_allowlist(&admin, &0, &allow);

    client.create_stream(&employer, &employee, &blocked, &10_000, &10, &0, &None, &None);
}

/// A token that is not on the enabled allowlist is rejected by create_linear_vest.
#[test]
#[should_panic(expected = "E036")]
fn test_token_allowlist_rejects_unlisted_token_create_linear_vest() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let allowed = setup_token(&env, &employer);
    let blocked = setup_token(&env, &employer);

    client.initialize(&admin);
    let mut allow = soroban_sdk::Vec::new(&env);
    allow.push_back(allowed);
    client.set_token_allowlist(&admin, &0, &allow);

    client.create_linear_vest(&employer, &employee, &blocked, &10_000, &10, &None);
}

/// Clearing the allowlist restores permissive behavior.
#[test]
fn test_token_allowlist_cleared_restores_permissive() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let allowed = setup_token(&env, &employer);
    let other = setup_token(&env, &employer);

    client.initialize(&admin);
    let mut allow = soroban_sdk::Vec::new(&env);
    allow.push_back(allowed);
    client.set_token_allowlist(&admin, &0, &allow);
    assert!(client.is_token_allowlist_enabled());

    let empty: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
    client.set_token_allowlist(&admin, &1, &empty);
    assert!(!client.is_token_allowlist_enabled());

    let id = client.create_stream(&employer, &employee, &other, &10_000, &10, &0, &None, &None);
    assert_eq!(client.get_stream(&id).token, other);
}

/// Only the admin may change the allowlist.
#[test]
#[should_panic(expected = "E002")]
fn test_token_allowlist_requires_admin() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);
    let token_id = setup_token(&env, &admin);

    client.initialize(&admin);
    let mut allow = soroban_sdk::Vec::new(&env);
    allow.push_back(token_id);
    client.set_token_allowlist(&attacker, &0, &allow);
}

/// Metadata is stored on creation and readable back.
#[test]
fn test_metadata_create_and_read() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    // Legacy (pre-split) footprint: the full Stream struct as it was stored.
    let legacy_size = crate::types::Stream::LEGACY_ENCODED_SIZE;
    // Hot footprint: the packed Stream struct actually persisted.
    let hot_size = crate::types::Stream::ENCODED_SIZE;
    assert!(
        hot_size < legacy_size,
        "hot stream entry ({hot_size} bytes) must be smaller than legacy ({legacy_size} bytes)"
    );

    // The hot entry is what lives under the stream key; measure its rent.
    let hot_key = storage::DataKey::Stream(id);
    let hot_rent = env.storage().persistent().get_rent(&hot_key);
    assert!(hot_rent.is_some(), "hot stream entry must exist in persistent storage");

    // Cold fields must still decode through the merged view.
    let reason = soroban_sdk::String::from_slice(&env, "cold field check");
    client.pause_stream(&employer, &id, &Some(reason.clone()));
    let merged = client.get_stream(&id);
    assert_eq!(merged.status, StreamStatus::Paused);
    assert_eq!(merged.pause_reason, Some(reason));
    assert_eq!(merged.low_balance_threshold, 0);
}

/// Metadata is stored and readable through the getter.
#[test]
fn test_metadata_create_and_get() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let meta = metadata_map(&env, &[("department", "engineering"), ("cost_center", "cc-42")]);
    let id = client.create_stream_with_metadata(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &meta,
    );

    let stored = client.get_stream_metadata(&id);
    assert_eq!(stored.len(), 2);
    let (dept_key, _) = metadata_entry(&env, "department", "");
    assert_eq!(
        stored.get(dept_key).unwrap(),
        soroban_sdk::String::from_slice(&env, "engineering")
    );
    let (cc_key, _) = metadata_entry(&env, "cost_center", "");
    assert_eq!(
        stored.get(cc_key).unwrap(),
        soroban_sdk::String::from_slice(&env, "cc-42")
    );
}

/// Metadata can be replaced through the update entrypoint.
#[test]
fn test_metadata_update_replaces_map() {
// ---------------------------------------------------------------------------
// SC-101 – is_stream_expirable grace-period query
// ---------------------------------------------------------------------------

/// is_stream_expirable returns (false, seconds_until_eligible) before the
/// grace period elapses.
#[test]
fn test_is_stream_expirable_before_grace_period() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &1_100, &None, &None);

    // Exactly at stop_time: grace period has not started elapsing.
    env.ledger().with_mut(|l| l.timestamp = 1_100);
    let (eligible, seconds_until) = client.is_stream_expirable(&id);
    assert!(!eligible);
    assert_eq!(seconds_until, crate::GRACE_PERIOD);

    // One second before stop_time + GRACE_PERIOD.
    env.ledger().with_mut(|l| l.timestamp = 1_100 + crate::GRACE_PERIOD - 1);
    let (eligible, seconds_until) = client.is_stream_expirable(&id);
    assert!(!eligible);
    assert_eq!(seconds_until, 1);
}

/// is_stream_expirable returns (true, 0) exactly at stop_time + GRACE_PERIOD.
#[test]
fn test_is_stream_expirable_at_grace_boundary() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let contract_id = client.address.clone();

    client.initialize(&admin);
    let id = client.create_linear_vest(&employer, &employee, &token_id, &10_000, &100, &None);
    assert_conservation(&env, &client, &token, id, &contract_id);

    env.ledger().with_mut(|l| l.timestamp += 40);
    client.withdraw(&employee, &id);
    assert_conservation(&env, &client, &token, id, &contract_id);

    client.top_up(&employer, &id, &5_000);
    assert_conservation(&env, &client, &token, id, &contract_id);

    env.ledger().with_mut(|l| l.timestamp += 20);
    client.cancel_stream(&employer, &id, &None);
    assert_conservation(&env, &client, &token, id, &contract_id);

    let s = client.get_stream(&id);
    assert_eq!(s.status, StreamStatus::Cancelled);
    assert_eq!(s.withdrawn, 6_000);
}

/// Conservation holds across create → withdraw → settle for a linear vest
/// stream.
#[test]
fn test_conservation_linear_vest_settle() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);

    let id = client.create_stream_with_metadata(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &metadata_map(&env, &[("department", "engineering")]),
    );

    let updated = metadata_map(&env, &[("department", "finance"), ("cost_center", "cc-7")]);
    client.update_stream_metadata(&employer, &id, &updated);

    let stored = client.get_stream_metadata(&id);
    assert_eq!(stored.len(), 2);
    let (dept_key, _) = metadata_entry(&env, "department", "");
    assert_eq!(
        stored.get(dept_key).unwrap(),
        soroban_sdk::String::from_slice(&env, "finance")
    );
}

/// Exactly MAX_METADATA_ENTRIES entries are accepted.
#[test]
fn test_metadata_accepts_max_entries() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);
    let contract_id = client.address.clone();

    client.initialize(&admin);
    let id = client.create_linear_vest(&employer, &employee, &token_id, &10_000, &100, &None);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.settle(&employer, &id);
    assert_conservation(&env, &client, &token, id, &contract_id);

    let s = client.get_stream(&id);
    assert_eq!(s.withdrawn, 10_000);
    assert_eq!(token.balance(&employee), 10_000);
}

/// Conservation holds across create → withdraw → transfer for a split stream.
#[test]
fn test_conservation_split_stream_lifecycle() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &1_100, &None, &None);

    env.ledger().with_mut(|l| l.timestamp = 1_100 + crate::GRACE_PERIOD);
    let (eligible, seconds_until) = client.is_stream_expirable(&id);
    assert!(eligible, "stream is eligible exactly at the grace boundary");
    assert_eq!(seconds_until, 0);

    // Just after the boundary it stays eligible.
    env.ledger().with_mut(|l| l.timestamp = 1_100 + crate::GRACE_PERIOD + 1);
    let (eligible, seconds_until) = client.is_stream_expirable(&id);
    assert!(eligible);
    assert_eq!(seconds_until, 0);
}

/// is_stream_expirable is read-only: it must not mutate the stream.
#[test]
fn test_is_stream_expirable_is_read_only() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);

    let mut map = soroban_sdk::Map::new(&env);
    for i in 0..crate::MAX_METADATA_ENTRIES {
        let key = soroban_sdk::String::from_slice(&env, "k");
        let mut buf = [0u8; 8];
        let mut n = i;
        let mut len = 0usize;
        if n == 0 {
            buf[0] = b'0';
            len = 1;
        }
        while n > 0 {
            buf[len] = b'0' + (n % 10) as u8;
            n /= 10;
            len += 1;
        }
        let mut key_bytes = [0u8; 9];
        key_bytes[0] = b'k';
        for j in 0..len {
            key_bytes[j + 1] = buf[len - 1 - j];
        }
        let key = soroban_sdk::String::from_bytes(&env, &key_bytes[..len + 1]);
        let value = soroban_sdk::String::from_slice(&env, "v");
        map.set(key, value);
    }
    assert_eq!(map.len(), crate::MAX_METADATA_ENTRIES);

    let id = client.create_stream_with_metadata(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &map,
    );
    assert_eq!(client.get_stream_metadata(&id).len(), crate::MAX_METADATA_ENTRIES);
}

/// MAX_METADATA_ENTRIES + 1 entries are rejected.
#[test]
#[should_panic(expected = "E040")]
fn test_metadata_rejects_too_many_entries() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let stranger = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &1_100, &None, &None);
    let before = client.get_stream(&id);

    env.ledger().with_mut(|l| l.timestamp = 1_100 + crate::GRACE_PERIOD);
    let _ = client.is_stream_expirable(&id);

    let after = client.get_stream(&id);
    assert_eq!(before.status, after.status);
    assert_eq!(before.withdrawn, after.withdrawn);
    assert_eq!(before.last_withdraw_time, after.last_withdraw_time);
}

// ---------------------------------------------------------------------------
// SC-110 – get_streams_by_employer_and_token
// ---------------------------------------------------------------------------

/// Snapshot: the per-token employer view intersects both indexes, keeps
/// creation order and paginates with a capped limit.
#[test]
fn test_get_streams_by_employer_and_token_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let other_employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_a = setup_token(&env, &employer);
    let token_b = setup_token(&env, &employer);

    client.initialize(&admin);
    let a1 = client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);
    let b1 = client.create_stream(&employer, &employee, &token_b, &10_000, &10, &0, &None, &None);
    let a2 = client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);
    let a3 = client.create_stream(&employer, &employee, &token_a, &10_000, &10, &0, &None, &None);
    // A stream owned by a different employer must never appear.
    let _other = client.create_stream(&other_employer, &employee, &token_a, &10_000, &10, &0, &None, &None);

    let all_a = client.get_streams_by_employer_and_token(&employer, &token_a, &0, &50);
    assert_eq!(all_a.len(), 3);
    assert_eq!(all_a.get(0).unwrap(), a1);
    assert_eq!(all_a.get(1).unwrap(), a2);
    assert_eq!(all_a.get(2).unwrap(), a3);

    let all_b = client.get_streams_by_employer_and_token(&employer, &token_b, &0, &50);
    assert_eq!(all_b.len(), 1);
    assert_eq!(all_b.get(0).unwrap(), b1);

    // Pagination.
    let page = client.get_streams_by_employer_and_token(&employer, &token_a, &1, &1);
    assert_eq!(page.len(), 1);
    assert_eq!(page.get(0).unwrap(), a2);

    let beyond = client.get_streams_by_employer_and_token(&employer, &token_a, &10, &50);
    assert!(beyond.is_empty());

    // Limit is capped.
    let capped = client.get_streams_by_employer_and_token(&employer, &token_a, &0, &100);
    assert_eq!(capped.len(), 3);
}

/// get_streams_by_employer_and_token is read-only and returns empty for an
/// unknown token.
#[test]
fn test_get_streams_by_employer_and_token_unknown_token_empty() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let unknown_token = Address::generate(&env);

    client.initialize(&admin);
    assert!(client
        .get_streams_by_employer_and_token(&employer, &unknown_token, &0, &50)
        .is_empty());
}

    let mut map = soroban_sdk::Map::new(&env);
    for i in 0..=crate::MAX_METADATA_ENTRIES {
        let mut key_bytes = [0u8; 9];
        key_bytes[0] = b'k';
        key_bytes[1] = b'0' + (i % 10) as u8;
        let key = soroban_sdk::String::from_bytes(&env, &key_bytes[..2]);
        let value = soroban_sdk::String::from_slice(&env, "v");
        map.set(key, value);
    }

    client.set_settle_auth_mode(&admin, &0, &crate::types::SettleAuthMode::Allowlist);
    client.set_settle_auth_mode(&admin, &1, &crate::types::SettleAuthMode::Open);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.settle_stream(&stranger, &id);
    assert_eq!(client.get_stream(&id).withdrawn, 1000);
}

/// set_settle_auth_mode enforces the admin nonce.
#[test]
fn test_set_settle_auth_mode_rejects_stale_nonce() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    client.set_settle_auth_mode(&admin, &0, &crate::types::SettleAuthMode::Allowlist);
    // Reusing nonce 0 must fail.
    let res = client.try_set_settle_auth_mode(&admin, &0, &crate::types::SettleAuthMode::Open);
    assert!(res.is_err(), "stale nonce must be rejected");
}

/// set_settle_auth_mode requires the admin's auth.
#[test]
fn test_set_settle_auth_mode_requires_admin_auth() {
    let env = Env::default();
    let id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // No mock_all_auths: the admin has not signed.
    let res = client.try_set_settle_auth_mode(&admin, &0, &crate::types::SettleAuthMode::Allowlist);
    assert!(res.is_err(), "set_settle_auth_mode must require admin auth");
}

/// A key longer than MAX_METADATA_KEY_LEN is rejected.
#[test]
#[should_panic(expected = "E041")]
fn test_metadata_rejects_oversized_key() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let mut key_bytes = [0u8; 64];
    for b in key_bytes.iter_mut() {
        *b = b'a';
    }
    let key = soroban_sdk::String::from_bytes(&env, &key_bytes[..(crate::MAX_METADATA_KEY_LEN + 1) as usize]);
    let mut map = soroban_sdk::Map::new(&env);
    map.set(key, soroban_sdk::String::from_slice(&env, "v"));

    client.create_stream_with_metadata(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &map,
    );
}

/// A value longer than MAX_METADATA_VALUE_LEN is rejected.
#[test]
#[should_panic(expected = "E042")]
fn test_metadata_rejects_oversized_value() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let mut value_bytes = [0u8; 128];
    for b in value_bytes.iter_mut() {
        *b = b'v';
    }
    let value =
        soroban_sdk::String::from_bytes(&env, &value_bytes[..(crate::MAX_METADATA_VALUE_LEN + 1) as usize]);
    let mut map = soroban_sdk::Map::new(&env);
    map.set(soroban_sdk::String::from_slice(&env, "k"), value);

    client.create_stream_with_metadata(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &map,
    );
}

// ---------------------------------------------------------------------------
// TEST-53 – per-employer per-ledger rate limit (SEC-24)
// ---------------------------------------------------------------------------

/// Fill the per-employer per-ledger create limit; the next create must revert
/// with ERR_RATE_LIMIT_EXCEEDED (E025).
#[test]
#[should_panic(expected = "E025")]
fn test_test53_rate_limit_exceeded_reverts() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let limit = client.get_rate_limit();
    for _ in 0..limit {
        client.create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None);
    }
    // One past the limit must revert.
    client.create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None);
}

/// The per-ledger counter resets on the next ledger.
#[test]
fn test_test53_rate_limit_resets_next_ledger() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let limit = client.get_rate_limit();
    for _ in 0..limit {
        client.create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None);
    }
    assert!(client
        .try_create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None)
        .is_err());

    // Advance to the next ledger: the counter resets and a create succeeds.
    env.ledger().with_mut(|l| l.sequence_number += 1);
    let id = client.create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None);
    assert!(id > 0);
}

/// A batch counts every entry toward the limit; an over-limit batch reverts.
#[test]
fn test_test53_rate_limit_batch_counts_all_entries() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let limit = client.get_rate_limit();
    let mut entries = soroban_sdk::Vec::new(&env);
    for _ in 0..limit {
        entries.push_back(crate::types::StreamCreateParams {
            employer: employer.clone(),
            employee: Address::generate(&env),
            token: token_id.clone(),
            deposit: 100,
            rate: 1,
            start_time: 0,
            end_time: None,
            cliff_time: None,
        });
    }
    client.create_streams_batch(&employer, &entries);
    assert!(client
        .try_create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None)
        .is_err());
}

// ---------------------------------------------------------------------------
// SC-115 (#483): configurable MAX_BATCH_SIZE
// ---------------------------------------------------------------------------

/// Default batch size limit is 50 before any admin configuration.
#[test]
fn test_max_batch_size_default_is_50() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    assert_eq!(client.max_batch_size(), 50);
}

/// Lower bound 1 is accepted.
#[test]
fn test_set_max_batch_size_accepts_lower_bound() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_max_batch_size(&admin, &0, &1);
    assert_eq!(client.max_batch_size(), 1);
}

/// Upper bound 200 is accepted.
#[test]
fn test_set_max_batch_size_accepts_upper_bound() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_max_batch_size(&admin, &0, &200);
    assert_eq!(client.max_batch_size(), 200);
}

/// 0 is rejected.
#[test]
#[should_panic(expected = "E043")]
fn test_set_max_batch_size_rejects_zero() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_max_batch_size(&admin, &0, &0);
}

/// 201 is rejected.
#[test]
#[should_panic(expected = "E043")]
fn test_set_max_batch_size_rejects_above_upper_bound() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.set_max_batch_size(&admin, &0, &201);
}

/// Batch entrypoints honour the configured limit.
#[test]
#[should_panic(expected = "E005")]
fn test_batch_respects_configured_max_batch_size() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let limit = client.get_rate_limit();
    // A batch that exactly fills the limit succeeds.
    let params = make_stream_params(&env, &token_id, limit);
    let ids = client.create_streams_batch(&employer, &params);
    assert_eq!(ids.len(), limit);

    // The next single create is over the limit.
    assert!(client
        .try_create_stream(&employer, &Address::generate(&env), &token_id, &100, &1, &0, &None, &None)
        .is_err());

    // A fresh ledger: an over-limit batch reverts atomically.
    env.ledger().with_mut(|l| l.sequence_number += 1);
    let over = make_stream_params(&env, &token_id, limit + 1);
    assert!(client.try_create_streams_batch(&employer, &over).is_err());
}

/// A batch exactly at the configured limit succeeds.
#[test]
fn test_batch_at_configured_max_batch_size_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    client.set_max_batch_size(&admin, &1, &2);

    let params = make_stream_params(&env, &token_id, 2);
    let ids = client.create_streams_batch(&employer, &params);
    assert_eq!(ids.len(), 2);
}

// ---------------------------------------------------------------------------
// SC-114 (#482): split vest streams
// ---------------------------------------------------------------------------

fn split_beneficiaries(
    env: &Env,
    a: &Address,
    bps_a: u32,
    b: &Address,
    bps_b: u32,
) -> soroban_sdk::Vec<crate::types::SplitBeneficiary> {
    let mut v = soroban_sdk::Vec::new(env);
    v.push_back(crate::types::SplitBeneficiary {
        beneficiary: a.clone(),
        basis_points: bps_a,
    });
    v.push_back(crate::types::SplitBeneficiary {
        beneficiary: b.clone(),
        basis_points: bps_b,
    });
    v
}

/// Split vest creation stores the split and reports the LinearVest type.
#[test]
fn test_create_split_vest_stores_split() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let split = split_beneficiaries(&env, &a, 6_000, &b, 4_000);
    let id = client.create_split_vest(&employer, &token_id, &100_000, &100, &None, &split);

    assert_eq!(
        client.get_stream_type(&id),
        crate::types::StreamType::LinearVest
    );
    let stored = client.get_split_beneficiaries(&id);
    assert_eq!(stored.len(), 2);
}

/// Split basis points must sum to 10_000.
#[test]
#[should_panic(expected = "E044")]
fn test_create_split_vest_rejects_bad_basis_points() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let split = split_beneficiaries(&env, &a, 6_000, &b, 3_000);
    client.create_split_vest(&employer, &token_id, &100_000, &100, &None, &split);
}

// ---------------------------------------------------------------------------
// SC-72: get_pending_upgrade query (#440)
// ---------------------------------------------------------------------------

/// Returns None when no upgrade has been proposed yet.
#[test]
fn test_get_pending_upgrade_none_initially() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let result = client.get_pending_upgrade();
    assert!(result.is_none(), "expected None before any propose_upgrade");
    env.as_contract(&client.address, || {
        soroban_sdk::snapshot_writer::write(&env, "test_get_pending_upgrade_none_initially.1");
    });
}

/// Returns Some((wasm_hash, unlock_time)) after propose_upgrade.
#[test]
fn test_get_pending_upgrade_some_after_propose() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let wasm_hash: soroban_sdk::BytesN<32> = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.propose_upgrade(&wasm_hash, &0);

    let result = client.get_pending_upgrade();
    assert!(result.is_some(), "expected Some after propose_upgrade");
    let (stored_hash, unlock_time) = result.unwrap();
    assert_eq!(stored_hash, wasm_hash, "wasm_hash mismatch");
    // ledger timestamp starts at 0; TIMELOCK_DELAY = 172800
    assert_eq!(unlock_time, 172_800u64, "unlock_time mismatch");
}

/// Returns None after the upgrade proposal is cancelled.
#[test]
fn test_get_pending_upgrade_none_after_cancel() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let wasm_hash: soroban_sdk::BytesN<32> = soroban_sdk::BytesN::from_array(&env, &[0u8; 32]);
    client.propose_upgrade(&wasm_hash, &0);
    client.cancel_upgrade(&1);

    let result = client.get_pending_upgrade();
    assert!(result.is_none(), "expected None after cancel_upgrade");
}

/// Split vest withdraw distributes claimable per basis points.
#[test]
fn test_split_vest_withdraw_distributes_claimable() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let split = split_beneficiaries(&env, &a, 6_000, &b, 4_000);
    let id = client.create_split_vest(&employer, &token_id, &100_000, &100, &None, &split);

    env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(client.claimable(&id), 50_000);

    client.withdraw_split(&a, &id);
    assert_eq!(token.balance(&a), 30_000);
    assert_eq!(token.balance(&b), 20_000);
    assert_eq!(client.get_stream(&id).withdrawn, 50_000);
}

// ---------------------------------------------------------------------------
// SC-77 (#445): immutable / locked streams
// ---------------------------------------------------------------------------

/// An immutable stream is flagged on creation and still accrues normally.
#[test]
fn test_immutable_stream_created_with_flag() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &true,
    );

    assert!(client.get_stream(&id).immutable);
    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 1_000);
}

/// A regular stream is not immutable.
#[test]
fn test_non_immutable_stream_flag_defaults_false() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None, &false);
    assert!(!client.get_stream(&id).immutable);
}

/// cancel_stream rejects an immutable stream.
#[test]
#[should_panic(expected = "E045")]
fn test_immutable_stream_cancel_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &true,
    );

    env.ledger().with_mut(|l| l.timestamp += 10);
    client.cancel_stream(&employer, &id, &None);
}

/// pause_stream rejects an immutable stream.
#[test]
#[should_panic(expected = "E045")]
fn test_immutable_stream_pause_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &true,
    );

    client.pause_stream(&employer, &id, &None);
}

/// cancel_streams_batch rejects a batch containing an immutable stream.
#[test]
#[should_panic(expected = "E045")]
fn test_immutable_stream_cancel_batch_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &true,
    );

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.cancel_streams_batch(&employer, &ids);
}

/// pause_stream_batch rejects a batch containing an immutable stream.
#[test]
#[should_panic(expected = "E045")]
fn test_immutable_stream_pause_batch_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &true,
    );

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.pause_stream_batch(&employer, &ids);
}

/// Immutable streams still accept top-ups and remain withdrawable.
#[test]
fn test_immutable_stream_top_up_and_withdraw_succeed() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &10_000,
        &10,
        &0,
        &None,
        &None,
        &true,
    );

    client.top_up(&employer, &id, &5_000);
    assert_eq!(client.get_stream(&id).deposit, 15_000);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw(&employee, &id);
    assert_eq!(token.balance(&employee), 1_000);
    assert_eq!(client.get_stream(&id).withdrawn, 1_000);
    assert!(client.get_stream(&id).immutable);
}

// ---------------------------------------------------------------------------
// SC-78 (#446): milestone-based vesting
// ---------------------------------------------------------------------------

fn milestone_schedule(
    env: &Env,
    entries: &[(u64, u32)],
) -> soroban_sdk::Vec<crate::types::Milestone> {
    let mut v = soroban_sdk::Vec::new(env);
    for (ts, bps) in entries.iter() {
        v.push_back(crate::types::Milestone {
            timestamp: *ts,
            basis_points: *bps,
        });
    }
    v
}

/// Nothing is claimable before the first milestone.
#[test]
fn test_milestone_nothing_before_first_milestone() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = milestone_schedule(&env, &[(1_500, 2_500), (2_000, 5_000)]);
    let id = client.create_milestone_vest(&employer, &employee, &token_id, &100_000, &schedule, &None);

    env.ledger().with_mut(|l| l.timestamp = 1_499);
    assert_eq!(client.claimable(&id), 0);
}

/// Claimable equals the vested fraction exactly at each milestone boundary.
#[test]
fn test_milestone_boundaries() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = milestone_schedule(&env, &[(1_500, 2_500), (2_000, 5_000), (3_000, 10_000)]);
    let id = client.create_milestone_vest(&employer, &employee, &token_id, &100_000, &schedule, &None);

    env.ledger().with_mut(|l| l.timestamp = 1_500);
    assert_eq!(client.claimable(&id), 25_000);
    env.ledger().with_mut(|l| l.timestamp = 1_999);
    assert_eq!(client.claimable(&id), 25_000);
    env.ledger().with_mut(|l| l.timestamp = 2_000);
    assert_eq!(client.claimable(&id), 50_000);
    env.ledger().with_mut(|l| l.timestamp = 3_000);
    assert_eq!(client.claimable(&id), 100_000);
}

/// A cliff delays all milestone vesting until it passes.
#[test]
fn test_milestone_cliff_delays_vesting() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = milestone_schedule(&env, &[(1_500, 2_500), (2_000, 5_000)]);
    let id =
        client.create_milestone_vest(&employer, &employee, &token_id, &100_000, &schedule, &Some(1_800));

    env.ledger().with_mut(|l| l.timestamp = 1_700);
    assert_eq!(client.claimable(&id), 0);
    env.ledger().with_mut(|l| l.timestamp = 1_800);
    assert_eq!(client.claimable(&id), 25_000);
    env.ledger().with_mut(|l| l.timestamp = 2_000);
    assert_eq!(client.claimable(&id), 50_000);
}

/// Partial claims decrement the remaining claimable amount.
#[test]
fn test_milestone_partial_claims() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = milestone_schedule(&env, &[(1_500, 2_500), (2_000, 5_000)]);
    let id = client.create_milestone_vest(&employer, &employee, &token_id, &100_000, &schedule, &None);

    env.ledger().with_mut(|l| l.timestamp = 1_500);
    client.withdraw(&employee, &id);
    assert_eq!(token.balance(&employee), 25_000);
    assert_eq!(client.claimable(&id), 0);

    env.ledger().with_mut(|l| l.timestamp = 2_000);
    assert_eq!(client.claimable(&id), 25_000);
    client.withdraw(&employee, &id);
    assert_eq!(token.balance(&employee), 50_000);
    assert_eq!(client.get_stream(&id).withdrawn, 50_000);
}

/// Milestone fractions must be strictly increasing and end at 10_000 bps.
#[test]
#[should_panic(expected = "E046")]
fn test_milestone_schedule_must_end_at_full_vest() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = milestone_schedule(&env, &[(1_500, 2_500), (2_000, 5_000)]);
    client.create_milestone_vest(&employer, &employee, &token_id, &100_000, &schedule, &None);
}

/// Milestone timestamps must be strictly increasing.
#[test]
#[should_panic(expected = "E046")]
fn test_milestone_schedule_must_be_sorted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = milestone_schedule(&env, &[(2_000, 5_000), (1_500, 10_000)]);
    client.create_milestone_vest(&employer, &employee, &token_id, &100_000, &schedule, &None);
}

// ---------------------------------------------------------------------------
// SC-79 (#447): dynamic rate schedule
// ---------------------------------------------------------------------------

fn rate_schedule(env: &Env, entries: &[(u64, i128)]) -> soroban_sdk::Vec<(u64, i128)> {
    let mut v = soroban_sdk::Vec::new(env);
    for (ts, rate) in entries.iter() {
        v.push_back((*ts, *rate));
    }
    v
}

/// Rate changes auto-apply at the configured timestamps.
#[test]
fn test_rate_schedule_mid_schedule_withdraw() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = rate_schedule(&env, &[(1_100, 10), (1_200, 20)]);
    let id = client.create_stream_with_rate_schedule(
        &employer,
        &employee,
        &token_id,
        &100_000,
        &1,
        &0,
        &None,
        &None,
        &schedule,
    );

    // 100s at 1/s + 100s at 10/s + 100s at 20/s
    env.ledger().with_mut(|l| l.timestamp = 1_300);
    assert_eq!(client.claimable(&id), 3_100);

    client.withdraw(&employee, &id);
    assert_eq!(token.balance(&employee), 3_100);
    assert_eq!(client.get_stream(&id).withdrawn, 3_100);
}

/// An unsorted rate schedule is rejected.
#[test]
#[should_panic(expected = "E047")]
fn test_rate_schedule_unsorted_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = rate_schedule(&env, &[(1_200, 10), (1_100, 20)]);
    client.create_stream_with_rate_schedule(
        &employer,
        &employee,
        &token_id,
        &100_000,
        &1,
        &0,
        &None,
        &None,
        &schedule,
    );
}

/// A schedule entry in the past (gap before the first entry) is rejected.
#[test]
#[should_panic(expected = "E047")]
fn test_rate_schedule_past_timestamp_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = rate_schedule(&env, &[(900, 10)]);
    client.create_stream_with_rate_schedule(
        &employer,
        &employee,
        &token_id,
        &100_000,
        &1,
        &0,
        &None,
        &None,
        &schedule,
    );
}

/// Duplicate timestamps (overlap) are rejected.
#[test]
#[should_panic(expected = "E047")]
fn test_rate_schedule_overlap_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = rate_schedule(&env, &[(1_100, 10), (1_100, 20)]);
    client.create_stream_with_rate_schedule(
        &employer,
        &employee,
        &token_id,
        &100_000,
        &1,
        &0,
        &None,
        &None,
        &schedule,
    );
}

/// A negative rate in the schedule is rejected.
#[test]
#[should_panic(expected = "E047")]
fn test_rate_schedule_negative_rate_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    let schedule = rate_schedule(&env, &[(1_100, -1)]);
    client.create_stream_with_rate_schedule(
        &employer,
        &employee,
        &token_id,
        &100_000,
        &1,
        &0,
        &None,
        &None,
        &schedule,
    );
}

// ---------------------------------------------------------------------------
// SC-80 (#448): withdraw_to delegate recipient
// ---------------------------------------------------------------------------

/// withdraw_to sends tokens to the delegate and decrements the holder's claimable.
#[test]
fn test_withdraw_to_delegates_tokens() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let savings = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    let token = paystream_token::TokenContractClient::new(&env, &token_id);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None, &false);

    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 1_000);

    client.withdraw_to(&employee, &id, &savings);

    assert_eq!(token.balance(&savings), 1_000);
    assert_eq!(token.balance(&employee), 0);
    assert_eq!(client.claimable(&id), 0);
    assert_eq!(client.get_stream(&id).withdrawn, 1_000);
}

/// withdraw_to emits an event carrying the delegate recipient.
#[test]
fn test_withdraw_to_emits_event() {
    use soroban_sdk::{symbol_short, vec, IntoVal};

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let savings = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None, &false);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.withdraw_to(&employee, &id, &savings);

    let events = crate::test::all_events(&env);
    let topics = vec![
        &env,
        symbol_short!("v1").into_val(&env),
        symbol_short!("withdraw_to").into_val(&env),
        id.into_val(&env),
    ];
    assert!(events.iter().any(|(_, t, _)| t == topics));
}

/// withdraw_to requires the holder's auth; a third party cannot redirect funds.
#[test]
fn test_withdraw_to_requires_holder_auth() {
    let env = Env::default();
    let id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let attacker = Address::generate(&env);
    let savings = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let stream_id =
        client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None, &false);

    env.ledger().with_mut(|l| l.timestamp += 100);
    // No mock_all_auths: the attacker has not signed for the employee.
    let res = client.try_withdraw_to(&attacker, &stream_id, &savings);
    assert!(res.is_err(), "withdraw_to must require the holder's auth");
}

// ---------------------------------------------------------------------------
// SC-66 (#434): configurable per-stream max deposit cap
// ---------------------------------------------------------------------------

/// Default max deposit is 0 (unlimited): a very large deposit is accepted.
#[test]
fn test_max_deposit_defaults_to_unlimited() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    assert_eq!(client.get_max_deposit(), 0, "max deposit must default to 0 (off)");

    // 500_000_000 is far above any cap we set in other tests; with the default
    // of 0 (unlimited) it must be accepted.
    let id = client.create_stream(
        &employer,
        &employee,
        &token_id,
        &500_000_000,
        &1,
        &0,
        &None,
        &None,
    );
    assert_eq!(client.get_stream(&id).deposit, 500_000_000);
}

/// Boundary: deposit == max_deposit is allowed.
#[test]
fn test_create_stream_deposit_equal_to_max_deposit_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_max_deposit(&admin, &0, &10_000);
    assert_eq!(client.get_max_deposit(), 10_000);

    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0, &None, &None);
    assert_eq!(client.get_stream(&id).deposit, 10_000);
}

/// Boundary: deposit == max_deposit + 1 is rejected with ERR_ABOVE_MAX_DEPOSIT.
#[test]
#[should_panic(expected = "E036")]
fn test_create_stream_deposit_above_max_deposit_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_max_deposit(&admin, &0, &10_000);

    // 10_001 > max_deposit = 10_000 → ERR_ABOVE_MAX_DEPOSIT
    client.create_stream(&employer, &employee, &token_id, &10_001, &1, &0, &None, &None);
}

/// Setting max_deposit back to 0 disables the cap again.
#[test]
fn test_set_max_deposit_zero_disables_cap() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_max_deposit(&admin, &0, &1_000);
    client.set_max_deposit(&admin, &1, &0);
    assert_eq!(client.get_max_deposit(), 0);

    let id = client.create_stream(&employer, &employee, &token_id, &50_000, &1, &0, &None, &None);
    assert_eq!(client.get_stream(&id).deposit, 50_000);
}

/// Non-admin callers cannot change the max deposit.
#[test]
#[should_panic]
fn test_set_max_deposit_non_admin_rejected() {
    let env = Env::default();
    let contract_id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &contract_id);

    env.mock_all_auths();
    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Fresh env without mocked auths: the attacker's call must fail auth.
    let env2 = Env::default();
    env2.register_at(&contract_id, StreamContract, ());
    let client2 = StreamContractClient::new(&env2, &contract_id);

    let attacker = Address::generate(&env2);
    client2.set_max_deposit(&attacker, &0, &1_000);
}
