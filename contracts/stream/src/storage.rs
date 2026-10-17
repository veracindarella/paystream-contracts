// SPDX-License-Identifier: Apache-2.0

use crate::types::{
    AutoTopupConfig, DataKey, LinearVestSchedule, PendingUpgrade, ProtocolFeeConfig, Stream,
    StreamParams, StreamStatus, StreamType, ERR_ADMIN_NOT_SET, ERR_BAD_NONCE, ERR_OVERFLOW,
};
use soroban_sdk::{Address, BytesN, Env, Vec};

/// Minimum delay (in seconds) between proposing and executing an upgrade.
/// Default: 172 800 s = 48 hours.
pub const TIMELOCK_DELAY: u64 = 172_800;

/// Default minimum deposit (10_000 stroops = 0.001 XLM equivalent).
pub const DEFAULT_MIN_DEPOSIT: i128 = 10_000;

/// Grace period after stop_time before a stream may be expired by admin (in seconds).
/// Default: 7 days = 604_800 s. Admin-callable expire_streams skips streams that
/// still have unclaimed tokens or that have not yet crossed stop_time + GRACE_PERIOD.
pub const GRACE_PERIOD: u64 = 604_800; // 7 days

/// Persistent storage TTL thresholds (in ledgers).
/// Stellar produces ~1 ledger/5 s → 1 year ≈ 6_307_200 ledgers.
/// We keep stream data alive for at least 1 year and extend to 2 years on
/// every active-stream operation so long-running streams never expire.
const TTL_THRESHOLD: u32 = 6_307_200; // ~1 year
const TTL_EXTEND_TO: u32 = 12_614_400; // ~2 years

/// Persist a stream and extend its storage lifetime.
pub fn save_stream(env: &Env, stream: &Stream) {
    let key = DataKey::Stream(stream.id);
    env.storage().persistent().set(&key, stream);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Load a stream by ID and extend its storage lifetime when present.
pub fn load_stream(env: &Env, id: u64) -> Option<Stream> {
    let key = DataKey::Stream(id);
    let stream: Option<Stream> = env.storage().persistent().get(&key);
    if stream.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    stream
}

/// Allocate and persist the next sequential stream ID.
pub fn next_id(env: &Env) -> u64 {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::StreamCount)
        .unwrap_or(0);
    // Saturating add: stream IDs will never realistically reach u64::MAX, but
    // we use checked arithmetic throughout as a policy.
    let next = count.checked_add(1).expect("stream count overflow");
    env.storage().instance().set(&DataKey::StreamCount, &next);
    next
}

/// Store the contract administrator address.
pub fn set_admin(env: &Env, admin: &Address) {
    env.storage().instance().set(&DataKey::Admin, admin);
}

#[allow(dead_code)]
/// Return the configured administrator, panicking if it is not initialized.
pub fn get_admin(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .expect(ERR_ADMIN_NOT_SET)
}

/// Store the address nominated as the next administrator.
pub fn set_pending_admin(env: &Env, pending: &Address) {
    env.storage()
        .instance()
        .set(&DataKey::PendingAdmin, pending);
}

/// Return the nominated administrator, if a transfer is pending.
pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&DataKey::PendingAdmin)
}

/// Store the nonce bound to the pending administrator proposal.
pub fn set_pending_admin_nonce(env: &Env, nonce: u64) {
    env.storage()
        .instance()
        .set(&DataKey::PendingAdminNonce, &nonce);
}

/// Return the nonce bound to the pending administrator proposal.
pub fn get_pending_admin_nonce(env: &Env) -> Option<u64> {
    env.storage()
        .instance()
        .get(&DataKey::PendingAdminNonce)
}

/// Clear the pending administrator and its associated nonce.
pub fn clear_pending_admin(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingAdmin);
    env.storage()
        .instance()
        .remove(&DataKey::PendingAdminNonce);
}

/// Return the configured minimum deposit, or the default when unset.
pub fn get_min_deposit(env: &Env) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::MinDeposit)
        .unwrap_or(DEFAULT_MIN_DEPOSIT)
}

/// Store the minimum gross deposit required to create a stream.
pub fn set_min_deposit(env: &Env, amount: i128) {
    env.storage().instance().set(&DataKey::MinDeposit, &amount);
}

/// Return the configured fee and treasury, or `(0, None)` when unset.
pub fn get_protocol_fee(env: &Env) -> (u16, Option<Address>) {
    match env
        .storage()
        .instance()
        .get::<DataKey, ProtocolFeeConfig>(&DataKey::ProtocolFee)
    {
        Some(config) => (config.fee_bps, Some(config.treasury)),
        None => (0, None),
    }
}

/// Store the configured protocol fee and treasury address.
pub fn set_protocol_fee(env: &Env, fee_bps: u16, treasury: &Address) {
    env.storage().instance().set(
        &DataKey::ProtocolFee,
        &ProtocolFeeConfig {
            fee_bps,
            treasury: treasury.clone(),
        },
    );
}

/// Return a stream's type, defaulting to `Payroll` for legacy streams.
pub fn get_stream_type(env: &Env, stream_id: u64) -> StreamType {
    let key = DataKey::StreamType(stream_id);
    let stream_type: StreamType = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or(StreamType::Payroll);
    if stream_type == StreamType::LinearVest {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    stream_type
}

/// Return the linear vest schedule for a stream, if one is stored.
pub fn get_linear_vest_schedule(env: &Env, stream_id: u64) -> Option<LinearVestSchedule> {
    let key = DataKey::LinearVestSchedule(stream_id);
    let schedule: Option<LinearVestSchedule> = env.storage().persistent().get(&key);
    if schedule.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    schedule
}

/// Store a linear vest schedule and mark its stream as `LinearVest`.
pub fn set_linear_vest_schedule(env: &Env, stream_id: u64, schedule: &LinearVestSchedule) {
    let type_key = DataKey::StreamType(stream_id);
    let schedule_key = DataKey::LinearVestSchedule(stream_id);
    env.storage()
        .persistent()
        .set(&type_key, &StreamType::LinearVest);
    env.storage().persistent().set(&schedule_key, schedule);
    env.storage()
        .persistent()
        .extend_ttl(&type_key, TTL_THRESHOLD, TTL_EXTEND_TO);
    env.storage()
        .persistent()
        .extend_ttl(&schedule_key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Tokens earned by employee up to `now` that have not yet been withdrawn.
///
/// Includes any `pending_accrual` banked by a previous `update_rate` call
/// so that earnings from before a rate change are not lost.
///
/// All arithmetic uses checked or saturating operations to prevent overflow
/// with large `rate_per_second` or `elapsed` values (see issue #2).
pub fn claimable_amount(stream: &Stream, now: u64) -> i128 {
    match stream.status {
        StreamStatus::Cancelled | StreamStatus::Exhausted => return 0,
        _ => {}
    }
    // Cap at stop_time in one expression to avoid a branch in the common case.
    let effective_end = if stream.stop_time > 0 && now > stream.stop_time {
        stream.stop_time
    } else {
        now
    };
    // saturating_sub: elapsed is always >= 0 after this
    let elapsed = effective_end.saturating_sub(stream.last_withdraw_time) as i128;

    // checked_mul: panic with a descriptive message on overflow rather than
    // silently wrapping and producing an incorrect (possibly negative) payout.
    let earned = elapsed
        .checked_mul(stream.rate_per_second)
        .expect(ERR_OVERFLOW);

    // Add any accrual banked by a previous update_rate call so that earnings
    // from before a rate change are not lost.
    let total_earned = earned
        .checked_add(stream.pending_accrual)
        .expect(ERR_OVERFLOW);

    // remaining can never be negative for a well-formed stream, but clamp to 0
    // defensively.
    let remaining = stream
        .deposit
        .checked_sub(stream.withdrawn)
        .unwrap_or(0)
        .max(0);

    total_earned.min(remaining).max(0)
}

/// Return the vested amount available to withdraw under a linear schedule.
pub fn linear_vest_claimable_amount(
    stream: &Stream,
    schedule: &LinearVestSchedule,
    now: u64,
) -> i128 {
    if stream.status == StreamStatus::Cancelled || stream.status == StreamStatus::Exhausted {
        return 0;
    }

    let vest_period = schedule.vest_period as i128;
    assert!(vest_period > 0, "vest period must be greater than zero");
    let current_time = if stream.status == StreamStatus::Paused {
        stream.last_withdraw_time
    } else {
        now
    };
    let elapsed = current_time.saturating_sub(stream.start_time);
    if schedule.cliff.is_some_and(|cliff| elapsed < cliff) {
        return 0;
    }

    let elapsed = elapsed.min(schedule.vest_period);
    let elapsed_i128 = elapsed as i128;
    let whole = (schedule.total_grant / vest_period)
        .checked_mul(elapsed_i128)
        .expect(ERR_OVERFLOW);
    let remainder = (schedule.total_grant % vest_period) as u128;
    let fraction = (remainder * elapsed as u128 / schedule.vest_period as u128) as i128;
    let vested = whole.checked_add(fraction).expect(ERR_OVERFLOW);
    let unwithdrawn_grant = vested
        .checked_sub(stream.withdrawn)
        .unwrap_or(0)
        .max(0);
    let remaining = stream
        .deposit
        .checked_sub(stream.withdrawn)
        .unwrap_or(0)
        .max(0);
    unwithdrawn_grant.min(remaining)
}

/// Append `stream_id` to the employer's stream index.
/// Called once per `create_stream`; O(1) amortised — no full scan.
pub fn index_employer_stream(env: &Env, employer: &Address, stream_id: u64) {
    let key = DataKey::EmployerStreams(employer.clone());
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(stream_id);
    env.storage().persistent().set(&key, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Return all stream IDs owned by `employer`.
///
/// Extends TTL on read (matching the `load_stream` pattern) so that the index
/// does not expire for employers with long-running streams and no new stream
/// creation activity (SC-15).
pub fn get_employer_streams(env: &Env, employer: &Address) -> Vec<u64> {
    let key = DataKey::EmployerStreams(employer.clone());
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    ids
}

/// Remove `stream_id` from the employer's stream index.
///
/// Used by `transfer_stream` to deindex the old employer. Performs a linear
/// scan of the stored Vec and rebuilds it without the target ID. O(n) where
/// n is the number of streams owned by the employer; acceptable because
/// employers are expected to have at most hundreds of concurrent streams.
pub fn remove_employer_stream(env: &Env, employer: &Address, stream_id: u64) {
    let key = DataKey::EmployerStreams(employer.clone());
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    let mut new_ids: Vec<u64> = Vec::new(env);
    for id in ids.iter() {
        if id != stream_id {
            new_ids.push_back(id);
        }
    }
    env.storage().persistent().set(&key, &new_ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Append `stream_id` to the employee's stream index.
pub fn index_employee_stream(env: &Env, employee: &Address, stream_id: u64) {
    let key = DataKey::EmployeeStreams(employee.clone());
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(stream_id);
    env.storage().persistent().set(&key, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Return all stream IDs paying `employee`.
///
/// Extends TTL on read (matching the `load_stream` pattern) so that the index
/// does not expire for employees with long-running streams and no new stream
/// creation activity (SC-15).
pub fn get_employee_streams(env: &Env, employee: &Address) -> Vec<u64> {
    let key = DataKey::EmployeeStreams(employee.clone());
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    ids
}

/// Append `stream_id` to the token's stream index.
pub fn index_token_stream(env: &Env, token: &Address, stream_id: u64) {
    let key = DataKey::TokenStreams(token.clone());
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(stream_id);
    env.storage().persistent().set(&key, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Return all stream IDs funded with `token`.
pub fn get_token_streams(env: &Env, token: &Address) -> Vec<u64> {
    let key = DataKey::TokenStreams(token.clone());
    let ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    ids
}

/// Return a stream's auto top-up configuration, extending its storage lifetime.
pub fn get_auto_topup(env: &Env, stream_id: u64) -> Option<AutoTopupConfig> {
    let key = DataKey::AutoTopup(stream_id);
    let config: Option<AutoTopupConfig> = env.storage().persistent().get(&key);
    if config.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    config
}

/// Store an auto top-up configuration and extend its storage lifetime.
pub fn set_auto_topup(env: &Env, stream_id: u64, config: &AutoTopupConfig) {
    let key = DataKey::AutoTopup(stream_id);
    env.storage().persistent().set(&key, config);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Remove a stream's auto top-up configuration.
pub fn remove_auto_topup(env: &Env, stream_id: u64) {
    let key = DataKey::AutoTopup(stream_id);
    env.storage().persistent().remove(&key);
}

// ---------------------------------------------------------------------------
// Admin nonce helpers (issue #70 — replay attack protection)
// ---------------------------------------------------------------------------

/// Return the current admin nonce (0 if never set).
pub fn get_admin_nonce(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&DataKey::AdminNonce)
        .unwrap_or(0u64)
}

/// Verify `nonce` equals the stored nonce, then increment it atomically.
///
/// # Panics
/// - E009 if `nonce` does not match the expected value.
pub fn consume_admin_nonce(env: &Env, nonce: u64) {
    let expected = get_admin_nonce(env);
    assert!(nonce == expected, "{}", ERR_BAD_NONCE);
    env.storage()
        .instance()
        .set(&DataKey::AdminNonce, &(expected + 1));
}

// ---------------------------------------------------------------------------
// Upgrade time-lock helpers (SEC-02)
// ---------------------------------------------------------------------------

/// Store a pending upgrade record.
pub fn set_pending_upgrade(env: &Env, upgrade: &PendingUpgrade) {
    env.storage()
        .instance()
        .set(&DataKey::PendingUpgrade, upgrade);
}

/// Retrieve the pending upgrade record, if any.
pub fn get_pending_upgrade(env: &Env) -> Option<PendingUpgrade> {
    env.storage().instance().get(&DataKey::PendingUpgrade)
}

/// Remove the pending upgrade record.
pub fn clear_pending_upgrade(env: &Env) {
    env.storage().instance().remove(&DataKey::PendingUpgrade);
}

// ---------------------------------------------------------------------------
// Time-based stream index (issue #21)
// ---------------------------------------------------------------------------

/// Number of seconds in one day — used to derive the day-bucket key from a
/// Unix timestamp.  Streams created within the same UTC day land in the same
/// bucket, giving O(1) per-create writes and O(buckets) range scans.
pub const SECONDS_PER_DAY: u64 = 86_400;

/// Return the day-bucket key for `timestamp` (i.e. `timestamp / 86_400`).
pub fn day_bucket(timestamp: u64) -> u64 {
    timestamp / SECONDS_PER_DAY
}

/// Append `stream_id` to the day-bucket that covers `timestamp`.
///
/// Called once per `create_stream` / `create_streams_batch`.  O(1) amortised.
/// TTL is extended on every write so active days never expire before their
/// streams do.
pub fn index_stream_by_timestamp(env: &Env, timestamp: u64, stream_id: u64) {
    let bucket = day_bucket(timestamp);
    let key = DataKey::StreamsByTimestamp(bucket);
    let mut ids: Vec<u64> = env
        .storage()
        .persistent()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    ids.push_back(stream_id);
    env.storage().persistent().set(&key, &ids);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Return all stream IDs created in the half-open range [`from_ts`, `to_ts`).
///
/// Iterates every day-bucket that overlaps the range and accumulates the IDs.
/// Buckets with no data are skipped.  IDs within a bucket are in insertion
/// order (creation order within a day is preserved).
///
/// # Parameters
/// - `from_ts` — inclusive lower bound (Unix timestamp in seconds)
/// - `to_ts`   — exclusive upper bound (Unix timestamp in seconds)
///
/// # Returns
/// `Vec<u64>` of stream IDs; empty if no streams were created in the range.
pub fn get_streams_in_range(env: &Env, from_ts: u64, to_ts: u64) -> Vec<u64> {
    let mut result: Vec<u64> = Vec::new(env);
    if from_ts >= to_ts {
        return result;
    }
    let start_bucket = day_bucket(from_ts);
    // to_ts is exclusive; the last bucket to check is the one that contains
    // the timestamp just before to_ts.
    let end_bucket = day_bucket(to_ts.saturating_sub(1));

    let mut bucket = start_bucket;
    loop {
        let key = DataKey::StreamsByTimestamp(bucket);
        if let Some(ids) = env.storage().persistent().get::<DataKey, Vec<u64>>(&key) {
            for id in ids.iter() {
                result.push_back(id);
            }
        }
        if bucket >= end_bucket {
            break;
        }
        bucket += 1;
    }
    result
}
