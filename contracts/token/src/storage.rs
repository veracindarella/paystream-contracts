// SPDX-License-Identifier: Apache-2.0

use crate::types::TokenDataKey;
use soroban_sdk::{Address, Env, String};

/// Storage TTL thresholds (in ledgers), matching the stream contract.
/// Stellar produces ~1 ledger/5 s → 1 year ≈ 6_307_200 ledgers.
pub(crate) const TTL_THRESHOLD: u32 = 6_307_200; // ~1 year
pub(crate) const TTL_EXTEND_TO: u32 = 12_614_400; // ~2 years

pub fn balance_of(env: &Env, owner: &Address) -> i128 {
    let key = TokenDataKey::Balance(owner.clone());
    let store = env.storage().persistent();
    match store.get(&key) {
        Some(bal) => {
            store.extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
            bal
        }
        None => 0,
    }
}

pub fn set_balance(env: &Env, owner: &Address, amount: i128) {
    let key = TokenDataKey::Balance(owner.clone());
    let store = env.storage().persistent();
    store.set(&key, &amount);
    store.extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

pub fn allowance(env: &Env, owner: &Address, spender: &Address) -> i128 {
    let key = TokenDataKey::Allowance(owner.clone(), spender.clone());
    let store = env.storage().persistent();
    match store.get(&key) {
        Some(amount) => {
            store.extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
            amount
        }
        None => 0,
    }
}

pub fn set_allowance(env: &Env, owner: &Address, spender: &Address, amount: i128) {
    let key = TokenDataKey::Allowance(owner.clone(), spender.clone());
    let store = env.storage().persistent();
    store.set(&key, &amount);
    store.extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

pub fn total_supply(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&TokenDataKey::TotalSupply)
        .unwrap_or(0)
}

pub fn set_total_supply(env: &Env, supply: i128) {
    env.storage()
        .instance()
        .set(&TokenDataKey::TotalSupply, &supply);
}

pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&TokenDataKey::Admin)
        .expect("admin not set")
}

pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&TokenDataKey::Admin, admin);
}

pub fn get_name(env: &Env) -> String {
    env.storage()
        .instance()
        .get(&TokenDataKey::Name)
        .expect("name not set")
}

pub fn get_symbol(env: &Env) -> String {
    env.storage()
        .instance()
        .get(&TokenDataKey::Symbol)
        .expect("symbol not set")
}

pub fn set_metadata(env: &Env, name: &String, symbol: &String) {
    let store = env.storage().instance();
    store.set(&TokenDataKey::Name, name);
    store.set(&TokenDataKey::Symbol, symbol);
}
