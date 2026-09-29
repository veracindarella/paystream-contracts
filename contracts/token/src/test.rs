// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

use crate::storage::TTL_EXTEND_TO;
use crate::types::TokenDataKey;
use crate::{TokenContract, TokenContractClient};
use soroban_sdk::testutils::{Address as _, Ledger};
use soroban_sdk::{Address, Env};

fn setup() -> (Env, TokenContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let id = env.register(TokenContract, ());
    let client = TokenContractClient::new(&env, &id);
    (env, client)
}

fn init(env: &Env, client: &TokenContractClient, admin: &Address, supply: i128) {
    client.initialize(
        admin,
        &supply,
        &String::from_str(env, "PayStream Token"),
        &String::from_str(env, "PST"),
    );
}

#[test]
fn test_initialize() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    init(&env, &client, &admin, 1_000_000);
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
    init(&env, &client, &admin, 1_000);
    client.transfer(&admin, &user, &400);
    assert_eq!(client.balance(&admin), 600);
    assert_eq!(client.balance(&user), 400);
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
    init(&env, &client, &admin, 1_000);
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
    init(&env, &client, &admin, 1_000);
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
    init(&env, &client, &admin, 100);
    client.transfer(&admin, &user, &50);
    client.burn(&user, &200);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_transfer_overdraft() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    init(&env, &client, &admin, 100);
    client.transfer(&admin, &user, &999);
}

#[test]
fn test_admin_transfer_flow() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&new_admin);
    client.accept_admin(&new_admin);
    client.mint(&new_admin, &user, &10, &0);
    assert_eq!(client.balance(&user), 10);
}

#[test]
#[should_panic(expected = "not pending admin")]
fn test_accept_admin_wrong_address() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    client.initialize(&admin, &1_000);
    client.propose_admin(&Address::generate(&env));
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
    assert_eq!(client.allowance(&admin, &spender), (300, exp));
    env.ledger().with_mut(|l| l.sequence_number = exp);
    client.transfer_from(&spender, &admin, &to, &100);
    assert_eq!(client.balance(&to), 100);
    assert_eq!(client.allowance(&admin, &spender), (200, exp));
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
