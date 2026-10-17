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

/// Emitted when a stream receipt changes hands (PROD-01).
pub fn receipt_transferred(env: &Env, id: u64, from: &Address, to: &Address) {
    env.events()
        .publish((symbol_short!("receipt"), id), (from.clone(), to.clone()));
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
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("rate_upd"), stream_id),
        (old_rate, new_rate),
    );
}

/// Emitted when `update_stop_time` changes a stream's hard stop time.
pub fn stop_time_updated(env: &Env, stream_id: u64, old_stop_time: u64, new_stop_time: u64) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("stop_upd"), stream_id),
        (old_stop_time, new_stop_time),
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

pub fn auto_topup_set(env: &Env, stream_id: u64, enabled: bool) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("at_set"), stream_id),
        enabled,
    );
}

pub fn auto_topped_up(env: &Env, stream_id: u64, amount: i128, total_topped_up: i128) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("at_up"), stream_id),
        (amount, total_topped_up),
    );
}

/// Emitted by `withdraw` when remaining stream deposit falls below the low balance threshold.
pub fn low_balance(env: &Env, id: u64, employer: &Address, remaining: i128, threshold: i128) {
    env.events().publish(
        (symbol_short!("low_bal"), id),
        (employer.clone(), remaining, threshold),
    );
}

/// Emitted when `set_low_balance_threshold` updates the stream's alert threshold.
pub fn threshold_updated(env: &Env, stream_id: u64, old_threshold: i128, new_threshold: i128) {
    env.events().publish(
        (symbol_short!("v1"), symbol_short!("thresh_up"), stream_id),
        (old_threshold, new_threshold),
    );
}

