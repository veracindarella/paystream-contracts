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

/// Returns `(amount, expiration_ledger)`; `(0, 0)` if no allowance is stored.
pub fn allowance(env: &Env, owner: &Address, spender: &Address) -> (i128, u32) {
    env.storage()
        .temporary()
        .get(&TokenDataKey::Allowance(owner.clone(), spender.clone()))
        .unwrap_or((0, 0))
}

pub fn set_allowance(
    env: &Env,
    owner: &Address,
    spender: &Address,
    amount: i128,
    expiration_ledger: u32,
) {
    env.storage().temporary().set(
        &TokenDataKey::Allowance(owner.clone(), spender.clone()),
        &(amount, expiration_ledger),
    );
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

pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&TokenDataKey::Admin)
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

pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&TokenDataKey::PendingAdmin)
}

pub fn set_pending_admin(env: &Env, admin: &Address) {
    env.storage()
        .instance()
        .set(&TokenDataKey::PendingAdmin, admin);
}

pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&TokenDataKey::PendingAdmin);
}

pub fn admin_nonce(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&TokenDataKey::AdminNonce)
        .unwrap_or(0)
}

/// Asserts `nonce` matches the stored admin nonce, then increments it.
pub fn consume_admin_nonce(env: &Env, nonce: u64) {
    let current = admin_nonce(env);
    assert!(nonce == current, "invalid nonce");
    env.storage()
        .instance()
        .set(&TokenDataKey::AdminNonce, &(current + 1));
}
