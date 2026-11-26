// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

extern crate std;

use crate::types::TokenDataKey;
use crate::{TokenContract, TokenContractClient, TOKEN_CONTRACT_VERSION};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{vec, Address, Env, Vec};

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
fn test_token_initialize_cannot_be_called_twice() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000_000);
    // Second call must return AlreadyInitialized
    assert_eq!(
        client.try_initialize(&admin, &500_000),
        Err(Ok(TokenError::AlreadyInitialized))
    );
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
fn test_burn_from_exceeds_allowance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &500);
    client.approve(&user, &spender, &100, &100);
    assert_eq!(
        client.try_burn_from(&spender, &user, &200),
        Err(Ok(TokenError::AllowanceExceeded))
    );
}

#[test]
fn test_burn_insufficient_balance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &100);
    client.transfer(&admin, &user, &50);
    assert_eq!(
        client.try_burn(&user, &200),
        Err(Ok(TokenError::InsufficientBalance))
    );
}

#[test]
fn test_transfer_overdraft() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &100);
    assert_eq!(
        client.try_transfer(&admin, &user, &999),
        Err(Ok(TokenError::InsufficientBalance))
    );
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
fn test_accept_admin_wrong_address() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&Address::generate(&env), &0);
    assert_eq!(
        client.try_accept_admin(&Address::generate(&env)),
        Err(Ok(TokenError::NotPendingAdmin))
    );
}

#[test]
fn test_accept_admin_without_pending() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_accept_admin(&Address::generate(&env)),
        Err(Ok(TokenError::NoPendingAdmin))
    );
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
fn test_mint_replay_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.mint(&admin, &user, &10, &0);
    assert_eq!(
        client.try_mint(&admin, &user, &10, &0),
        Err(Ok(TokenError::InvalidNonce))
    );
}

#[test]
fn test_mint_overflow_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &i128::MAX);
    assert_eq!(
        client.try_mint(&admin, &admin, &1, &0),
        Err(Ok(TokenError::ArithmeticOverflow))
    );
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
fn test_transfer_from_expired_allowance_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    let to = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 5;
    client.approve(&admin, &spender, &300, &exp);
    env.ledger().with_mut(|l| l.sequence_number = exp + 1);
    assert_eq!(
        client.try_transfer_from(&spender, &admin, &to, &100),
        Err(Ok(TokenError::AllowanceExpired))
    );
}

#[test]
fn test_burn_from_expired_allowance_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let exp = env.ledger().sequence() + 5;
    client.approve(&admin, &spender, &300, &exp);
    env.ledger().with_mut(|l| l.sequence_number = exp + 1);
    assert_eq!(
        client.try_burn_from(&spender, &admin, &100),
        Err(Ok(TokenError::AllowanceExpired))
    );
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
fn test_upgrade_wrong_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_upgrade(&Address::generate(&env), &0, &zero_hash(&env)),
        Err(Ok(TokenError::NotAdmin))
    );
}

#[test]
fn test_upgrade_wrong_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_upgrade(&admin, &5, &zero_hash(&env)),
        Err(Ok(TokenError::InvalidNonce))
    );
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
fn test_execute_upgrade_before_timelock_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.upgrade(&admin, &0, &zero_hash(&env));
    env.ledger().with_mut(|l| l.timestamp = 86_399);
    assert_eq!(
        client.try_execute_upgrade(&admin, &1),
        Err(Ok(TokenError::UpgradeTimelockActive))
    );
}

#[test]
fn test_execute_upgrade_without_pending_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_execute_upgrade(&admin, &0),
        Err(Ok(TokenError::NoPendingUpgrade))
    );
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
fn test_migrate_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_migrate(&Address::generate(&env), &0),
        Err(Ok(TokenError::NotAdmin))
    );
}

#[test]
fn test_migrate_wrong_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_migrate(&admin, &3),
        Err(Ok(TokenError::InvalidNonce))
    );
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
fn test_mint_over_cap_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_max_supply(&admin, &0, &1_500);
    assert_eq!(
        client.try_mint(&admin, &admin, &501, &1),
        Err(Ok(TokenError::MaxSupplyExceeded))
    );
}

#[test]
fn test_set_max_supply_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_set_max_supply(&Address::generate(&env), &0, &1_500),
        Err(Ok(TokenError::NotAdmin))
    );
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
fn test_approve_past_expiration_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    env.ledger().with_mut(|l| l.sequence_number = 100);
    assert_eq!(
        client.try_approve(&admin, &spender, &300, &99),
        Err(Ok(TokenError::ExpirationInPast))
    );
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
fn test_burn_from_insufficient_balance() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.transfer(&admin, &user, &100);
    client.approve(&user, &spender, &500, &100);
    assert_eq!(
        client.try_burn_from(&spender, &user, &200),
        Err(Ok(TokenError::InsufficientBalance))
    );
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
fn test_propose_admin_stale_nonce_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&Address::generate(&env), &0);
    assert_eq!(
        client.try_propose_admin(&Address::generate(&env), &0),
        Err(Ok(TokenError::InvalidNonce))
    );
}

// --- TOK-13: pause / unpause ---

#[test]
fn test_transfer_while_paused_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.pause_token(&admin, &0);
    assert_eq!(
        client.try_transfer(&admin, &Address::generate(&env), &10),
        Err(Ok(TokenError::TokenPaused))
    );
}

#[test]
fn test_transfer_from_while_paused_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.approve(&admin, &spender, &100, &100);
    client.pause_token(&admin, &0);
    assert_eq!(
        client.try_transfer_from(&spender, &admin, &Address::generate(&env), &10),
        Err(Ok(TokenError::TokenPaused))
    );
}

#[test]
fn test_burn_while_paused_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.pause_token(&admin, &0);
    assert_eq!(
        client.try_burn(&admin, &10),
        Err(Ok(TokenError::TokenPaused))
    );
}

#[test]
fn test_burn_from_while_paused_returns_error() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.approve(&admin, &spender, &100, &100);
    client.pause_token(&admin, &0);
    assert_eq!(
        client.try_burn_from(&spender, &admin, &10),
        Err(Ok(TokenError::TokenPaused))
    );
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
fn test_pause_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(
        client.try_pause_token(&Address::generate(&env), &0),
        Err(Ok(TokenError::NotAdmin))
    );
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
fn test_mint_to_batch_overflow_reverts() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &(i128::MAX - 10));
    assert_eq!(
        client.try_mint_to_batch(
            &admin,
            &0,
            &vec![
                &env,
                (Address::generate(&env), 5),
                (Address::generate(&env), 10),
            ],
        ),
        Err(Ok(TokenError::ArithmeticOverflow))
    );
}

#[test]
fn test_mint_to_batch_cap_exceeded() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let mut recipients: Vec<(Address, i128)> = Vec::new(&env);
    for _ in 0..51 {
        recipients.push_back((Address::generate(&env), 1));
    }
    assert_eq!(
        client.try_mint_to_batch(&admin, &0, &recipients),
        Err(Ok(TokenError::BatchTooLarge))
    );
}

// ---------------------------------------------------------------------------
// TOK-33 – per-recipient cap in mint_to_batch
// ---------------------------------------------------------------------------

#[test]
fn test_recipient_cap_defaults_to_uncapped() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(client.get_recipient_cap(), 0);
    client.mint_to_batch(&admin, &0, &vec![&env, (Address::generate(&env), 1_000_000)]);
    assert_eq!(client.total_supply(), 1_001_000);
}

#[test]
fn test_mint_to_batch_within_cap_succeeds() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let a = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_recipient_cap(&admin, &0, &100);
    assert_eq!(client.get_recipient_cap(), 100);
    client.mint_to_batch(&admin, &1, &vec![&env, (a.clone(), 100)]);
    assert_eq!(client.balance(&a), 100);
}

#[test]
#[should_panic(expected = "T006: amount exceeds per-recipient cap")]
fn test_mint_to_batch_boundary_over_cap_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let a = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_recipient_cap(&admin, &0, &100);
    // One over the cap must be rejected ...
    client.mint_to_batch(&admin, &1, &vec![&env, (a.clone(), 101)]);
}

#[test]
fn test_mint_to_batch_all_or_nothing_on_cap_violation() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_recipient_cap(&admin, &0, &100);
    let recipients = vec![&env, (a.clone(), 50), (b.clone(), 101)];
    let result = client.try_mint_to_batch(&admin, &1, &recipients);
    assert!(result.is_err());
    // The whole batch must revert: `a` must not have been credited either.
    assert_eq!(client.balance(&a), 0);
    assert_eq!(client.total_supply(), 1_000);
}

#[test]
#[should_panic(expected = "not admin")]
fn test_set_recipient_cap_non_admin_rejected() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.set_recipient_cap(&Address::generate(&env), &0, &100);
}

// ---------------------------------------------------------------------------
// TOK-32 – metadata() struct query
// ---------------------------------------------------------------------------

#[test]
fn test_metadata_defaults() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let meta = client.metadata();
    assert_eq!(meta.decimal, 7);
    assert_eq!(meta.name, soroban_sdk::String::from_str(&env, "PayStream Token"));
    assert_eq!(meta.symbol, soroban_sdk::String::from_str(&env, "PST"));
    assert_eq!(client.decimals(), 7);
}

#[test]
fn test_metadata_reflects_set_metadata() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let name = soroban_sdk::String::from_str(&env, "Example");
    let symbol = soroban_sdk::String::from_str(&env, "EX");
    client.set_metadata(&admin, &9, &name, &symbol);
    let meta = client.metadata();
    assert_eq!(meta.decimal, 9);
    assert_eq!(meta.name, name);
    assert_eq!(meta.symbol, symbol);
    assert_eq!(client.name(), name);
    assert_eq!(client.symbol(), symbol);
    assert_eq!(client.decimals(), 9);
}

#[test]
#[should_panic(expected = "metadata already set")]
fn test_set_metadata_write_once() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    let name = soroban_sdk::String::from_str(&env, "A");
    let symbol = soroban_sdk::String::from_str(&env, "A");
    client.set_metadata(&admin, &1, &name, &symbol);
    client.set_metadata(&admin, &1, &name, &symbol);
}

// ---------------------------------------------------------------------------
// TOK-31 – upgrade timelock snapshot coverage
// ---------------------------------------------------------------------------

/// Snapshot: full upgrade lifecycle. Asserts every field at every step so a
/// schema or behaviour regression (wrong unlock time, pending not cleared,
/// version not stamped, nonce not consumed) fails this test.
#[test]
fn test_execute_upgrade_after_timelock_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.upgrade(&admin, &0, &zero_hash(&env));
    let pending = client.pending_upgrade().unwrap();
    assert_eq!(pending.wasm_hash, zero_hash(&env));
    assert_eq!(pending.unlock_time, 1_000 + 86_400);
    assert_eq!(client.admin_nonce(), 1);

    // Exactly at unlock_time the timelock has elapsed.
    env.ledger().with_mut(|l| l.timestamp = 1_000 + 86_400);
    client.execute_upgrade(&admin, &1);
    assert!(client.pending_upgrade().is_none());
    assert_eq!(client.admin_nonce(), 2);

    client.migrate(&admin, &2);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);
    assert_eq!(client.admin_nonce(), 3);
}

/// Snapshot: cancelling a pending upgrade leaves balances, supply and
/// version untouched and frees the nonce for reuse by other admin calls.
#[test]
fn test_cancel_upgrade_leaves_state_snapshot() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.upgrade(&admin, &0, &zero_hash(&env));

    client.cancel_upgrade(&admin, &1);
    assert!(client.pending_upgrade().is_none());
    assert_eq!(client.total_supply(), 1_000);
    assert_eq!(client.balance(&admin), 1_000);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);
    assert_eq!(client.admin_nonce(), 2);

    // Nonce 2 is now valid for a subsequent admin call.
    client.migrate(&admin, &2);
    assert_eq!(client.version(), TOKEN_CONTRACT_VERSION);
}

// ---------------------------------------------------------------------------
// TOK-29 – admin multi-sig alignment with SEC-01
// ---------------------------------------------------------------------------

fn setup_multisig(env: &Env) -> (TokenContractClient<'static>, Address, Vec<Address>) {
    let id = env.register(TokenContract, ());
    let client = TokenContractClient::new(env, &id);
    let admin = Address::generate(env);
    let mut signers: Vec<Address> = Vec::new(env);
    for _ in 0..3 {
        signers.push_back(Address::generate(env));
    }
    client.initialize(&admin, &1_000);
    client.set_admin_signers(&admin, &0, &signers, &2);
    (client, admin, signers)
}

#[test]
fn test_multisig_admin_op_succeeds_with_threshold_signers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let user = Address::generate(&env);
    let approvals = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    client.mint_multisig(&admin, &user, &100, &1, &approvals);
    assert_eq!(client.balance(&user), 100);
    assert_eq!(client.admin_nonce(), 2);
}

#[test]
#[should_panic(expected = "T005: insufficient signers")]
fn test_multisig_admin_op_rejected_with_single_key() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let user = Address::generate(&env);
    let approvals = vec![&env, signers.get(0).unwrap()];
    client.mint_multisig(&admin, &user, &100, &1, &approvals);
}

#[test]
#[should_panic(expected = "T005: insufficient signers")]
fn test_multisig_admin_op_rejected_with_unknown_signer() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let user = Address::generate(&env);
    let approvals = vec![
        &env,
        signers.get(0).unwrap(),
        Address::generate(&env),
    ];
    client.mint_multisig(&admin, &user, &100, &1, &approvals);
}

#[test]
#[should_panic(expected = "T005: insufficient signers")]
fn test_multisig_admin_op_rejected_with_duplicate_signers() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let user = Address::generate(&env);
    let approvals = vec![
        &env,
        signers.get(0).unwrap(),
        signers.get(0).unwrap(),
    ];
    client.mint_multisig(&admin, &user, &100, &1, &approvals);
}

#[test]
#[should_panic(expected = "T006: single-key admin disabled")]
fn test_single_key_admin_path_disabled_on_mainnet() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _signers) = setup_multisig(&env);
    let user = Address::generate(&env);
    // The legacy single-key mint path must be unavailable once the
    // multi-sig policy is active (mainnet configuration).
    client.mint(&admin, &user, &100, &1);
}

// ---------------------------------------------------------------------------
// TOK-30 – emergency drain alignment with SEC-03
// ---------------------------------------------------------------------------

#[test]
#[should_panic(expected = "T007: token not paused")]
fn test_emergency_drain_requires_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let recipient = Address::generate(&env);
    let approvals = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    client.propose_emergency_drain(&admin, &1, &recipient, &approvals);
    client.execute_emergency_drain(&admin, &2, &approvals);
}

#[test]
#[should_panic(expected = "T008: no pending drain")]
fn test_emergency_drain_requires_proposal() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    client.pause_token(&admin, &1);
    let approvals = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    client.execute_emergency_drain(&admin, &2, &approvals);
}

#[test]
#[should_panic(expected = "T009: drain timelock active")]
fn test_emergency_drain_rejected_before_timelock() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let recipient = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.pause_token(&admin, &1);
    let approvals = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];
    client.propose_emergency_drain(&admin, &2, &recipient, &approvals);
    env.ledger().with_mut(|l| l.timestamp = 1_000 + 86_399);
    client.execute_emergency_drain(&admin, &3, &approvals);
}

#[test]
fn test_emergency_drain_full_flow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, signers) = setup_multisig(&env);
    let recipient = Address::generate(&env);
    let approvals = vec![&env, signers.get(0).unwrap(), signers.get(1).unwrap()];

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    client.pause_token(&admin, &1);
    assert!(client.is_paused());

    client.propose_emergency_drain(&admin, &2, &recipient, &approvals);
    let pending = client.pending_emergency_drain().unwrap();
    assert_eq!(pending.recipient, recipient);
    assert_eq!(pending.unlock_time, 1_000 + 86_400);

    env.ledger().with_mut(|l| l.timestamp = 1_000 + 86_400);
    let balance = client.balance(&admin);
    client.execute_emergency_drain(&admin, &3, &approvals);

    assert_eq!(client.balance(&recipient), balance);
    assert_eq!(client.balance(&admin), 0);
    assert_eq!(client.total_supply(), 0);
    assert!(client.pending_emergency_drain().is_none());
    assert!(!env.events().all().is_empty());
}
