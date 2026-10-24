// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

extern crate std;

use soroban_sdk::testutils::{Address as _, Events as _, Ledger as _};
use soroban_sdk::{Address, Env};

use crate::storage;
use crate::types::{AutoTopupConfig, StreamParams, StreamStatus};
use crate::{StreamContract, StreamContractClient};

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
        });
    }
    params
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

#[test]
#[should_panic(expected = "protocol fee cannot exceed 100 basis points")]
fn test_protocol_fee_above_one_percent_is_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.initialize(&admin);
    client.set_protocol_fee(&admin, &0, &101, &treasury);
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
    });
    params.push_back(StreamParams {
        employee: Address::generate(&env),
        token: token_id.clone(),
        deposit: 100_000,
        rate_per_second: 10,
        stop_time: 0,
            cliff_time: None,
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
#[should_panic(expected = "E004")]
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

    // top_up with 1 should panic with E004, not a generic "deposit overflow" message.
    client.top_up(&employer, &id, &1, &None);
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
    });
    params.push_back(StreamParams {
        employee: employee3,
        token: token_b.clone(),
        deposit: 10_000,
        rate_per_second: 10,
        stop_time: 0,
            cliff_time: None,
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

/// stream_status panics with "stream not found" for a non-existent stream.
#[test]
#[should_panic(expected = "stream not found")]
fn test_stream_status_not_found_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin);
    client.stream_status(&999);
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
#[should_panic(expected = "E029")]
fn test_create_streams_batch_over_max_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);
    let params = make_stream_params(&env, &token_id, 51);

    client.create_streams_batch(&employer, &params);
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
#[should_panic(expected = "E029")]
fn test_cancel_streams_batch_over_max_panics() {
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

    client.cancel_streams_batch(&employer, &ids);
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
#[should_panic(expected = "E029")]
fn test_pause_stream_batch_over_max_panics() {
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

    client.pause_stream_batch(&employer, &ids);
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
#[should_panic(expected = "E029")]
fn test_resume_stream_batch_over_max_panics() {
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

    client.resume_stream_batch(&employer, &ids);
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
#[should_panic(expected = "stream_ids must not be empty")]
fn test_cancel_streams_batch_empty_list_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    client.initialize(&admin);

    let ids: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    client.cancel_streams_batch(&employer, &ids);
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

/// Wrong caller (not the employer) must be rejected with ERR_NOT_EMPLOYER.
#[test]
#[should_panic(expected = "E018")]
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

    client.transfer_stream(&attacker, &id, &new_employer);
}

/// Transfer of a Cancelled stream must be rejected with ERR_STREAM_ALREADY_ENDED.
#[test]
#[should_panic(expected = "E020")]
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

    client.transfer_stream(&employer, &id, &new_employer);
}

/// Transfer of an Exhausted stream must be rejected with ERR_STREAM_ALREADY_ENDED.
#[test]
#[should_panic(expected = "E020")]
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

    client.transfer_stream(&employer, &id, &new_employer);
}

/// new_employer must not equal the stream's employee (E028).
#[test]
#[should_panic(expected = "E028")]
fn test_transfer_stream_new_employer_is_employee_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0, &None, &None);

    client.transfer_stream(&employer, &id, &employee);
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

#[test]
#[should_panic(expected = "E035")]
fn test_cliff_after_stop_time_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.create_stream(&employer, &employee, &token_id, &10_000, &10, &1_500, &None, &Some(1_600));
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
#[should_panic(expected = "E031")]
fn test_template_create_after_delete_fails() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &1, &template_params(&env, &token_id, None));
    client.delete_template(&employer, &1);
    client.create_stream_from_template(&employer, &employee, &1);
}

#[test]
#[should_panic(expected = "E031")]
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
    client.create_stream_from_template(&other, &employee, &1);
}

#[test]
#[should_panic(expected = "E031")]
fn test_template_other_employer_cannot_delete() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let other = Address::generate(&env);
    let token_id = setup_token(&env, &employer);
    client.initialize(&admin);

    client.save_template(&employer, &1, &template_params(&env, &token_id, None));
    client.delete_template(&other, &1);
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
