// SPDX-License-Identifier: Apache-2.0

#![cfg(test)]

use crate::{TokenContract, TokenContractClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env};

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
fn test_mint_and_burn() {
    let (env, client) = setup();
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin, &1_000);
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
    client.initialize(&admin, &1_000);
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
    client.initialize(&admin, &1_000);
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

// ---------------------------------------------------------------------------
// Issue #69 – TEST-20: Property-based tests for token arithmetic
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// Helper: spin up a fresh token contract with `initial_supply` minted to admin.
    fn setup_with_supply(initial_supply: i128) -> (Env, TokenContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(TokenContract, ());
        let client = TokenContractClient::new(&env, &id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &initial_supply);
        (env, client, admin)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100_000))]

        /// transfer: total supply is conserved and no balance goes negative.
        #[test]
        fn prop_transfer_conserves_supply(
            supply in 1i128..=i128::MAX / 2,
            amount in 0i128..=i128::MAX / 2,
        ) {
            let amount = amount.min(supply); // cap transfer at available balance
            let (env, client, admin) = setup_with_supply(supply);
            let recipient = Address::generate(&env);

            let supply_before = client.total_supply();
            client.transfer(&admin, &recipient, &amount);
            let supply_after = client.total_supply();

            prop_assert_eq!(supply_before, supply_after, "transfer must not change total supply");
            prop_assert!(client.balance(&admin) >= 0, "sender balance must not go negative");
            prop_assert!(client.balance(&recipient) >= 0, "recipient balance must not go negative");
            prop_assert_eq!(
                client.balance(&admin) + client.balance(&recipient),
                supply,
                "sum of balances must equal total supply"
            );
        }

        /// mint: total supply increases by the exact amount minted.
        #[test]
        fn prop_mint_increases_supply_by_exact_amount(
            supply in 0i128..=i128::MAX / 2,
            mint_amount in 1i128..=i128::MAX / 2,
        ) {
            // Ensure supply + mint_amount doesn't overflow
            prop_assume!(supply.checked_add(mint_amount).is_some());

            let (env, client, admin) = setup_with_supply(supply);
            let recipient = Address::generate(&env);

            let supply_before = client.total_supply();
            client.mint(&admin, &recipient, &mint_amount);
            let supply_after = client.total_supply();

            prop_assert_eq!(
                supply_after - supply_before,
                mint_amount,
                "total supply must increase by exactly the minted amount"
            );
            prop_assert!(client.balance(&recipient) >= 0, "minted balance must not be negative");
        }

        /// burn: total supply decreases by the exact amount burned.
        #[test]
        fn prop_burn_decreases_supply_by_exact_amount(
            supply in 1i128..=i128::MAX / 2,
            burn_amount in 1i128..=i128::MAX / 2,
        ) {
            let burn_amount = burn_amount.min(supply); // cap burn at available balance
            let (env, client, admin) = setup_with_supply(supply);

            let supply_before = client.total_supply();
            client.burn(&admin, &burn_amount);
            let supply_after = client.total_supply();

            prop_assert_eq!(
                supply_before - supply_after,
                burn_amount,
                "total supply must decrease by exactly the burned amount"
            );
            prop_assert!(client.balance(&admin) >= 0, "balance after burn must not be negative");
        }
    }
}
