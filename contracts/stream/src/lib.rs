// SPDX-License-Identifier: Apache-2.0

//! PayStream salary-streaming contract: employers lock tokens that accrue to
//! employees per second and can be withdrawn at any time.

#![no_std]
// `missing_docs` is denied per module (storage, validate, utils) rather than
// crate-wide: since soroban-sdk 28 the `#[contract]` / `#[contracttype]`
// macros emit undocumented public items into the crate root and `types`,
// which no attribute can reach. Hand-written items there are still documented.

mod events;
/// Persistent storage helpers and stream accounting calculations.
pub mod storage;
/// Public contract data types, storage keys, and error constants.
pub mod types;
/// Network helpers such as the native XLM SAC address.
pub mod utils;
/// Input validation shared by the stream entrypoints.
pub mod validate;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, Map, Symbol, Vec};

/// Compile-time contract version.  Increment this constant with every
/// WASM upgrade so that `migrate` stamps the new version into instance
/// storage and `version()` can be queried off-chain.
pub const CONTRACT_VERSION: u32 = 1;
use storage::{
    set_protocol_fee,
    check_and_increment_create_rate, get_stream_ended_at, load_stream_any, set_split_beneficiaries,
    ARCHIVE_WAIT_PERIOD, GRACE_PERIOD, SECONDS_PER_DAY, get_linear_vest_schedule, get_protocol_fee, get_stream_type, linear_vest_claimable_amount, set_linear_vest_schedule, set_stream_ended_at, set_max_creates_per_ledger as store_max_creates,
    accrual_start, claimable_amount, clear_pending_admin, clear_pending_upgrade,
    consume_admin_nonce, delete_template, get_admin, get_admin_nonce, get_auto_topup,
    get_employee_streams, get_employer_streams, get_max_deposit, get_min_deposit, get_pending_admin,
    get_pending_admin_nonce, get_pending_upgrade, get_stream_cliff, get_streams_in_range,
    get_streams_in_time_range, get_template_ids, get_token_streams, index_employee_stream,
    index_employer_stream, index_stream_by_timestamp, index_token_stream, load_stream,
    load_template, mark_cliff_reached, next_id, receipt_owner, remove_auto_topup,
    remove_employer_stream, save_stream, save_template, set_admin, set_auto_topup, set_max_deposit,
    set_min_deposit, set_pending_admin, set_pending_admin_nonce, set_pending_upgrade,
    set_receipt_owner, set_stream_cliff, stream_ids_by_employee, stream_ids_by_employer,
    TIMELOCK_DELAY,
};
use types::{
    AutoTopupConfig, PendingUpgrade, SplitBeneficiary,
    ERR_ABOVE_MAX_DEPOSIT, ERR_ARCHIVE_TOO_EARLY, ERR_BELOW_MIN_DEPOSIT, ERR_INVALID_AUTO_TOPUP, ERR_INVALID_RATE, ERR_NO_AUTO_TOPUP, ERR_NO_PENDING_UPGRADE, ERR_STOP_TIME_PAST, ERR_STREAM_NOT_ENDED, ERR_UPGRADE_TIMELOCK_ACTIVE, ERR_ZERO_RATE,
    DataKey, LinearVestSchedule, Stream, StreamParams, StreamStatus, StreamType,
    ERR_ADMIN_NOT_SET, ERR_BAD_PENDING_NONCE,
    ERR_BATCH_TOO_LARGE, ERR_CONTRACT_PAUSED, ERR_EMPTY_PARAMS, ERR_NEW_EMPLOYER_IS_EMPLOYEE, ERR_NOT_ADMIN,
    ERR_INVALID_STOP_TIME, ERR_NOT_EMPLOYEE, ERR_NOT_EMPLOYER, ERR_NOT_PENDING_ADMIN,
    ERR_NO_PENDING_ADMIN, ERR_OVERFLOW,
    ERR_REENTRANT, ERR_STREAM_ALREADY_ENDED, ERR_STREAM_CANCELLED, ERR_STREAM_EXHAUSTED,
    ERR_STREAM_NOT_ACTIVE, ERR_STREAM_NOT_FOUND, ERR_STREAM_NOT_PAUSED, ERR_ZERO_DEPOSIT,
    ERR_NOT_RECEIPT_OWNER, ERR_TEMPLATE_NOT_FOUND,
    MAX_BATCH_SIZE,
};
use validate::{
    validate_cliff, validate_create_stream, validate_rate, validate_split_beneficiaries,
    validate_top_up,
};

fn get_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

fn protocol_fee_amount(deposit: i128, fee_bps: u32) -> i128 {
    let fee_bps = fee_bps as i128;
    (deposit / 10_000) * fee_bps + ((deposit % 10_000) * fee_bps) / 10_000
}

fn stream_claimable_amount(env: &Env, stream: &Stream, now: u64) -> i128 {
    match get_linear_vest_schedule(env, stream.id) {
        Some(schedule) => linear_vest_claimable_amount(stream, &schedule, now),
        None => claimable_amount(stream, now),
    }
}

fn assert_payroll_stream(env: &Env, stream_id: u64) {
    assert_eq!(
        get_stream_type(env, stream_id),
        StreamType::Payroll,
        "operation not supported for linear vest streams"
    );
}

/// Applies a pre-authorized auto top-up after a withdraw when the stream's
/// remaining balance falls below the configured trigger threshold.
///
/// Uses `transfer_from`, so the employer must have approved the contract as
/// spender. The top-up is skipped (never reverts the withdraw) when the cap is
/// reached, the stream has ended, or allowance/balance is insufficient.
fn maybe_auto_topup(env: &Env, stream: &mut Stream, now: u64) {
    let mut config = match get_auto_topup(env, stream.id) {
        Some(c) => c,
        None => return,
    };
    if stream.stop_time > 0 && now >= stream.stop_time {
        return;
    }
    let remaining = stream.deposit - stream.withdrawn;
    if remaining >= config.trigger_threshold {
        return;
    }
    let amount = config
        .topup_amount
        .min(config.max_total - config.total_topped_up);
    if amount <= 0 {
        return;
    }

    let this = env.current_contract_address();
    let token_client = token::Client::new(env, &stream.token);
    if !matches!(
        token_client.try_transfer_from(&this, &stream.employer, &this, &amount),
        Ok(Ok(()))
    ) {
        return;
    }

    stream.deposit = stream.deposit.checked_add(amount).expect(ERR_OVERFLOW);
    if stream.status == StreamStatus::Exhausted {
        stream.status = StreamStatus::Active;
    }
    config.total_topped_up += amount;
    set_auto_topup(env, stream.id, &config);
    events::auto_topped_up(env, stream.id, amount, config.total_topped_up);
}

#[contract]
/// PayStream contract entry points.
pub struct StreamContract;

#[contractimpl]
impl StreamContract {
    /// Initialise the contract with an admin address.
    ///
    /// Must be called once after deployment. The `admin` address gains the
    /// ability to pause/unpause the contract, set the minimum deposit, and
    /// perform upgrades.
    ///
    /// Emits a `contract_initialized` event on success so off-chain indexers
    /// can determine when and by whom the contract was initialised without
    /// scanning raw ledger metadata.
    ///
    /// # Parameters
    /// - `admin` — address that becomes the contract admin (requires auth)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails
    /// - Panics with "already initialized" if the contract has already been initialised
    pub fn initialize(env: Env, admin: Address) {
        assert!(
            !env.storage().instance().has(&DataKey::Admin),
            "already initialized"
        );
        admin.require_auth();
        set_admin(&env, &admin);
        events::contract_initialized(&env, &admin);
    }

    /// Step 1 of two-step admin transfer: current admin proposes a new admin.
    ///
    /// The nominated address must call [`accept_admin`] with the same `nonce`
    /// to complete the transfer. Binding a nonce to the proposal makes the
    /// transfer intent non-replayable: an attacker who observes the proposal
    /// on-chain cannot front-run acceptance without knowing the nonce.
    ///
    /// # Parameters
    /// - `new_admin` — address being nominated as the next admin
    /// - `nonce` — current admin nonce; consumed here for replay protection
    ///   and also stored so `accept_admin` can verify it
    ///
    /// # Errors
    /// - Panics if the current admin auth fails
    /// - E009 if `nonce` does not match the stored admin nonce
    pub fn propose_admin(env: Env, new_admin: Address, nonce: u64) {
        let current = get_admin(&env);
        current.require_auth();
        consume_admin_nonce(&env, nonce);
        set_pending_admin(&env, &new_admin);
        set_pending_admin_nonce(&env, nonce);
    }

    /// Step 2 of two-step admin transfer: proposed admin accepts and becomes admin.
    ///
    /// # Parameters
    /// - `new_admin` — must match the address set by [`propose_admin`] (requires auth)
    /// - `nonce` — must match the nonce stored by [`propose_admin`]
    ///
    /// # Errors
    /// - E010 if there is no pending admin
    /// - E011 if `new_admin` does not match the pending admin
    /// - E024 if `nonce` does not match the nonce stored by propose_admin
    pub fn accept_admin(env: Env, new_admin: Address, nonce: u64) {
        new_admin.require_auth();
        let pending = get_pending_admin(&env).expect(ERR_NO_PENDING_ADMIN);
        assert_eq!(pending, new_admin, "{}", ERR_NOT_PENDING_ADMIN);
        let stored_nonce = get_pending_admin_nonce(&env).expect(ERR_NO_PENDING_ADMIN);
        assert!(nonce == stored_nonce, "{}", ERR_BAD_PENDING_NONCE);
        set_admin(&env, &new_admin);
        clear_pending_admin(&env);
    }

    /// Admin pauses the entire contract — blocks new streams and withdrawals.
    ///
    /// While paused, `create_stream`, `create_streams_batch`, and `withdraw`
    /// will all panic. Admin operations (top-up, cancel, etc.) remain available.
    ///
    /// # Parameters
    /// - `nonce` — current admin nonce; must match the stored value (replay protection)
    ///
    /// # Errors
    /// - Panics if admin auth fails
    /// - E009 if `nonce` does not match the stored nonce
    pub fn pause_contract(env: Env, nonce: u64) {
        let admin = get_admin(&env);
        admin.require_auth();
        consume_admin_nonce(&env, nonce);
        set_paused(&env, true);
        events::contract_paused(&env, true);
    }

    /// Admin unpauses the contract, restoring normal operation.
    ///
    /// # Parameters
    /// - `nonce` — current admin nonce; must match the stored value (replay protection)
    ///
    /// # Errors
    /// - Panics if admin auth fails
    /// - E009 if `nonce` does not match the stored nonce
    pub fn unpause_contract(env: Env, nonce: u64) {
        let admin = get_admin(&env);
        admin.require_auth();
        consume_admin_nonce(&env, nonce);
        set_paused(&env, false);
        events::contract_paused(&env, false);
    }

    /// Set the minimum deposit enforced on `create_stream`.
    ///
    /// Streams created after this call must have `deposit >= amount`.
    /// Existing streams are unaffected.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    /// - `amount` — new minimum deposit (must be > 0)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - E009 if `nonce` is wrong
    /// - E002 if `amount` ≤ 0
    pub fn set_min_deposit(env: Env, admin: Address, nonce: u64, amount: i128) {
        admin.require_auth();
        let stored_admin = get_admin(&env);
        assert_eq!(admin, stored_admin, "{}", ERR_NOT_ADMIN);
        consume_admin_nonce(&env, nonce);
        assert!(amount > 0, "{}", ERR_ZERO_DEPOSIT);
        set_min_deposit(&env, amount);
    }

    /// Set the maximum deposit enforced on `create_stream` (SC-66).
    ///
    /// Streams created after this call must have `deposit <= amount`. A value
    /// of `0` disables the cap (unlimited), which is the default for backward
    /// compatibility. Existing streams are unaffected.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    /// - `amount` — new maximum deposit (must be ≥ 0; 0 = unlimited)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - E009 if `nonce` is wrong
    /// - Panics if `amount` < 0
    pub fn set_max_deposit(env: Env, admin: Address, nonce: u64, amount: i128) {
        admin.require_auth();
        let stored_admin = get_admin(&env);
        assert_eq!(admin, stored_admin, "{}", ERR_NOT_ADMIN);
        consume_admin_nonce(&env, nonce);
        assert!(amount >= 0, "max deposit must be non-negative");
        set_max_deposit(&env, amount);
        events::max_deposit_updated(&env, amount);
    }

    /// Set the maximum number of streams an employer may create per ledger.
    ///
    /// The default is 50. The new value takes effect immediately for the next
    /// `create_stream` or `create_streams_batch` call.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    /// - `max` — new per-ledger limit (must be ≥ 1)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - E009 if `nonce` is wrong
    /// - Panics if `max` is 0
    pub fn set_max_creates_per_ledger(env: Env, admin: Address, nonce: u64, max: u32) {
        admin.require_auth();
        let stored_admin = get_admin(&env);
        assert_eq!(admin, stored_admin, "{}", ERR_NOT_ADMIN);
        consume_admin_nonce(&env, nonce);
        assert!(max >= 1, "max must be at least 1");
        store_max_creates(&env, max);
        events::max_creates_per_ledger_updated(&env, max);
    }

    /// Set the protocol creation fee and its treasury destination.
    ///
    /// The fee is deducted from gross stream deposits. A value of 0 disables
    /// fees; non-zero values are capped at 100 basis points (1%).
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    /// - `fee_bps` — fee in basis points, from 0 through 100
    /// - `treasury` — address receiving collected fees
    ///
    /// # Errors
    /// - Panics if admin auth fails or does not match the stored admin
    /// - E009 if `nonce` is wrong
    /// - Panics if `fee_bps` exceeds 100
    pub fn set_protocol_fee(env: Env, admin: Address, nonce: u64, fee_bps: u32, treasury: Address) {
        admin.require_auth();
        let stored_admin = get_admin(&env);
        assert_eq!(admin, stored_admin, "{}", ERR_NOT_ADMIN);
        assert!(fee_bps <= 100, "protocol fee cannot exceed 100 basis points");
        consume_admin_nonce(&env, nonce);
        set_protocol_fee(&env, fee_bps, &treasury);
    }

    /// Employer creates a salary stream and deposits funds into the contract escrow.
    ///
    /// Tokens are transferred from `employer` to the contract immediately.
    /// The employee can call [`withdraw`] at any time to claim earned tokens.
    ///
    /// # Parameters
    /// - `employer` — employer address; funds are pulled from here (requires auth)
    /// - `employee` — employee address; receives streamed tokens
    /// - `token_address` — SEP-41 token contract address
    /// - `deposit` — gross tokens to fund the stream (must be ≥ min deposit and,
    ///   when a max deposit is configured, ≤ max deposit); any configured
    ///   protocol fee is deducted before the net amount is locked
    /// - `rate_per_second` — tokens streamed per second (1 – 1,000,000,000)
    /// - `stop_time` — hard stop timestamp in seconds; 0 means indefinite
    /// - `low_balance_threshold` — optional low-balance alert level
    /// - `cliff_time` — optional cliff timestamp (PROD-17). Nothing accrues
    ///   before it; once passed, accrual runs from `cliff_time` (not from the
    ///   start time). A cliff at or before `now` has no effect.
    ///
    /// # Returns
    /// The new stream ID as `u64`.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - E002 if `deposit` ≤ 0
    /// - E007 if `deposit` < minimum deposit
    /// - E036 if a max deposit is configured and `deposit` > max deposit
    /// - E001 if `rate_per_second` ≤ 0
    /// - E008 if `rate_per_second` > 1,000,000,000
    /// - Panics if `stop_time` is non-zero and in the past
    /// - Panics if `employer` == `employee`
    /// - E035 if `cliff_time` > `stop_time` (when `stop_time` is set)
    /// - Panics if the token transfer fails
    pub fn create_stream(
        env: Env,
        employer: Address,
        employee: Address,
        token_address: Address,
        deposit: i128,
        rate_per_second: i128,
        stop_time: u64,
        low_balance_threshold: Option<i128>,
        cliff_time: Option<u64>,
    ) -> u64 {
        employer.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);

        // SEC-24: enforce per-employer, per-ledger stream creation rate limit.
        check_and_increment_create_rate(&env, &employer, 1);

        let now = env.ledger().timestamp();
        let min_deposit = get_min_deposit(&env);
        let max_deposit = get_max_deposit(&env);
        validate_create_stream(
            deposit,
            min_deposit,
            max_deposit,
            rate_per_second,
            stop_time,
            now,
            &employer,
            &employee,
        );
        validate_cliff(cliff_time, stop_time);
        // A cliff that has already passed is a no-op.
        let cliff = cliff_time.filter(|c| *c > now);

        let (fee_bps, treasury) = get_protocol_fee(&env);
        let fee = protocol_fee_amount(deposit, fee_bps);
        let stream_deposit = deposit - fee;
        let token_client = token::Client::new(&env, &token_address);
        token_client.balance(&employer); // SEP-41 probe
        token_client.transfer(&employer, &env.current_contract_address(), &stream_deposit);
        if fee > 0 {
            token_client.transfer(
                &employer,
                &treasury.expect("protocol fee treasury is not configured"),
                &fee,
            );
        }

        let id = next_id(&env);
        let stream = Stream {
            id,
            employer: employer.clone(),
            employee: employee.clone(),
            token: token_address,
            deposit: stream_deposit,
            withdrawn: 0,
            rate_per_second,
            start_time: now,
            stop_time,
            // PROD-17: accrual is measured from last_withdraw_time, so anchoring
            // it at the cliff makes claimable_amount return 0 until the cliff
            // and accrue from cliff_time afterwards.
            last_withdraw_time: cliff.unwrap_or(now),
            status: StreamStatus::Active,
            pending_accrual: 0,
            locked: false,
            low_balance_threshold: low_balance_threshold.unwrap_or(0),
            pause_reason: None,
        };
        save_stream(&env, &stream);
        if let Some(c) = cliff {
            set_stream_cliff(&env, id, c);
        }
        index_employer_stream(&env, &employer, id);
        index_employee_stream(&env, &employee, id);
        index_token_stream(&env, &stream.token, id);
        index_stream_by_timestamp(&env, now, id);
        events::stream_created(&env, id, &employer, &employee, rate_per_second);
        id
    }

    /// Employer creates a grant that vests linearly over a fixed period.
    ///
    /// The optional cliff delays access to all accrued vesting until the cliff
    /// duration has elapsed; vesting itself accrues from `start_time`.
    pub fn create_linear_vest(
        env: Env,
        employer: Address,
        employee: Address,
        token_address: Address,
        total_grant: i128,
        vest_period: u64,
        cliff: Option<u64>,
    ) -> u64 {
        employer.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);

        let now = env.ledger().timestamp();
        assert!(total_grant > 0, "{}", ERR_ZERO_DEPOSIT);
        assert!(
            total_grant >= get_min_deposit(&env),
            "{}",
            ERR_BELOW_MIN_DEPOSIT
        );
        let max_deposit = get_max_deposit(&env);
        assert!(
            max_deposit == 0 || total_grant <= max_deposit,
            "{}",
            ERR_ABOVE_MAX_DEPOSIT
        );
        assert!(vest_period > 0, "vest period must be greater than zero");
        assert!(
            cliff.is_none_or(|cliff_duration| cliff_duration <= vest_period),
            "cliff cannot exceed vest period"
        );
        assert!(employer != employee, "employer and employee must differ");
        let stop_time = now.checked_add(vest_period).expect(ERR_OVERFLOW);

        let (fee_bps, treasury) = get_protocol_fee(&env);
        let fee = protocol_fee_amount(total_grant, fee_bps);
        let net_grant = total_grant - fee;
        let token_client = token::Client::new(&env, &token_address);
        token_client.balance(&employer); // SEP-41 probe
        token_client.transfer(&employer, &env.current_contract_address(), &net_grant);
        if fee > 0 {
            token_client.transfer(
                &employer,
                &treasury.expect("protocol fee treasury is not configured"),
                &fee,
            );
        }

        let id = next_id(&env);
        let stream = Stream {
            id,
            employer: employer.clone(),
            employee: employee.clone(),
            token: token_address.clone(),
            deposit: net_grant,
            withdrawn: 0,
            rate_per_second: 0,
            start_time: now,
            stop_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            pending_accrual: 0,
            locked: false,
            low_balance_threshold: 0,
            pause_reason: None,
        };
        save_stream(&env, &stream);
        set_linear_vest_schedule(
            &env,
            id,
            &LinearVestSchedule {
                total_grant: net_grant,
                vest_period,
                cliff,
            },
        );
        index_employer_stream(&env, &employer, id);
        index_employee_stream(&env, &employee, id);
        index_token_stream(&env, &token_address, id);
        index_stream_by_timestamp(&env, now, id);
        events::stream_created(&env, id, &employer, &employee, 0);
        id
    }

    /// Employer creates multiple salary streams atomically in a single transaction.
    ///
    /// All streams succeed or all revert. Cheaper than N individual
    /// `create_stream` calls for N ≥ 2 because Stellar charges one base fee
    /// per transaction.
    ///
    /// # Parameters
    /// - `employer` — employer address (requires auth)
    /// - `params` — list of [`StreamParams`]; must contain 1–50 entries
    ///
    /// # Returns
    /// `Vec<u64>` of new stream IDs in the same order as `params`.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - Panics if `params` is empty
    /// - Panics with E029 if `params` contains more than 50 entries
    /// - Same per-stream validations as [`create_stream`]; panic message includes
    ///   the zero-based stream index, e.g. `"stream[3]: E001: rate_per_second must
    ///   be greater than zero"`, so callers can identify the failing entry without
    ///   retrying each stream individually.
    pub fn create_streams_batch(
        env: Env,
        employer: Address,
        params: Vec<StreamParams>,
    ) -> Vec<u64> {
        employer.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);
        assert!(!params.is_empty(), "{}", ERR_EMPTY_PARAMS);
        assert!(params.len() <= MAX_BATCH_SIZE, "{}", ERR_BATCH_TOO_LARGE);

        // SEC-24: count the entire batch against the per-ledger rate limit so a
        // single batch call cannot bypass the cap.
        check_and_increment_create_rate(&env, &employer, params.len());

        let now = env.ledger().timestamp();
        let min_deposit = get_min_deposit(&env);
        let max_deposit = get_max_deposit(&env);
        let mut ids: Vec<u64> = Vec::new(&env);
        let mut index: u32 = 0;

        // Validate each stream and sum net deposits per token so each token is
        // pulled from the employer with one escrow transfer.
        let (fee_bps, treasury) = get_protocol_fee(&env);
        let mut totals: Map<Address, i128> = Map::new(&env);
        let mut fee_totals: Map<Address, i128> = Map::new(&env);
        for p in params.iter() {
            // Validate each parameter independently and prefix any panic with the
            // stream index so callers can identify which entry failed.
            assert!(p.deposit > 0, "stream[{}]: {}", index, ERR_ZERO_DEPOSIT);
            assert!(
                p.deposit >= min_deposit,
                "stream[{}]: {}",
                index,
                ERR_BELOW_MIN_DEPOSIT
            );
            assert!(
                max_deposit == 0 || p.deposit <= max_deposit,
                "stream[{}]: {}",
                index,
                ERR_ABOVE_MAX_DEPOSIT
            );
            assert!(
                p.rate_per_second > 0,
                "stream[{}]: {}",
                index,
                ERR_ZERO_RATE
            );
            assert!(
                p.rate_per_second <= validate::MAX_RATE_PER_SECOND,
                "stream[{}]: {}",
                index,
                ERR_INVALID_RATE
            );
            if p.stop_time > 0 {
                assert!(
                    p.stop_time > now,
                    "stream[{}]: {}",
                    index,
                    ERR_STOP_TIME_PAST
                );
            }
            validate_cliff(p.cliff_time, p.stop_time);
            assert!(
                employer != p.employee,
                "stream[{}]: employer and employee must differ",
                index
            );
            let fee = protocol_fee_amount(p.deposit, fee_bps);
            let net_deposit = p.deposit - fee;
            let total = totals
                .get(p.token.clone())
                .unwrap_or(0)
                .checked_add(net_deposit)
                .expect(ERR_OVERFLOW);
            totals.set(p.token.clone(), total);
            if fee > 0 {
                let fee_total = fee_totals
                    .get(p.token.clone())
                    .unwrap_or(0)
                    .checked_add(fee)
                    .expect(ERR_OVERFLOW);
                fee_totals.set(p.token.clone(), fee_total);
            }
        }

        for (token_address, total) in totals.iter() {
            let token_client = token::Client::new(&env, &token_address);
            token_client.balance(&employer); // SEP-41 probe
            token_client.transfer(&employer, &env.current_contract_address(), &total);
        }
        if fee_bps > 0 {
            let treasury = treasury.expect("protocol fee treasury is not configured");
            for (token_address, total_fee) in fee_totals.iter() {
                let token_client = token::Client::new(&env, &token_address);
                token_client.transfer(&employer, &treasury, &total_fee);
            }
        }

        let mut ids: Vec<u64> = Vec::new(&env);
        for p in params.iter() {
            let id = next_id(&env);
            let stream = Stream {
                id,
                employer: employer.clone(),
                employee: p.employee.clone(),
                token: p.token.clone(),
                deposit: p.deposit - protocol_fee_amount(p.deposit, fee_bps),
                withdrawn: 0,
                rate_per_second: p.rate_per_second,
                start_time: now,
                stop_time: p.stop_time,
                last_withdraw_time: p.cliff_time.filter(|c| *c > now).unwrap_or(now),
                status: StreamStatus::Active,
                pending_accrual: 0,
                locked: false,
                low_balance_threshold: 0,
                pause_reason: None,
            };
            save_stream(&env, &stream);
            if let Some(c) = p.cliff_time.filter(|c| *c > now) {
                set_stream_cliff(&env, id, c);
            }
            index_employer_stream(&env, &employer, id);
            index_employee_stream(&env, &p.employee, id);
            index_token_stream(&env, &p.token, id);
            index_stream_by_timestamp(&env, now, id);
            events::stream_created(&env, id, &employer, &p.employee, p.rate_per_second);
            ids.push_back(id);
            index += 1;
        }

        ids
    }

    /// Create a multi-beneficiary stream that splits earnings across multiple recipients (PROD-22).
    ///
    /// The deposit is locked on-chain and accrues at the specified rate per second.
    /// When withdrawn, earnings are split among beneficiaries according to their
    /// basis points allocation (e.g., 5000 basis points = 50% of earnings).
    ///
    /// # Parameters
    /// - `employer` — stream owner who deposits funds (requires auth)
    /// - `token` — token contract address (SEP-41 or SAC)
    /// - `deposit` — total tokens to lock (must be > 0 and >= minimum)
    /// - `rate_per_second` — tokens accrued per second (must be > 0)
    /// - `stop_time` — optional hard stop timestamp (0 = no limit; if set, must be in future)
    /// - `beneficiaries` — recipients and their earnings split; must have 2-20 entries with basis points summing to 10000
    /// - `low_balance_threshold` — optional alert threshold (0 = disabled)
    ///
    /// # Returns
    /// Stream ID as `u64`.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - E007 if deposit < minimum
    /// - E036 if a max deposit is configured and deposit > max deposit
    /// - E001 if rate_per_second ≤ 0
    /// - E008 if rate_per_second exceeds maximum
    /// - E022 if stop_time is set and in the past
    /// - E031 if fewer than 2 beneficiaries
    /// - E032 if more than 20 beneficiaries
    /// - E030 if basis points do not sum to exactly 10000
    pub fn create_split_stream(
        env: Env,
        employer: Address,
        token: Address,
        deposit: i128,
        rate_per_second: i128,
        stop_time: u64,
        beneficiaries: Vec<SplitBeneficiary>,
        low_balance_threshold: Option<i128>,
    ) -> u64 {
        employer.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);

        let now = env.ledger().timestamp();
        let min_deposit = get_min_deposit(&env);
        let max_deposit = get_max_deposit(&env);

        // Validate the base stream parameters
        assert!(deposit > 0, "{}", ERR_ZERO_DEPOSIT);
        assert!(deposit >= min_deposit, "{}", ERR_BELOW_MIN_DEPOSIT);
        assert!(
            max_deposit == 0 || deposit <= max_deposit,
            "{}",
            ERR_ABOVE_MAX_DEPOSIT
        );
        assert!(rate_per_second > 0, "{}", ERR_ZERO_RATE);
        assert!(
            rate_per_second <= validate::MAX_RATE_PER_SECOND,
            "{}",
            ERR_INVALID_RATE
        );
        if stop_time > 0 {
            assert!(stop_time > now, "{}", ERR_STOP_TIME_PAST);
        }

        // Validate beneficiary configuration
        validate_split_beneficiaries(&beneficiaries);

        // Transfer deposit from employer to contract
        let this = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&employer, &this, &deposit);

        // Create the stream with the first beneficiary as the nominal employee
        // (used for event emission and historical compatibility)
        let first_beneficiary = beneficiaries.get(0).expect("beneficiary missing");
        let stream_id = next_id(&env);
        let stream = Stream {
            id: stream_id,