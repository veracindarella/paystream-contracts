// SPDX-License-Identifier: Apache-2.0

//! SEP-41 token events.
//!
//! Topics and data follow the SEP-41 interface so wallets and indexers that
//! understand the Stellar Asset Contract can follow this token too.
#![allow(deprecated)] // `Events::publish` — topic layout must match SEP-41 exactly.

use crate::types::TransferMuxedData;
use soroban_sdk::{symbol_short, Address, Env};

/// `["transfer", from, to]` → `amount`, or `{amount, to_muxed_id}` for muxed recipients.
pub fn transfer(env: &Env, from: &Address, to: &Address, to_muxed_id: Option<u64>, amount: i128) {
    let topics = (symbol_short!("transfer"), from.clone(), to.clone());
    match to_muxed_id {
        Some(id) => env.events().publish(
            topics,
            TransferMuxedData {
                amount,
                to_muxed_id: id,
            },
        ),
        None => env.events().publish(topics, amount),
    }
}

/// `["approve", from, spender]` → `[amount, expiration_ledger]`.
pub fn approve(env: &Env, from: &Address, spender: &Address, amount: i128, expiration_ledger: u32) {
    env.events().publish(
        (symbol_short!("approve"), from.clone(), spender.clone()),
        (amount, expiration_ledger),
    );
}

/// `["burn", from]` → `amount`.
pub fn burn(env: &Env, from: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("burn"), from.clone()), amount);
}

/// `["mint", to]` → `amount` (admin interface, as on the SAC).
pub fn mint(env: &Env, to: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("mint"), to.clone()), amount);
}

/// `["em_dr_prp", recipient]` → `()` — an emergency drain has been proposed.
pub fn emergency_drain_proposed(env: &Env, recipient: &Address) {
    env.events()
        .publish((symbol_short!("em_dr_prp"), recipient.clone()), ());
}

/// `["em_dr_drn", recipient]` → `(token, amount)` — an emergency drain executed.
pub fn emergency_drained(env: &Env, recipient: &Address, token: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("em_dr_drn"), recipient.clone()),
        (token.clone(), amount),
    );
}
