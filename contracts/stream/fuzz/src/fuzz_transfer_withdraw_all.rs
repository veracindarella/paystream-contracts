//! Fuzz target for TEST-61 (#576): transfer_receipt followed by withdraw_all.
//!
//! Invariants under test:
//!  * After `transfer_receipt`, the old holder can no longer withdraw from the
//!    stream (their `withdraw_all` returns an empty / zero result).
//!  * `withdraw_all` from the new holder credits the new holder only.
//!  * The employee stream index and `receipt_owner` remain consistent after the
//!    transfer (the stream is indexed under the new holder, and the recorded
//!    receipt owner matches the new holder).

use proptest::prelude::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token, Address, Env,
};

use stream::{StreamContract, StreamContractClient};

/// Bounded number of proptest cases to keep the fuzz job fast.
const CASES: u32 = 64;

/// Minimal harness that wires up the stream contract and a Stellar token.
struct Harness {
    env: Env,
    client: StreamContractClient<'static>,
    token: token::Client<'static>,
    token_admin: token::StellarAssetClient<'static>,
    contract_id: Address,
}

impl Harness {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        // Deterministic ledger so time advancement is predictable.
        env.ledger().set(LedgerInfo {
            timestamp: 1_000,
            protocol_version: 20,
            sequence_number: 1,
            network_id: Default::default(),
            base_reserve: 10,
            min_temp_entry_ttl: 16,
            min_persistent_entry_ttl: 16,
            max_entry_ttl: 6_312_000,
        });

        let contract_id = env.register_contract(None, StreamContract);
        let client = StreamContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract(admin.clone());
        let token = token::Client::new(&env, &token_id);
        let token_admin = token::StellarAssetClient::new(&env, &token_id);

        client.initialize(&admin, &token_id);

        Self {
            env,
            client,
            token,
            token_admin,
            contract_id,
        }
    }

    fn fund(&self, who: &Address, amount: i128) {
        self.token_admin.mint(who, &amount);
    }

    fn advance(&self, seconds: u64) {
        let mut info = self.env.ledger().get();
        info.timestamp += seconds;
        self.env.ledger().set(info);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: CASES,
        max_shrink_iters: 0,
        .. ProptestConfig::default()
    })]

    #[test]
    fn transfer_then_withdraw_all_credits_new_holder_only(
        deposit in 1_000i128..1_000_000i128,
        rate in 1i128..1_000i128,
        elapsed in 1u64..10_000u64,
    ) {
        let h = Harness::new();
        let env = &h.env;

        let employer = Address::generate(env);
        let old_holder = Address::generate(env);
        let new_holder = Address::generate(env);

        // Fund employer and approve the stream contract to pull the deposit.
        h.fund(&employer, deposit);
        h.token.approve(&employer, &h.contract_id, &deposit, &u32::MAX);

        // Create a stream for the original employee (old holder).
        let stream_id = h.client.create_stream(
            &employer,
            &old_holder,
            &h.token.address,
            &deposit,
            &rate,
            &0u64, // start now
            &0u64, // no cliff
            &0u64, // no explicit end; open-ended
        );

        // The stream must be indexed under the original employee.
        let old_streams = h.client.get_streams_by_employee(&old_holder);
        prop_assert!(old_streams.contains(&stream_id));
        prop_assert_eq!(h.client.get_receipt_owner(&stream_id), Some(old_holder.clone()));

        // Transfer the receipt to the new holder.
        h.client.transfer_receipt(&old_holder, &stream_id, &new_holder);

        // Indexes and receipt owner must be consistent after the transfer.
        let old_streams_after = h.client.get_streams_by_employee(&old_holder);
        prop_assert!(!old_streams_after.contains(&stream_id));

        let new_streams_after = h.client.get_streams_by_employee(&new_holder);
        prop_assert!(new_streams_after.contains(&stream_id));

        prop_assert_eq!(h.client.get_receipt_owner(&stream_id), Some(new_holder.clone()));

        // Advance time so there is something to withdraw.
        h.advance(elapsed);

        // Old holder must not be able to withdraw anything.
        let old_balance_before = h.token.balance(&old_holder);
        let old_withdrawn = h.client.withdraw_all(&old_holder, &stream_id);
        prop_assert_eq!(old_withdrawn, 0i128);
        prop_assert_eq!(h.token.balance(&old_holder), old_balance_before);

        // New holder withdraws everything accrued.
        let new_balance_before = h.token.balance(&new_holder);
        let new_withdrawn = h.client.withdraw_all(&new_holder, &stream_id);

        // The new holder is credited exactly the amount withdrawn.
        prop_assert_eq!(
            h.token.balance(&new_holder),
            new_balance_before + new_withdrawn
        );

        // Withdrawing again from the new holder yields nothing more.
        let new_balance_after = h.token.balance(&new_holder);
        let second_withdraw = h.client.withdraw_all(&new_holder, &stream_id);
        prop_assert_eq!(second_withdraw, 0i128);
        prop_assert_eq!(h.token.balance(&new_holder), new_balance_after);

        // Old holder still cannot withdraw after the new holder drained it.
        let old_balance_final = h.token.balance(&old_holder);
        let old_withdrawn_again = h.client.withdraw_all(&old_holder, &stream_id);
        prop_assert_eq!(old_withdrawn_again, 0i128);
        prop_assert_eq!(h.token.balance(&old_holder), old_balance_final);
    }
}

fn main() {}