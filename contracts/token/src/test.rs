// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

use crate::storage::TTL_EXTEND_TO;
use crate::types::TokenDataKey;
use crate::{TokenContract, TokenContractClient};
use soroban_sdk::testutils::storage::Persistent;
use soroban_sdk::testutils::{Address as _, Events, Ledger};
use soroban_sdk::{symbol_short, vec, Address, Env, IntoVal, String};

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
    init(&env, &client, &admin, 1_000);
    client.mint(&admin, &user, &500);
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
    client.approve(&user, &spender, &300);
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
    client.approve(&user, &spender, &100);
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
fn test_metadata() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    init(&env, &client, &admin, 1_000);
    assert_eq!(client.name(), String::from_str(&env, "PayStream Token"));
    assert_eq!(client.symbol(), String::from_str(&env, "PST"));
    assert_eq!(client.decimals(), 7);
}

#[test]
fn test_allowance_query() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    let to = Address::generate(&env);
    init(&env, &client, &admin, 1_000);
    assert_eq!(client.allowance(&admin, &spender), 0);
    client.approve(&admin, &spender, &300);
    assert_eq!(client.allowance(&admin, &spender), 300);
    client.transfer_from(&spender, &admin, &to, &100);
    assert_eq!(client.allowance(&admin, &spender), 200);
}

#[test]
fn test_events() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    init(&env, &client, &admin, 1_000);

    client.transfer(&admin, &user, &100);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("transfer"), admin.clone(), user.clone()).into_val(&env),
                100_i128.into_val(&env),
            ),
        ]
    );

    client.approve(&user, &admin, &50);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("approve"), user.clone(), admin.clone()).into_val(&env),
                50_i128.into_val(&env),
            ),
        ]
    );

    client.mint(&admin, &user, &10);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("mint"), admin.clone(), user.clone()).into_val(&env),
                10_i128.into_val(&env),
            ),
        ]
    );

    client.burn(&user, &5);
    assert_eq!(
        env.events().all(),
        vec![
            &env,
            (
                client.address.clone(),
                (symbol_short!("burn"), user.clone()).into_val(&env),
                5_i128.into_val(&env),
            ),
        ]
    );
}

#[test]
fn test_ttl_extended() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let spender = Address::generate(&env);
    env.ledger()
        .with_mut(|l| l.max_entry_ttl = TTL_EXTEND_TO + 1);
    init(&env, &client, &admin, 1_000);
    client.approve(&admin, &spender, &10);

    client.balance(&admin);
    client.allowance(&admin, &spender);

    env.as_contract(&client.address, || {
        let bal_ttl = env
            .storage()
            .persistent()
            .get_ttl(&TokenDataKey::Balance(admin.clone()));
        let allow_ttl = env
            .storage()
            .persistent()
            .get_ttl(&TokenDataKey::Allowance(admin.clone(), spender.clone()));
        assert_eq!(bal_ttl, TTL_EXTEND_TO);
        assert_eq!(allow_ttl, TTL_EXTEND_TO);
    });
}
