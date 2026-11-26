// SPDX-License-Identifier: Apache-2.0

use crate::types::{PendingUpgrade, TokenDataKey, TokenMetadata};
use soroban_sdk::{Address, Env};

/// TOK-16: extend TTL when remaining TTL drops below ~1 year.
pub const TTL_THRESHOLD: u32 = 6_307_200;
/// TOK-16: extend TTL to ~2 years.
pub const TTL_EXTEND_TO: u32 = 12_614_400;

/// TOK-18: 24-hour upgrade time-lock, in seconds.
pub const UPGRADE_TIMELOCK: u64 = 86_400;

/// TOK-30: 24-hour emergency drain time-lock, in seconds.
pub const DRAIN_TIMELOCK: u64 = 86_400;

pub fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
}

pub fn balance_of(env: &Env, owner: &Address) -> i128 {
    let key = TokenDataKey::Balance(owner.clone());
    let balance = env.storage().persistent().get(&key);
    if balance.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    balance.unwrap_or(0)
}

pub fn set_balance(env: &Env, owner: &Address, amount: i128) {
    let key = TokenDataKey::Balance(owner.clone());
    env.storage().persistent().set(&key, &amount);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Returns `(amount, expiration_ledger)`; `(0, 0)` if no allowance is stored.
///
/// Expired allowances (`ledger > expiration_ledger`) report an amount of 0.
pub fn allowance(env: &Env, owner: &Address, spender: &Address) -> (i128, u32) {
    let key = TokenDataKey::Allowance(owner.clone(), spender.clone());
    let (amount, expiration_ledger): (i128, u32) =
        env.storage().temporary().get(&key).unwrap_or((0, 0));
    if env.ledger().sequence() > expiration_ledger {
        return (0, expiration_ledger);
    }
    extend_allowance_ttl(env, &key, expiration_ledger);
    (amount, expiration_ledger)
}

/// Keep the temporary allowance entry alive until its expiration ledger.
fn extend_allowance_ttl(env: &Env, key: &TokenDataKey, expiration_ledger: u32) {
    let live_for = expiration_ledger.saturating_sub(env.ledger().sequence());
    if live_for > 0 && env.storage().temporary().has(key) {
        env.storage().temporary().extend_ttl(key, live_for, live_for);
    }
}

pub fn set_allowance(
    env: &Env,
    owner: &Address,
    spender: &Address,
    amount: i128,
    expiration_ledger: u32,
) {
    let key = TokenDataKey::Allowance(owner.clone(), spender.clone());
    env.storage()
        .temporary()
        .set(&key, &(amount, expiration_ledger));
    extend_allowance_ttl(env, &key, expiration_ledger);
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

pub fn is_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&TokenDataKey::Paused)
        .unwrap_or(false)
}

pub fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&TokenDataKey::Paused, &paused);
}

/// TOK-15: 0 means uncapped.
pub fn max_supply(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&TokenDataKey::MaxSupply)
        .unwrap_or(0)
}

pub fn set_max_supply(env: &Env, cap: i128) {
    env.storage().instance().set(&TokenDataKey::MaxSupply, &cap);
}

/// TOK-32: write-once SEP-41 metadata.
pub fn has_metadata(env: &Env) -> bool {
    env.storage().instance().has(&TokenDataKey::Metadata)
}

pub fn get_metadata(env: &Env) -> Option<TokenMetadata> {
    env.storage().instance().get(&TokenDataKey::Metadata)
}

pub fn store_metadata(env: &Env, metadata: &TokenMetadata) {
    env.storage().instance().set(&TokenDataKey::Metadata, metadata);
}

/// TOK-18/TOK-10: pending upgrade bookkeeping.
pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&TokenDataKey::PendingUpgrade)
}

pub fn set_pending_upgrade(env: &Env, pending: &PendingUpgrade) {
    env.storage()
        .instance()
        .set(&TokenDataKey::PendingUpgrade, pending);
}

pub fn clear_pending_upgrade(env: &Env) {
    env.storage().instance().remove(&TokenDataKey::PendingUpgrade);
}

/// TOK-18: stamped contract version, 0 if never set.
pub fn version(env: &Env) -> u32 {
    env.storage().instance().get(&TokenDataKey::Version).unwrap_or(0)
}

pub fn set_version(env: &Env, version: u32) {
    env.storage().instance().set(&TokenDataKey::Version, &version);
}

/// TOK-33: optional per-recipient cap for `mint_to_batch`; 0 (default) means
/// no cap is enforced.
pub fn recipient_cap(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&TokenDataKey::RecipientCap)
        .unwrap_or(0)
}

pub fn set_recipient_cap(env: &Env, cap: i128) {
    env.storage()
        .instance()
        .set(&TokenDataKey::RecipientCap, &cap);
}
