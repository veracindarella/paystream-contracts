// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Env};

use crate::storage;
use crate::types::StreamStatus;
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
    token.initialize(
        admin,
        &1_000_000_000,
        &soroban_sdk::String::from_str(env, "Test Token"),
        &soroban_sdk::String::from_str(env, "TST"),
    );
    token_id
}

// ---------------------------------------------------------------------------
// Issue #7 – Emit contract_initialized event in initialize
// ---------------------------------------------------------------------------

/// initialize must emit a contract_initialized event with the admin address.
#[test]
fn test_initialize_emits_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(StreamContract, ());
    let client = StreamContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    // Before initialize, no events
    assert_eq!(env.events().all().len(), 0);

    client.initialize(&admin);

    // After initialize, exactly one event must have been emitted
    let events = env.events().all();
    assert_eq!(events.len(), 1, "expected exactly one event from initialize");
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
    let id = client.create_stream(&employer, &employee, &token_id, &3600, &1, &0);
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
fn test_claimable_increases_with_time() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 1000);
}

#[test]
fn test_withdraw() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

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
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0);

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.pause_stream(&employer, &id);

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id);

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id);

    // Find the "cancelled" event in the emitted events list
    let events = env.events().all();
    let cancelled_events: SdkVec<_> = events
        .iter()
        .filter(|(_, topics, _): &(_, SdkVec<Val>, Val)| {
            // first topic is symbol "cancelled"
            if let Some(first) = topics.get(0) {
                let sym: Result<soroban_sdk::Symbol, _> = first.try_into_val(&env);
                sym.map(|s| s == symbol_short!("cancelled")).unwrap_or(false)
            } else {
                false
            }
        })
        .collect();

    assert!(!cancelled_events.is_empty(), "no cancelled event emitted");

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &(now + 50));

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 30);
    client.pause_stream(&employer, &id);
    env.ledger().with_mut(|l| l.timestamp += 200);
    client.resume_stream(&employer, &id);

    env.ledger().with_mut(|l| l.timestamp += 20);
    client.pause_stream(&employer, &id);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0);

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

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
        locked: false,
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
        locked: false,
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
    client.create_stream(&employer, &employee, &token_id, &10_000, &0, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &3600, &1, &0);
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
    client.create_stream(&employer, &employee, &token_id, &100, &1, &0);
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
    client.create_stream(&employer, &employer, &token_id, &10_000, &1, &0);
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
    client.create_stream(&employer, &employee, &token_id, &10_000, &1, &past_stop_time);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0);
    client.top_up(&employer, &id, &0);
}

// ---------------------------------------------------------------------------
// Issue #27 – update_rate: change stream rate without cancel/recreate
// ---------------------------------------------------------------------------

/// Employer increases the rate; claimable is recalculated at the new rate
/// going forward (old accrual is settled at the time of the rate change).
#[test]
fn test_update_rate_increase() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    // rate=10/s, deposit=10_000
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    // 100 s at old rate → 1000 tokens accrued but NOT withdrawn
    env.ledger().with_mut(|l| l.timestamp += 100);

    // Raise rate to 20/s — this also resets last_withdraw_time to now
    client.update_rate(&employer, &id, &20);

    let s = client.get_stream(&id);
    assert_eq!(s.rate_per_second, 20);

    // After another 50 s at new rate → 50 * 20 = 1000 more claimable
    env.ledger().with_mut(|l| l.timestamp += 50);
    assert_eq!(client.claimable(&id), 1000); // only accrual since rate change counts
}

/// Employer decreases the rate.
#[test]
fn test_update_rate_decrease() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.update_rate(&employer, &id, &5);

    let s = client.get_stream(&id);
    assert_eq!(s.rate_per_second, 5);

    env.ledger().with_mut(|l| l.timestamp += 100);
    assert_eq!(client.claimable(&id), 500); // 100 s * 5/s
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id);

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
    client.cancel_stream(&employer, &id);
    client.update_rate(&employer, &id, &5);
}

// ---------------------------------------------------------------------------
// Issue #20 – Contract upgrade / migration path
// ---------------------------------------------------------------------------

mod stream_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/paystream_stream.wasm");
}

#[test]
fn test_upgrade_preserves_stream_state() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);

    let new_wasm_hash = env.deployer().upload_contract_wasm(stream_wasm::WASM);
    client.upgrade(&new_wasm_hash, &0);

    let s = client.get_stream(&id);
    assert_eq!(s.deposit, 10_000);
    assert_eq!(s.rate_per_second, 10);
    assert_eq!(s.status, StreamStatus::Active);
    assert_eq!(client.claimable(&id), 1000);
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
// Issue #61 – TEST-12: Load test for streams_by_employer with 100+ streams
// ---------------------------------------------------------------------------

/// Load test: create 100 streams for a single employer and verify that
/// `streams_by_employer` returns all 100 IDs correctly.
///
/// This test exercises the `EmployerStreams` persistent `Vec<u64>` index at
/// scale and documents whether any Soroban value-size limits are approached.
///
/// Notes on SC-05 (`stream_count_by_employer`):
/// That function is not yet implemented. Until it lands, the count is verified
/// via `streams_by_employer(&employer).len()`. Switch to the dedicated function
/// once SC-05 is merged.
///
/// CPU / memory observations (run locally with `cargo test -- --nocapture`):
/// Soroban's testutils do not expose raw CPU-instruction or memory counters
/// through the public SDK v22.0.0 API. To measure those metrics, run the
/// contract against a local Stellar node with resource metering enabled and
/// inspect the `TransactionMeta` field. See `benchmarks/gas-optimization-report.md`
/// for the project's benchmarking approach. No Soroban value-size limits were
/// triggered at 100 streams during manual testing.
#[test]
fn test_streams_by_employer_load_100() {
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
        let id = client.create_stream(&employer, &employee, &token_id, &1000, &1, &0);
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

    // --- Verify streams_by_employer returns all 100 IDs ---
    let employer_streams = client.streams_by_employer(&employer);

    // Count check — proxy for stream_count_by_employer (SC-05).
    // TODO: replace with client.stream_count_by_employer(&employer) once SC-05 lands.
    assert_eq!(
        employer_streams.len() as u64,
        STREAM_COUNT,
        "streams_by_employer must return exactly {STREAM_COUNT} IDs"
    );

    // Content check — every created ID must appear in the index.
    for id in expected_ids.iter() {
        assert!(
            employer_streams.contains(id),
            "stream ID {id} missing from streams_by_employer result"
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
        assert_eq!(s.employer, employer, "stream {id} should belong to employer");
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    client.cancel_stream(&employer, &id);

    let events = env.events().all();
    let cancelled_event = events.iter().find(|(_, topics, _)| {
        *topics
            == vec![
                &env,
                symbol_short!("cancelled").into_val(&env),
                id.into_val(&env),
            ]
    });

    assert!(cancelled_event.is_some(), "cancelled event not emitted");
    let (_, _, data) = cancelled_event.unwrap();
    let (claimable, refund): (i128, i128) = data.into_val(&env);
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
    let id = client.create_stream(&employer, &employee, &token_id, &5_000, &10, &0);
    client.pause_stream(&employer, &id);
    client.cancel_stream(&employer, &id);

    let events = env.events().all();
    let cancelled_event = events.iter().find(|(_, topics, _)| {
        *topics
            == vec![
                &env,
                symbol_short!("cancelled").into_val(&env),
                id.into_val(&env),
            ]
    });

    assert!(cancelled_event.is_some(), "cancelled event not emitted");
    let (_, _, data) = cancelled_event.unwrap();
    let (claimable, refund): (i128, i128) = data.into_val(&env);
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

    client.propose_admin(&new_admin);
    assert_eq!(client.get_pending_admin(), Some(new_admin.clone()));
}

#[test]
fn test_get_pending_admin_none_after_accept() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    client.initialize(&admin);

    client.propose_admin(&new_admin);
    client.accept_admin(&new_admin);
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

    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0);

    // Force deposit to i128::MAX so adding any positive amount overflows.
    env.as_contract(&client.address, || {
        let mut stream = crate::storage::load_stream(&env, id).unwrap();
        stream.deposit = i128::MAX;
        save_stream(&env, &stream);
    });

    // top_up with 1 should panic with E004, not a generic "deposit overflow" message.
    client.top_up(&employer, &id, &1);
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
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &5, &0);
    let id3 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);
    let results = client.withdraw_all(&employee);

    // All three streams yielded tokens
    assert_eq!(results.len(), 3);

    // Verify each (stream_id, amount) pair
    let r0 = results.get(0).unwrap();
    assert_eq!(r0.0, id1);
    assert_eq!(r0.1, 1000); // 100s * 10/s

    let r1 = results.get(1).unwrap();
    assert_eq!(r1.0, id2);
    assert_eq!(r1.1, 500); // 100s * 5/s

    let r2 = results.get(2).unwrap();
    assert_eq!(r2.0, id3);
    assert_eq!(r2.1, 100); // 100s * 1/s
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
    let _id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
    let _id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &5, &0);

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
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &5, &0);
    let id3 = client.create_stream(&employer, &employee, &token_id, &10_000, &1, &0);

    env.ledger().with_mut(|l| l.timestamp += 50);

    // Cancel stream 1, pause stream 2 — only stream 3 should be withdrawn
    client.cancel_stream(&employer, &id1);
    client.pause_stream(&employer, &id2);

    env.ledger().with_mut(|l| l.timestamp += 50);
    let results = client.withdraw_all(&employee);

    // Only stream 3 is Active with claimable tokens
    assert_eq!(results.len(), 1);
    let r0 = results.get(0).unwrap();
    assert_eq!(r0.0, id3);
    // 100 seconds total active for stream 3 (pause on id2 doesn't affect id3)
    assert_eq!(r0.1, 100); // 100s * 1/s
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
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 100);

    // Create id2 after time has advanced — it starts now, so 0 claimable
    let _id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    let results = client.withdraw_all(&employee);

    // Only id1 has claimable tokens
    assert_eq!(results.len(), 1);
    let r0 = results.get(0).unwrap();
    assert_eq!(r0.0, id1);
    assert_eq!(r0.1, 1000); // 100s * 10/s
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

    client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0);
    assert_eq!(client.stream_count_by_employer(&employer), 1);

    client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0);
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

    client.create_stream(&employer1, &employee, &token_id1, &10_000, &10, &0);
    assert_eq!(client.stream_count_by_employee(&employee), 1);

    client.create_stream(&employer2, &employee, &token_id2, &10_000, &10, &0);
    assert_eq!(client.stream_count_by_employee(&employee), 2);
}

/// stream_count_by_employer count equals streams_by_employer length.
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
    client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0);
    client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0);
    client.create_stream(&employer, &employee3, &token_id, &10_000, &10, &0);

    let count = client.stream_count_by_employer(&employer);
    let list_len = client.streams_by_employer(&employer).len() as u64;
    assert_eq!(count, list_len);
    assert_eq!(count, 3);
}

/// stream_count_by_employee count equals streams_by_employee length.
#[test]
fn test_stream_count_by_employee_matches_list_length() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
    client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    let count = client.stream_count_by_employee(&employee);
    let list_len = client.streams_by_employee(&employee).len() as u64;
    assert_eq!(count, list_len);
    assert_eq!(count, 2);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
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
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &0);
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
    let id1 = client.create_stream(&employer, &employee1, &token_id, &10_000, &10, &0);
    let id2 = client.create_stream(&employer, &employee2, &token_id, &10_000, &10, &0);

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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    env.ledger().with_mut(|l| l.timestamp += 50);
    client.pause_stream(&employer, &id);

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
    let id1 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);
    let id2 = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &0);

    // Cancel id2 individually first
    client.cancel_stream(&employer, &id2);

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
    token.mint(&employer1, &employer2, &10_000);

    client.initialize(&admin);
    // Stream owned by employer1
    let id = client.create_stream(&employer1, &employee, &token_id, &10_000, &10, &0);

    // employer2 tries to batch cancel employer1's stream — must panic
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(id);
    client.cancel_streams_batch(&employer2, &ids);
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
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &stop);

    // Advance past stop_time — all tokens have been streamed
    env.ledger().with_mut(|l| l.timestamp += 100);

    // withdraw: claimable = 0 (capped at stop_time), but stop_time is past
    // → stream must transition to Exhausted
    let withdrawn = client.withdraw(&employee, &id);
    assert_eq!(withdrawn, 500); // tokens earned up to stop_time
    assert_eq!(client.get_stream(&id).status, StreamStatus::Exhausted);
}

/// settle_stream transitions an Active stream to Exhausted when stop_time has
/// passed and no tokens remain claimable.
#[test]
fn test_settle_stream_transitions_to_exhausted() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let now = env.ledger().timestamp();
    let stop = now + 50;
    // Stream: 500 tokens, 10/s, 50-second window — deposit exactly matches
    let id = client.create_stream(&employer, &employee, &token_id, &500, &10, &stop);

    // Employee withdraws all earned tokens before calling settle
    env.ledger().with_mut(|l| l.timestamp = stop);
    client.withdraw(&employee, &id);

    // Advance past stop_time
    env.ledger().with_mut(|l| l.timestamp += 10);

    // Anyone can call settle_stream to push status to Exhausted
    client.settle_stream(&id);
    assert_eq!(client.get_stream(&id).status, StreamStatus::Exhausted);
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
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &stop);
    // stop_time not yet reached — must panic
    client.settle_stream(&id);
}

/// settle_stream must panic if there are still claimable tokens.
#[test]
#[should_panic(expected = "stream still has claimable tokens")]
fn test_settle_stream_with_claimable_tokens_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let employer = Address::generate(&env);
    let employee = Address::generate(&env);
    let token_id = setup_token(&env, &employer);

    client.initialize(&admin);
    client.set_min_deposit(&admin, &0, &100);

    let now = env.ledger().timestamp();
    let stop = now + 50;
    // Deposit more than stop_time * rate_per_second → tokens remain after stop
    let id = client.create_stream(&employer, &employee, &token_id, &10_000, &10, &stop);

    env.ledger().with_mut(|l| l.timestamp += 100);
    // Still has 500 claimable tokens (10/s * 50s) — must panic
    client.settle_stream(&id);
}
