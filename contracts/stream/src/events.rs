// SPDX-License-Identifier: Apache-2.0

use crate::types::StreamStatus;
use soroban_sdk::{symbol_short, Address, Env};

pub fn contract_initialized(env: &Env, admin: &Address) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("init"),), admin.clone());
}

pub fn stream_created(env: &Env, id: u64, employer: &Address, employee: &Address, rate: i128) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("created"), id),
        (employer.clone(), employee.clone(), rate),
    );
}

pub fn withdrawn(env: &Env, id: u64, employee: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("withdraw"), id), (employee.clone(), amount));
}

pub fn stream_status_changed(env: &Env, id: u64, status: &StreamStatus) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("status"), id), status.clone());
}

/// Emitted when a stream is cancelled, recording the exact cash-flow split.
///
/// - `claimable_amount` — tokens paid to the employee at cancellation time
/// - `refund_amount`    — tokens returned to the employer (unstreamed portion)
pub fn stream_cancelled(env: &Env, id: u64, claimable_amount: i128, refund_amount: i128) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("cancelled"), id),
        (claimable_amount, refund_amount),
    );
}

pub fn topped_up(env: &Env, id: u64, employer: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("topup"), id), (employer.clone(), amount));
}

pub fn contract_paused(env: &Env, paused: bool) {
    env.events().publish((symbol_short!("v1"), symbol_short!("paused"),), paused);
}

/// Emitted when `update_rate` changes the stream's `rate_per_second`.
pub fn rate_updated(env: &Env, stream_id: u64, old_rate: i128, new_rate: i128) {
    env.events()
        .publish((symbol_short!("rate_upd"), stream_id), (old_rate, new_rate));
}

/// Emitted when the employer unlocks a milestone amount on a stream.
pub fn milestone_unlocked(env: &Env, stream_id: u64, employer: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("rate_upd"), stream_id),
        (old_rate, new_rate),
    );
}

/// Emitted by `expire_streams` when an eligible stream is transitioned to Exhausted.
pub fn stream_expired(env: &Env, stream_id: u64) {
    env.events()
        .publish((symbol_short!("expired"), stream_id), ());
}

pub fn upgrade_proposed(env: &Env, wasm_hash: &soroban_sdk::BytesN<32>, unlock_time: u64) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("upg_prop"),), (wasm_hash.clone(), unlock_time));
}

pub fn upgrade_executed(env: &Env, wasm_hash: &soroban_sdk::BytesN<32>) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("upg_exec"),), wasm_hash.clone());
}

pub fn upgrade_cancelled(env: &Env) {
    env.events()
        .publish((symbol_short!("v1"), symbol_short!("upg_cncl"),), ());
}

pub fn stream_transferred(env: &Env, stream_id: u64, old_employer: &Address, new_employer: &Address) {
    env.events().publish(
        (symbol_short!("xfr_strm"), stream_id),
        (old_employer.clone(), new_employer.clone()),
    );
}
