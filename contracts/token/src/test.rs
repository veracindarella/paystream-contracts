// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

extern crate std;

use crate::{TokenContract, TokenContractClient};
use soroban_sdk::testutils::{Address as _, Events, Ledger};
use soroban_sdk::{vec, Address, Env, IntoVal, MuxedAddress, Symbol, Val, Vec};

fn setup() -> (Env, TokenContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(TokenContract, ());
    let client = TokenContractClient::new(&env, &id);
    (env, client)
}

#[test]
fn test_initialize() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000);
    assert_eq!(client.total_supply(), 1_000_000);
    assert_eq!(client.balance(&admin), 1_000_000);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_token_initialize_cannot_be_called_twice() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000);
    // Second call must panic with "already initialized"
    client.initialize(&admin, &500_000);
}

#[test]
fn test_transfer() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &400);
    assert_eq!(client.balance(&admin), 600);
    assert_eq!(client.balance(&user), 400);
}

#[test]
fn test_transfer_to_muxed_recipient() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let underlying = Address::generate(&env);
    let muxed_id: u64 = 42;
    let muxed = MuxedAddress::new(underlying.clone(), muxed_id);
    client.initialize(&admin, &1_000);

    client.transfer(&admin, &muxed, &400);

    // Balances are keyed by the underlying address, not the muxed id.
    assert_eq!(client.balance(&admin), 600);
    assert_eq!(client.balance(&underlying), 400);

    // The emitted transfer event uses the underlying address in its topics and
    // carries the muxed id only in the event data.
    let events = env.events().all();
    let (_, topics, data) = events.last().unwrap();
    let expected_topics: Vec<Val> = vec![
        &env,
        Symbol::new(&env, "transfer").into_val(&env),
        admin.clone().into_val(&env),
        underlying.clone().into_val(&env),
    ];
    assert_eq!(topics, expected_topics);
    let (amount, to_muxed_id): (i128, Option<u64>) = data.into_val(&env);
    assert_eq!(amount, 400);
    assert_eq!(to_muxed_id, Some(muxed_id));
}

#[test]
fn test_mint_and_burn() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.mint(&admin, &user, &500, &0);
    assert_eq!(client.total_supply(), 1_500);
    // holder burns their own tokens
    client.burn(&user, &200);
    assert_eq!(client.total_supply(), 1_300);
    assert_eq!(client.balance(&user), 300);
}

#[test]
fn test_burn_from_with_allowance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &500);
    client.approve(&user, &spender, &300, &100);
    client.burn_from(&spender, &user, &200);
    assert_eq!(client.balance(&user), 300);
    assert_eq!(client.total_supply(), 800);
}

#[test]
#[should_panic(expected = "allowance exceeded")]
fn test_burn_from_exceeds_allowance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &500);
    client.approve(&user, &spender, &100, &100);
    client.burn_from(&spender, &user, &200);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_burn_insufficient_balance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &100);
    client.transfer(&admin, &user, &50);
    client.burn(&user, &200);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_transfer_overdraft() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &100);
    client.transfer(&admin, &user, &999);
}

#[test]
fn test_admin_transfer_flow() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&new_admin, &0);
    client.accept_admin(&new_admin);
    client.mint(&new_admin, &user, &10, &1);
    assert_eq!(client.balance(&user), 10);
}

#[test]
#[should_panic(expected = "not pending admin")]
fn test_accept_admin_wrong_address() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&Address::generate(&env), &0);
    client.accept_admin(&Address::generate(&env));
}

#[test]
#[should_panic(expected = "no pending admin")]
fn test_accept_admin_without_pending() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.accept_admin(&Address::generate(&env));
}

#[test]
fn test_mint_increments_nonce() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(client.admin_nonce(), 0);
    client.mint(&admin, &user, &10, &0);
    client.mint(&admin, &user, &10, &1);
    assert_eq!(client.admin_nonce(), 2);
}

#[test]
#[should_panic(expected = "invalid nonce")]
fn test_mint_replay_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.mint(&admin, &user, &10, &0);
    client.mint(&admin, &user, &10, &0);
}

#[test]
#[should_panic(expected = "T001: token arithmetic overflow")]
fn test_mint_overflow_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &i128::MAX);
    client.mint(&admin, &admin, &1, &0);
}

#[test]
fn test_allowance_not_expired_works() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    let to = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 5;
    client.approve(&admin, &spender, &300, &exp);
    assert_eq!(client.allowance(&admin, &spender), 300);
    env.ledger().with_mut(|l| l.sequence_number = exp);
    client.transfer_from(&spender, &admin, &to, &100);
    assert_eq!(client.balance(&to), 100);
    assert_eq!(client.allowance(&admin, &spender), 200);
}

#[test]
#[should_panic(expected = "allowance expired")]
fn test_transfer_from_expired_allowance_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    let to = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 5;
    client.approve(&admin, &spender, &300, &exp);
    env.ledger().with_mut(|l| l.sequence_number = exp + 1);
    client.transfer_from(&spender, &admin, &to, &100);
}

#[test]
#[should_panic(expected = "allowance expired")]
fn test_burn_from_expired_allowance_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 5;
    client.approve(&admin, &spender, &300, &exp);
    env.ledger().with_mut(|l| l.sequence_number = exp + 1);
    client.burn_from(&spender, &admin, &100);
}

// ---------------------------------------------------------------------------
// TOK-10 – upgrade mechanism
// ---------------------------------------------------------------------------

fn zero_hash(env: &Env) -> soroban_sdk::BytesN<32> {
    soroban_sdk::BytesN::from_array(env, &[0u8; 32])
}

#[test]
#[should_panic]
fn test_upgrade_non_admin_rejected() {
    let env = Env::default();
    let id = env.register(TokenContract, ());
    let client = TokenContractClient::new(&env, &id);
    let admin = Address::generate(&env);
    env.mock_all_auths();
    client.initialize(&admin, &1_000);
    // Drop mocked auths: upgrade must require the stored admin's signature.
    env.set_auths(&[]);
    client.upgrade(&admin, &0, &zero_hash(&env));
}

#[test]
#[should_panic(expected = "not admin")]
fn test_upgrade_wrong_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.upgrade(&Address::generate(&env), &0, &zero_hash(&env));
}

#[test]
#[should_panic(expected = "invalid nonce")]
fn test_upgrade_wrong_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.upgrade(&admin, &5, &zero_hash(&env));
}

#[test]
fn test_upgrade_schedules_with_24h_timelock() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.upgrade(&admin, &0, &zero_hash(&env));
    let pending = client.pending_upgrade().unwrap();
    assert_eq!(pending.wasm_hash, zero_hash(&env));
    assert_eq!(pending.unlock_time, 1_000 + 86_400);
}

#[test]
#[should_panic(expected = "T004: upgrade timelock active")]
fn test_execute_upgrade_before_timelock_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.upgrade(&admin, &0, &zero_hash(&env));
    env.ledger().with_mut(|l| l.timestamp = 86_399);
    client.execute_upgrade(&admin, &1);
}

#[test]
#[should_panic(expected = "T003: no pending upgrade")]
fn test_execute_upgrade_without_pending_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.execute_upgrade(&admin, &0);
}

#[test]
fn test_cancel_upgrade_clears_pending() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.upgrade(&admin, &0, &zero_hash(&env));
    client.cancel_upgrade(&admin, &1);
    assert!(client.pending_upgrade().is_none());
}

#[test]
fn test_version_stored_on_initialize() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    assert_eq!(client.version(), 0);
    client.initialize(&admin, &1_000);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);
}

#[test]
fn test_migrate_stamps_version_and_consumes_nonce() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.migrate(&admin, &0);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);
    assert_eq!(client.admin_nonce(), 1);
}

#[test]
#[should_panic(expected = "not admin")]
fn test_migrate_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.migrate(&Address::generate(&env), &0);
}

#[test]
#[should_panic(expected = "invalid nonce")]
fn test_migrate_wrong_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.migrate(&admin, &3);
}

// ---------------------------------------------------------------------------
// TOK-15 – max supply cap
// ---------------------------------------------------------------------------

#[test]
fn test_max_supply_defaults_to_uncapped() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(client.get_max_supply(), 0);
    client.mint(&admin, &admin, &1_000_000, &0);
    assert_eq!(client.total_supply(), 1_001_000);
}

#[test]
fn test_mint_up_to_cap_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_max_supply(&admin, &0, &1_500);
    assert_eq!(client.get_max_supply(), 1_500);
    client.mint(&admin, &admin, &500, &1);
    assert_eq!(client.total_supply(), 1_500);
}

#[test]
#[should_panic(expected = "T002: max supply exceeded")]
fn test_mint_over_cap_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_max_supply(&admin, &0, &1_500);
    client.mint(&admin, &admin, &501, &1);
}

#[test]
#[should_panic(expected = "not admin")]
fn test_set_max_supply_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_max_supply(&Address::generate(&env), &0, &1_500);
}

// ---------------------------------------------------------------------------
// TOK-16 – storage TTL management
// ---------------------------------------------------------------------------

fn balance_ttl(env: &Env, client: &TokenContractClient, owner: &Address) -> u32 {
    env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&TokenDataKey::Balance(owner.clone()))
    })
}

#[test]
fn test_balance_ttl_extended_on_set() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &10);
    assert_eq!(balance_ttl(&env, &client, &user), TTL_EXTEND_TO);
}

#[test]
fn test_balance_ttl_extended_on_get() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    // Advance far enough that remaining TTL falls below TTL_THRESHOLD.
    env.ledger()
        .with_mut(|l| l.sequence_number += TTL_EXTEND_TO - 6_000_000);
    assert!(balance_ttl(&env, &client, &admin) < crate::TTL_THRESHOLD);
    assert_eq!(client.balance(&admin), 1_000);
    assert_eq!(balance_ttl(&env, &client, &admin), TTL_EXTEND_TO);
}

#[test]
fn test_allowance_ttl_extended_to_expiration() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 1_000;
    client.approve(&admin, &spender, &300, &exp);
    let ttl = env.as_contract(&client.address, || {
        use soroban_sdk::testutils::storage::Temporary as _;
        env.storage()
            .temporary()
            .get_ttl(&TokenDataKey::Allowance(admin.clone(), spender.clone()))
    });
    assert!(ttl >= 1_000);
    assert_eq!(client.allowance(&admin, &spender), 300);
}

// ---------------------------------------------------------------------------
// TOK-17 – allowance expiry
// ---------------------------------------------------------------------------

#[test]
fn test_allowance_returns_zero_after_expiry() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 5;
    client.approve(&admin, &spender, &300, &exp);
    assert_eq!(client.allowance(&admin, &spender), 300);
    env.ledger().with_mut(|l| l.sequence_number = exp + 1);
    assert_eq!(client.allowance(&admin, &spender), 0);
}

#[test]
#[should_panic(expected = "expiration_ledger is in the past")]
fn test_approve_past_expiration_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    env.ledger().with_mut(|l| l.sequence_number = 100);
    client.approve(&admin, &spender, &300, &99);
}

// Requires a release WASM build of the token contract:
//   cargo build -p paystream-token --target wasm32v1-none --release
//   cargo test -p paystream-token --features wasm-tests
#[cfg(feature = "wasm-tests")]
mod token_wasm {
    soroban_sdk::contractimport!(file = "../../target/wasm32v1-none/release/paystream_token.wasm");
}

#[cfg(feature = "wasm-tests")]
#[test]
fn test_upgrade_preserves_balances_and_supply() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &250);

    let new_wasm_hash = env.deployer().upload_contract_wasm(token_wasm::WASM);
    client.upgrade(&admin, &0, &new_wasm_hash);
    env.ledger().with_mut(|l| l.timestamp += 86_400);
    client.execute_upgrade(&admin, &1);
    client.migrate(&admin, &2);

    assert_eq!(client.total_supply(), 1_000);
    assert_eq!(client.balance(&admin), 750);
    assert_eq!(client.balance(&user), 250);
    assert_eq!(client.admin_nonce(), 3);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);
}

// --- TOK-11: burn_from ---

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_burn_from_insufficient_balance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &100);
    client.approve(&user, &spender, &500, &100);
    client.burn_from(&spender, &user, &200);
}

#[test]
fn test_burn_from_reduces_allowance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &500);
    client.approve(&user, &spender, &300, &100);
    client.burn_from(&spender, &user, &200);
    assert_eq!(client.allowance(&user, &spender), (100, 100));
}

// --- TOK-12: admin nonce ---

#[test]
fn test_propose_admin_consumes_nonce() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(client.get_admin_nonce(), 0);
    client.propose_admin(&Address::generate(&env), &0);
    assert_eq!(client.get_admin_nonce(), 1);
}

#[test]
#[should_panic(expected = "invalid nonce")]
fn test_propose_admin_stale_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&Address::generate(&env), &0);
    client.propose_admin(&Address::generate(&env), &0);
}

// --- TOK-13: pause / unpause ---

#[test]
#[should_panic(expected = "T002: token is paused")]
fn test_transfer_while_paused_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.pause_token(&admin, &0);
    client.transfer(&admin, &Address::generate(&env), &10);
}

#[test]
#[should_panic(expected = "T002: token is paused")]
fn test_transfer_from_while_paused_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.approve(&admin, &spender, &100, &100);
    client.pause_token(&admin, &0);
    client.transfer_from(&spender, &admin, &Address::generate(&env), &10);
}

#[test]
#[should_panic(expected = "T002: token is paused")]
fn test_burn_while_paused_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.pause_token(&admin, &0);
    client.burn(&admin, &10);
}

#[test]
#[should_panic(expected = "T002: token is paused")]
fn test_burn_from_while_paused_panics() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.approve(&admin, &spender, &100, &100);
    client.pause_token(&admin, &0);
    client.burn_from(&spender, &admin, &10);
}

#[test]
fn test_unpause_restores_transfers() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.pause_token(&admin, &0);
    assert!(client.is_paused());
    client.unpause_token(&admin, &1);
    assert!(!client.is_paused());
    assert_eq!(client.get_admin_nonce(), 2);
    client.transfer(&admin, &user, &10);
    assert_eq!(client.balance(&user), 10);
}

#[test]
#[should_panic(expected = "not admin")]
fn test_pause_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.pause_token(&Address::generate(&env), &0);
}

// --- TOK-14: mint_to_batch ---

#[test]
fn test_mint_to_batch_success() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.mint_to_batch(&admin, &0, &vec![&env, (a.clone(), 100), (b.clone(), 250)]);
    assert_eq!(client.balance(&a), 100);
    assert_eq!(client.balance(&b), 250);
    assert_eq!(client.total_supply(), 1_350);
    assert_eq!(client.admin_nonce(), 1);
}

#[test]
#[should_panic(expected = "T001: token arithmetic overflow")]
fn test_mint_to_batch_overflow_reverts() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &(i128::MAX - 10));
    client.mint_to_batch(
        &admin,
        &0,
        &vec![
            &env,
            (Address::generate(&env), 5),
            (Address::generate(&env), 10),
        ],
    );
}

#[test]
#[should_panic(expected = "T003: batch too large")]
fn test_mint_to_batch_cap_exceeded() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let mut recipients: Vec<(Address, i128)> = Vec::new(&env);
    for _ in 0..51 {
        recipients.push_back((Address::generate(&env), 1));
    }
    client.mint_to_batch(&admin, &0, &recipients);
}
