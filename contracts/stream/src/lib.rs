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
    get_employee_streams, get_employer_streams, get_min_deposit, get_pending_admin,
    get_pending_admin_nonce, get_pending_upgrade, get_stream_cliff, get_streams_in_range,
    get_streams_in_time_range, get_template_ids, get_token_streams, index_employee_stream,
    index_employer_stream, index_stream_by_timestamp, index_token_stream, load_stream,
    load_template, mark_cliff_reached, next_id, receipt_owner, remove_auto_topup,
    remove_employer_stream, save_stream, save_template, set_admin, set_auto_topup, set_min_deposit,
    set_pending_admin, set_pending_admin_nonce, set_pending_upgrade, set_receipt_owner,
    set_stream_cliff, stream_ids_by_employee, stream_ids_by_employer, TIMELOCK_DELAY,
};
use types::{
    AutoTopupConfig, PendingUpgrade, SplitBeneficiary,
    ERR_ARCHIVE_TOO_EARLY, ERR_BELOW_MIN_DEPOSIT, ERR_INVALID_AUTO_TOPUP, ERR_INVALID_RATE, ERR_NO_AUTO_TOPUP, ERR_NO_PENDING_UPGRADE, ERR_STOP_TIME_PAST, ERR_STREAM_NOT_ENDED, ERR_UPGRADE_TIMELOCK_ACTIVE, ERR_ZERO_RATE,
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
    /// - `deposit` — gross tokens to fund the stream (must be ≥ min deposit); any
    ///   configured protocol fee is deducted before the net amount is locked
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
        validate_create_stream(
            deposit,
            min_deposit,
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

        // Validate the base stream parameters
        assert!(deposit > 0, "{}", ERR_ZERO_DEPOSIT);
        assert!(deposit >= min_deposit, "{}", ERR_BELOW_MIN_DEPOSIT);
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
            employer: employer.clone(),
            employee: first_beneficiary.address.clone(),
            token: token.clone(),
            deposit,
            withdrawn: 0,
            rate_per_second,
            start_time: now,
            stop_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            pending_accrual: 0,
            locked: false,
            low_balance_threshold: low_balance_threshold.unwrap_or(0),
            pause_reason: None,
        };

        save_stream(&env, &stream);
        set_split_beneficiaries(&env, stream_id, &beneficiaries);
        index_employer_stream(&env, &employer, stream_id);
        index_token_stream(&env, &token, stream_id);
        index_stream_by_timestamp(&env, now, stream_id);

        // Index all beneficiaries so they can query their streams
        for benef in beneficiaries.iter() {
            index_employee_stream(&env, &benef.address, stream_id);
        }

        events::split_stream_created(&env, stream_id, &employer, rate_per_second);
        stream_id
    }

    /// Employee withdraws all claimable tokens earned so far.
    ///
    /// Claimable amount is `min((now - last_withdraw_time) * rate_per_second, remaining_deposit)`.
    /// Returns 0 without reverting if nothing is claimable yet.
    /// Marks the stream Exhausted when the full deposit has been withdrawn.
    /// Also transitions the stream to Exhausted when `stop_time` has passed and
    /// no tokens remain claimable (SC-02).
    ///
    /// # Parameters
    /// - `employee` — must hold the stream receipt (requires auth)
    /// - `stream_id` — ID of the stream to withdraw from
    ///
    /// # Returns
    /// Amount transferred as `i128`; 0 if nothing was claimable.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - Panics if stream not found
    /// - E016 if caller does not hold the stream receipt
    /// - Panics if stream is not Active or Exhausted
    /// - E003 if a reentrant withdraw is detected
    pub fn withdraw(env: Env, employee: Address, stream_id: u64) -> i128 {
        employee.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        // PROD-01: the current receipt holder, not the original employee, may withdraw.
        assert_eq!(
            receipt_owner(&env, &stream),
            employee,
            "{}",
            ERR_NOT_EMPLOYEE
        );
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Exhausted,
            "{}",
            ERR_STREAM_NOT_ACTIVE
        );

        let now = env.ledger().timestamp();
        if let Some(c) = mark_cliff_reached(&env, stream.id, now) {
            events::cliff_reached(&env, stream.id, c);
        }
        let amount = stream_claimable_amount(&env, &stream, now);

        // SC-02: if stop_time has passed and nothing is left to claim, settle
        // the stream to Exhausted so off-chain indexers get an accurate status.
        if amount == 0 {
            if stream.status == StreamStatus::Active
                && stream.stop_time > 0
                && now >= stream.stop_time
            {
                stream.status = StreamStatus::Exhausted;
                save_stream(&env, &stream);
                events::stream_status_changed(&env, stream_id, &StreamStatus::Exhausted);
            }
            return 0;
        }

        assert!(!stream.locked, "{}", ERR_REENTRANT);
        stream.locked = true;
        save_stream(&env, &stream);

        stream.withdrawn = stream
            .withdrawn
            .checked_add(amount)
            .expect(ERR_OVERFLOW);
        stream.last_withdraw_time = accrual_start(&env, stream.id, now);
        // Clear any banked pre-rate-change accrual now that it has been paid out.
        stream.pending_accrual = 0;
        if stream.withdrawn >= stream.deposit {
            set_stream_ended_at(&env, stream_id, now);
            stream.status = StreamStatus::Exhausted;
        }

        let token_client = token::Client::new(&env, &stream.token);
        token_client.transfer(&env.current_contract_address(), &employee, &amount);
        maybe_auto_topup(&env, &mut stream, now);

        stream.locked = false;
        save_stream(&env, &stream);
        events::withdrawn(&env, stream_id, &employee, amount);
        let remaining = stream.deposit - stream.withdrawn;
        if stream.low_balance_threshold > 0 && remaining < stream.low_balance_threshold {
            events::low_balance(
                &env,
                stream_id,
                &stream.employer,
                remaining,
                stream.low_balance_threshold,
            );
        }
        amount
    }

    /// Employee withdraws all claimable tokens from every stream they receive.
    ///
    /// Iterates the employee's stream index and calls the withdraw logic on
    /// each stream in a single transaction. Auth is checked once up front.
    ///
    /// Streams that are Cancelled or Paused are skipped without reverting.
    /// Streams where the claimable amount is zero are also skipped.
    ///
    /// # Parameters
    /// - `employee` — employee address; must authenticate (requires auth, called once)
    ///
    /// # Returns
    /// `Vec<i128>` — amounts transferred, in employee stream-index order,
    /// with one entry for each stream from which tokens were transferred.
    /// Empty if nothing was claimable.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - E003 if a reentrant withdraw is detected on any stream
    pub fn withdraw_all(env: Env, employee: Address) -> Vec<i128> {
        employee.require_auth();
        assert!(!get_paused(&env), "contract is paused");

        let stream_ids = get_employee_streams(&env, &employee);
        let mut results: Vec<i128> = Vec::new(&env);
        
        // Return empty results gracefully if employee has no streams
        if stream_ids.is_empty() {
            return results;
        }

        let now = env.ledger().timestamp();

        for stream_id in stream_ids.iter() {
            let mut stream = match load_stream(&env, stream_id) {
                Some(s) => s,
                None => continue,
            };

            // PROD-01: skip streams whose receipt has been transferred away.
            if receipt_owner(&env, &stream) != employee {
                continue;
            }
            if let Some(c) = mark_cliff_reached(&env, stream.id, now) {
                events::cliff_reached(&env, stream.id, c);
            }

            // Skip streams that cannot be withdrawn from
            if stream.status == StreamStatus::Cancelled || stream.status == StreamStatus::Paused {
                continue;
            }

            // Only Active and Exhausted streams are eligible
            if stream.status != StreamStatus::Active && stream.status != StreamStatus::Exhausted {
                continue;
            }

            let amount = stream_claimable_amount(&env, &stream, now);
            if amount == 0 {
                continue;
            }

            assert!(!stream.locked, "{}", ERR_REENTRANT);
            stream.locked = true;
            save_stream(&env, &stream);

            stream.withdrawn = stream
                .withdrawn
                .checked_add(amount)
                .expect(ERR_OVERFLOW);
            stream.last_withdraw_time = accrual_start(&env, stream.id, now);
            // Clear any banked pre-rate-change accrual now that it has been paid out.
            stream.pending_accrual = 0;
            if stream.withdrawn >= stream.deposit {
                set_stream_ended_at(&env, stream_id, now);
                stream.status = StreamStatus::Exhausted;
            }

            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &employee, &amount);
            maybe_auto_topup(&env, &mut stream, now);

            stream.locked = false;
            save_stream(&env, &stream);
            events::withdrawn(&env, stream_id, &employee, amount);

            results.push_back(amount);
        }

        results
    }

    /// Employer tops up an active stream with additional funds.
    ///
    /// Increases `deposit` by `amount`. The stream's rate and timeline are
    /// unchanged; the extra funds simply extend how long the stream can run.
    ///
    /// The employer must approve the stream contract as a spender for at least
    /// `amount` tokens before calling this function.  An explicit allowance
    /// pre-check (SEC-23) is performed before the `transfer_from` call so that
    /// insufficient allowance produces a clear error message and no state is
    /// mutated.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to top up
    /// - `amount` — additional tokens to deposit (must be > 0)
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - E005 if stream is Cancelled
    /// - E006 if stream is Exhausted
    /// - Panics if `amount` ≤ 0
    /// - Panics with "insufficient allowance" if the employer has not approved enough
    /// - Panics if the token transfer fails
    pub fn top_up(
        env: Env,
        employer: Address,
        stream_id: u64,
        amount: i128,
        low_balance_threshold: Option<i128>,
    ) {
        employer.require_auth();
        validate_top_up(amount);
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert_payroll_stream(&env, stream_id);
        assert!(
            stream.status != StreamStatus::Cancelled,
            "{}",
            ERR_STREAM_CANCELLED
        );
        assert!(
            stream.status != StreamStatus::Exhausted,
            "{}",
            ERR_STREAM_EXHAUSTED
        );

        let this = env.current_contract_address();
        let token_client = token::Client::new(&env, &stream.token);
        // SEC-23: pre-check allowance so the contract can emit a clear error
        // before any state is mutated, rather than letting the token contract
        // fail deep inside a cross-contract call.
        let allowed = token_client.allowance(&employer, &this);
        assert!(
            allowed >= amount,
            "insufficient allowance: employer must approve the stream contract for at least the top-up amount"
        );
        token_client.transfer_from(&this, &employer, &this, &amount);

        stream.deposit = stream.deposit.checked_add(amount).expect(ERR_OVERFLOW);
        if let Some(threshold) = low_balance_threshold {
            stream.low_balance_threshold = threshold;
        }
        save_stream(&env, &stream);
        events::topped_up(&env, stream_id, &employer, amount);
    }

    /// Update the low-balance alert threshold for a stream.
    ///
    /// The employer can adjust or disable the alert threshold at any time
    /// without topping up. A threshold of 0 disables the alert.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to update
    /// - `threshold` — new alert threshold in deposit tokens (≥ 0; 0 disables alert)
    ///
    /// # Errors
    /// - Panics if caller is not the stream's employer (E018)
    /// - Panics if stream not found (E015)
    /// - Panics if stream is Cancelled (E005) or Exhausted (E006)
    /// - Panics if threshold is negative
    pub fn set_low_balance_threshold(env: Env, employer: Address, stream_id: u64, threshold: i128) {
        employer.require_auth();
        assert!(threshold >= 0, "threshold must be non-negative");

        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status != StreamStatus::Cancelled,
            "{}",
            ERR_STREAM_CANCELLED
        );
        assert!(
            stream.status != StreamStatus::Exhausted,
            "{}",
            ERR_STREAM_EXHAUSTED
        );

        let old_threshold = stream.low_balance_threshold;
        stream.low_balance_threshold = threshold;
        save_stream(&env, &stream);
        events::threshold_updated(&env, stream_id, old_threshold, threshold);
    }

    /// Employer pre-authorizes recurring top-ups for a stream.
    ///
    /// After each withdraw, if `deposit - withdrawn < trigger_threshold`, the
    /// contract pulls `topup_amount` from the employer via `transfer_from`,
    /// until `max_total` has been transferred in total. The employer must
    /// approve the stream contract as spender for at least that allowance.
    /// Calling again replaces the configuration and resets the running total.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream
    /// - `trigger_threshold` — remaining balance below which to top up (> 0)
    /// - `topup_amount` — amount per top-up (> 0, ≤ `max_total`)
    /// - `max_total` — cap on cumulative auto top-ups (> 0)
    ///
    /// # Errors
    /// - Panics if stream not found or caller is not the employer
    /// - E005 if stream is Cancelled
    /// - E025 if parameters are invalid
    pub fn set_auto_topup(
        env: Env,
        employer: Address,
        stream_id: u64,
        trigger_threshold: i128,
        topup_amount: i128,
        max_total: i128,
    ) {
        employer.require_auth();
        assert!(
            trigger_threshold > 0 && topup_amount > 0 && topup_amount <= max_total,
            "{}",
            ERR_INVALID_AUTO_TOPUP
        );
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert_payroll_stream(&env, stream_id);
        assert!(
            stream.status != StreamStatus::Cancelled,
            "{}",
            ERR_STREAM_CANCELLED
        );
        set_auto_topup(
            &env,
            stream_id,
            &AutoTopupConfig {
                trigger_threshold,
                topup_amount,
                max_total,
                total_topped_up: 0,
            },
        );
        events::auto_topup_set(&env, stream_id, true);
    }

    /// Employer revokes a stream's recurring top-up authorization.
    ///
    /// # Errors
    /// - Panics if stream not found or caller is not the employer
    /// - E026 if no auto top-up is configured
    pub fn cancel_auto_topup(env: Env, employer: Address, stream_id: u64) {
        employer.require_auth();
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            get_auto_topup(&env, stream_id).is_some(),
            "{}",
            ERR_NO_AUTO_TOPUP
        );
        remove_auto_topup(&env, stream_id);
        events::auto_topup_set(&env, stream_id, false);
    }

    /// Returns the stream's recurring auto top-up configuration, if any.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to query
    ///
    /// # Returns
    /// `Some(AutoTopupConfig)` if configured, or `None` if no auto top-up is set.
    pub fn get_auto_topup_config(env: Env, stream_id: u64) -> Option<AutoTopupConfig> {
        get_auto_topup(&env, stream_id)
    }

    /// Alias for [`get_auto_topup_config`].
    pub fn get_auto_topup(env: Env, stream_id: u64) -> Option<AutoTopupConfig> {
        Self::get_auto_topup_config(env, stream_id)
    }

    /// Employer pauses an active stream, stopping token accrual.
    ///
    /// The employee cannot withdraw while the stream is paused. Call
    /// [`resume_stream`] to restart accrual; paused time is excluded from
    /// the claimable calculation.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to pause
    /// - `pause_reason` — optional reason for pausing (regulatory compliance and dispute resolution)
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - Panics if stream is not Active
    pub fn pause_stream(
        env: Env,
        employer: Address,
        stream_id: u64,
        pause_reason: Option<soroban_sdk::String>,
    ) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert_eq!(
            stream.status,
            StreamStatus::Active,
            "{}",
            ERR_STREAM_NOT_ACTIVE
        );
        if get_stream_type(&env, stream_id) == StreamType::LinearVest {
            stream.last_withdraw_time = accrual_start(&env, stream.id, env.ledger().timestamp());
        }
        stream.status = StreamStatus::Paused;
        stream.pause_reason = pause_reason;
        save_stream(&env, &stream);
        events::stream_status_changed(&env, stream_id, &StreamStatus::Paused);
    }

    /// Employer pauses multiple Active streams atomically.
    ///
    /// Every stream must exist, belong to `employer`, and be Active. If any
    /// stream fails validation, the transaction reverts all status changes.
    ///
    /// # Parameters
    /// - `employer` — must own every stream (requires auth once)
    /// - `stream_ids` — IDs of 1–50 streams to pause
    ///
    /// # Errors
    /// - Panics if `stream_ids` is empty or exceeds 50 entries
    /// - Panics if any stream is not found, does not belong to `employer`, or is not Active
    pub fn pause_stream_batch(env: Env, employer: Address, stream_ids: Vec<u64>) {
        employer.require_auth();
        assert!(!stream_ids.is_empty(), "stream_ids must not be empty");
        assert!(
            stream_ids.len() <= MAX_BATCH_SIZE,
            "{}",
            ERR_BATCH_TOO_LARGE
        );

        let now = env.ledger().timestamp();
        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
            assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
            assert_eq!(
                stream.status,
                StreamStatus::Active,
                "{}",
                ERR_STREAM_NOT_ACTIVE
            );

            if get_stream_type(&env, stream_id) == StreamType::LinearVest {
                stream.last_withdraw_time = accrual_start(&env, stream.id, now);
            }
            stream.status = StreamStatus::Paused;
            save_stream(&env, &stream);
            events::stream_status_changed(&env, stream_id, &StreamStatus::Paused);
        }
    }

    /// Employer resumes a paused stream, restarting token accrual.
    ///
    /// `last_withdraw_time` is reset to the current ledger timestamp so that
    /// the paused interval is excluded from future claimable calculations.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to resume
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - Panics if stream is not Paused
    pub fn resume_stream(env: Env, employer: Address, stream_id: u64) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert_eq!(
            stream.status,
            StreamStatus::Paused,
            "{}",
            ERR_STREAM_NOT_PAUSED
        );
        let now = env.ledger().timestamp();
        if get_stream_type(&env, stream_id) == StreamType::LinearVest {
            let paused_for = now.saturating_sub(stream.last_withdraw_time);
            stream.start_time = stream
                .start_time
                .checked_add(paused_for)
                .expect(ERR_OVERFLOW);
            stream.stop_time = stream
                .stop_time
                .checked_add(paused_for)
                .expect(ERR_OVERFLOW);
        }
        stream.last_withdraw_time = accrual_start(&env, stream.id, now);
        stream.status = StreamStatus::Active;
        stream.pause_reason = None;
        save_stream(&env, &stream);
        events::stream_status_changed(&env, stream_id, &StreamStatus::Active);
    }

    /// Employer resumes multiple Paused streams atomically.
    ///
    /// Every stream must exist, belong to `employer`, and be Paused. If any
    /// stream fails validation, the transaction reverts all status changes.
    /// Each stream's accrual clock restarts at the current ledger timestamp.
    ///
    /// # Parameters
    /// - `employer` — must own every stream (requires auth once)
    /// - `stream_ids` — IDs of 1–50 streams to resume
    ///
    /// # Errors
    /// - Panics if `stream_ids` is empty or exceeds 50 entries
    /// - Panics if any stream is not found, does not belong to `employer`, or is not Paused
    pub fn resume_stream_batch(env: Env, employer: Address, stream_ids: Vec<u64>) {
        employer.require_auth();
        assert!(!stream_ids.is_empty(), "stream_ids must not be empty");
        assert!(
            stream_ids.len() <= MAX_BATCH_SIZE,
            "{}",
            ERR_BATCH_TOO_LARGE
        );

        let now = env.ledger().timestamp();
        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
            assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
            assert_eq!(
                stream.status,
                StreamStatus::Paused,
                "{}",
                ERR_STREAM_NOT_PAUSED
            );

            if get_stream_type(&env, stream_id) == StreamType::LinearVest {
                let paused_for = now.saturating_sub(stream.last_withdraw_time);
                stream.start_time = stream
                    .start_time
                    .checked_add(paused_for)
                    .expect(ERR_OVERFLOW);
                stream.stop_time = stream
                    .stop_time
                    .checked_add(paused_for)
                    .expect(ERR_OVERFLOW);
            }
            stream.last_withdraw_time = accrual_start(&env, stream.id, now);
            stream.status = StreamStatus::Active;
            save_stream(&env, &stream);
            events::stream_status_changed(&env, stream_id, &StreamStatus::Active);
        }
    }

    /// Employer updates the `rate_per_second` on an Active or Paused stream.
    ///
    /// Before changing the rate, any tokens accrued since the last withdrawal
    /// are settled by computing the elapsed claimable amount and adding it to
    /// `pending_accrual`. This ensures the employee retains full access to
    /// earnings at the old rate and will accrue at the new rate going forward.
    /// `last_withdraw_time` is then reset to the current timestamp.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to update
    /// - `new_rate` — new `rate_per_second` value (1 – 1,000,000,000)
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - Panics if stream is not Active or Paused
    /// - E001 if `new_rate` ≤ 0
    /// - E008 if `new_rate` > 1,000,000,000
    pub fn update_rate(env: Env, employer: Address, stream_id: u64, new_rate: i128) {
        employer.require_auth();
        validate_rate(new_rate);

        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
            "stream not active or paused"
        );
        assert_payroll_stream(&env, stream_id);

        let now = env.ledger().timestamp();

        // Bank everything claimable at the old rate into pending_accrual so the
        // employee can still claim it after the rate changes. `claimable_amount`
        // already includes the previously banked `pending_accrual`, so it
        // replaces (not adds to) the old value — adding it double-counted
        // earnings on every rate change.
        stream.pending_accrual = stream_claimable_amount(&env, &stream, now);

        // Reset last_withdraw_time so future claimable is measured from now
        // at the new rate only.
        stream.last_withdraw_time = accrual_start(&env, stream.id, now);

        let old_rate = stream.rate_per_second;
        stream.rate_per_second = new_rate;
        save_stream(&env, &stream);
        events::rate_updated(&env, stream_id, old_rate, new_rate);
    }

    /// Employer updates the hard stop time of an Active stream.
    ///
    /// Set `new_stop_time` to 0 to make the stream indefinite. A nonzero stop
    /// time cannot precede the stream's last withdrawal, which is the current
    /// accrual baseline.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to update
    /// - `new_stop_time` — Unix timestamp in seconds, or 0 for no hard stop
    ///
    /// # Errors
    /// - E015 if the stream does not exist
    /// - E017 if the stream is not Active
    /// - E018 if the caller is not the stream's employer
    /// - E030 if a nonzero `new_stop_time` precedes `last_withdraw_time`
    pub fn update_stop_time(env: Env, employer: Address, stream_id: u64, new_stop_time: u64) {
        employer.require_auth();

        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert_eq!(
            stream.status,
            StreamStatus::Active,
            "{}",
            ERR_STREAM_NOT_ACTIVE
        );
        assert!(
            new_stop_time == 0 || new_stop_time >= stream.last_withdraw_time,
            "{}",
            ERR_INVALID_STOP_TIME
        );

        let old_stop_time = stream.stop_time;
        stream.stop_time = new_stop_time;
        save_stream(&env, &stream);
        events::stop_time_updated(&env, stream_id, old_stop_time, new_stop_time);
    }

    /// Employer cancels a stream and reclaims unstreamed funds.    ///
    /// The employee receives all tokens earned up to the cancellation time.
    /// The employer is refunded the remaining deposit. Works on both Active
    /// and Paused streams.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to cancel
    /// - `cancel_reason` — optional reason symbol recorded on-chain for auditing
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - Panics if stream is already Cancelled or Exhausted
    pub fn cancel_stream(env: Env, employer: Address, stream_id: u64, cancel_reason: Option<Symbol>) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
            "{}",
            ERR_STREAM_ALREADY_ENDED
        );

        let now = env.ledger().timestamp();
        if let Some(c) = mark_cliff_reached(&env, stream.id, now) {
            events::cliff_reached(&env, stream.id, c);
        }
        let claimable = stream_claimable_amount(&env, &stream, now);
        let token_client = token::Client::new(&env, &stream.token);

        if claimable > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &receipt_owner(&env, &stream),
                &claimable,
            );
            stream.withdrawn = stream.withdrawn.checked_add(claimable).expect(ERR_OVERFLOW);
        }

        let refund = stream
            .deposit
            .checked_sub(stream.withdrawn)
            .unwrap_or(0)
            .max(0);
        if refund > 0 {
            token_client.transfer(&env.current_contract_address(), &employer, &refund);
        }

        set_stream_ended_at(&env, stream_id, now);
        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);
        events::stream_cancelled(&env, stream_id, claimable, refund);
    }

    /// Employer cancels multiple streams atomically in a single transaction.
    ///
    /// Mirrors `create_streams_batch`: all cancellations succeed or all revert.
    /// The employer pays each employee their earned share and is refunded the
    /// remainder for every stream in the batch. Cheaper than N individual
    /// `cancel_stream` calls for N ≥ 2 because Stellar charges one base fee
    /// per transaction.
    ///
    /// # Parameters
    /// - `employer` — employer address; must own every stream in the list (requires auth once)
    /// - `stream_ids` — IDs of 1–50 streams to cancel
    ///
    /// # Errors
    /// - Panics if `stream_ids` is empty
    /// - Panics with E029 if `stream_ids` contains more than 50 entries
    /// - Panics if any stream is not found
    /// - Panics if any stream does not belong to `employer`
    /// - Panics if any stream is already Cancelled or Exhausted
    pub fn cancel_streams_batch(env: Env, employer: Address, stream_ids: Vec<u64>) {
        employer.require_auth();
        assert!(!stream_ids.is_empty(), "stream_ids must not be empty");
        assert!(
            stream_ids.len() <= MAX_BATCH_SIZE,
            "{}",
            ERR_BATCH_TOO_LARGE
        );

        let now = env.ledger().timestamp();

        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id).expect("stream not found");
            assert_eq!(stream.employer, employer, "not the employer");
            assert!(
                stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
                "stream already ended"
            );

            let claimable = stream_claimable_amount(&env, &stream, now);
            let token_client = token::Client::new(&env, &stream.token);

            if claimable > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &receipt_owner(&env, &stream),
                    &claimable,
                );
                stream.withdrawn = stream.withdrawn.checked_add(claimable).expect(ERR_OVERFLOW);
            }

            let refund = stream
                .deposit
                .checked_sub(stream.withdrawn)
                .unwrap_or(0)
                .max(0);
            if refund > 0 {
                token_client.transfer(&env.current_contract_address(), &employer, &refund);
            }

            set_stream_ended_at(&env, stream_id, now);
            stream.status = StreamStatus::Cancelled;
            save_stream(&env, &stream);
            events::stream_status_changed(&env, stream_id, &StreamStatus::Cancelled);
        }
    }

    /// Archive a closed stream, moving its data from persistent to temporary
    /// (cheaper-rent) storage after a 30-day waiting period. Only Cancelled or
    /// Exhausted streams are eligible; archived streams are still readable via
    /// [`get_stream`].
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to archive
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - E030 if stream is not Cancelled or Exhausted
    /// - E031 if the 30-day waiting period has not elapsed
    pub fn archive_stream(env: Env, employer: Address, stream_id: u64) {
        employer.require_auth();
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status == StreamStatus::Cancelled || stream.status == StreamStatus::Exhausted,
            "{}",
            ERR_STREAM_NOT_ENDED
        );
        let now = env.ledger().timestamp();
        let ended_at = get_stream_ended_at(&env, stream_id).unwrap_or(0);
        assert!(
            ended_at > 0 && now >= ended_at.saturating_add(ARCHIVE_WAIT_PERIOD),
            "{}",
            ERR_ARCHIVE_TOO_EARLY
        );
        let key = DataKey::Stream(stream_id);
        env.storage().temporary().set(&key, &stream);
        env.storage().persistent().remove(&key);
        events::stream_archived(&env, stream_id);
    }

    /// Employer transfers ownership of a stream to a new employer address.
    ///
    /// Enables employer account rotation or business acquisition without
    /// cancelling and recreating existing streams. The stream's terms
    /// (rate, deposit, employee) are unchanged; only the `employer` field
    /// and the `EmployerStreams` index are updated.
    ///
    /// # Parameters
    /// - `employer` — current employer; must own the stream (requires auth)
    /// - `stream_id` — ID of the stream to transfer
    /// - `new_employer` — address that will become the new employer
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if `employer` auth fails
    /// - E018 if `employer` does not match the stream's employer
    /// - E020 if stream is Cancelled or Exhausted
    /// - E028 if `new_employer` equals the stream's employee
    pub fn transfer_stream(env: Env, employer: Address, stream_id: u64, new_employer: Address) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
            "{}",
            ERR_STREAM_ALREADY_ENDED
        );
        assert_ne!(
            new_employer, stream.employee,
            "{}",
            ERR_NEW_EMPLOYER_IS_EMPLOYEE
        );

        // Update EmployerStreams index: remove stream_id from old employer, add to new.
        remove_employer_stream(&env, &employer, stream_id);
        index_employer_stream(&env, &new_employer, stream_id);

        // Update the stream record.
        stream.employer = new_employer.clone();
        save_stream(&env, &stream);

        events::stream_transferred(&env, stream_id, &employer, &new_employer);
    }

    /// Settle an Active stream whose configured `stop_time` has been reached.
    ///
    /// Callable by anyone. Once `stop_time` has been reached, pays any remaining
    /// claimable tokens to the current receipt holder and transitions the stream
    /// from Active to Exhausted. This lets keepers settle streams without employee
    /// authorization.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to settle
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if stream is not Active
    /// - Panics if `stop_time` is 0 or has not yet been reached
    pub fn settle_stream(env: Env, stream_id: u64) {
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.status, StreamStatus::Active, "stream not active");

        let now = env.ledger().timestamp();
        assert!(
            stream.stop_time > 0 && now >= stream.stop_time,
            "stop_time not reached"
        );
        if let Some(c) = mark_cliff_reached(&env, stream.id, now) {
            events::cliff_reached(&env, stream.id, c);
        }

        let amount = stream_claimable_amount(&env, &stream, now);
        if amount > 0 {
            assert!(!stream.locked, "{}", ERR_REENTRANT);
            stream.locked = true;
            save_stream(&env, &stream);

            stream.withdrawn = stream
                .withdrawn
                .checked_add(amount)
                .expect(ERR_OVERFLOW);
            stream.last_withdraw_time = accrual_start(&env, stream.id, now);
            stream.pending_accrual = 0;

            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(
                &env.current_contract_address(),
                &receipt_owner(&env, &stream),
                &amount,
            );
            events::withdrawn(&env, stream_id, &receipt_owner(&env, &stream), amount);
        }

        stream.withdrawn = stream
            .withdrawn
            .checked_add(amount)
            .expect(ERR_OVERFLOW);
        stream.last_withdraw_time = accrual_start(&env, stream.id, now);
        stream.pending_accrual = 0;
        set_stream_ended_at(&env, stream_id, now);
        stream.status = StreamStatus::Exhausted;
        stream.locked = false;
        save_stream(&env, &stream);
        if amount > 0 {
            let owner = receipt_owner(&env, &stream);
            events::withdrawn(&env, stream_id, &owner, amount);
            let remaining = stream.deposit - stream.withdrawn;
            if stream.low_balance_threshold > 0 && remaining < stream.low_balance_threshold {
                events::low_balance(
                    &env,
                    stream_id,
                    &stream.employer,
                    remaining,
                    stream.low_balance_threshold,
                );
            }
        }
        events::stream_status_changed(&env, stream_id, &StreamStatus::Exhausted);
    }

    /// Admin batch-expires streams that have passed their stop_time + grace period
    /// and have no remaining claimable tokens.
    ///
    /// This is a keeper / janitor function for high-volume deployments. Expired
    /// streams occupy persistent storage and show as Active even though they will
    /// never pay out again. Calling this reclaims storage and keeps off-chain
    /// indexes accurate.
    ///
    /// Eligible criteria (ALL must be true):
    /// - `stop_time > 0`
    /// - `now > stop_time + GRACE_PERIOD` (7-day default grace period)
    /// - `claimable == 0` (employee has nothing left to claim)
    /// - stream status is `Active` (Paused streams are skipped — they may still
    ///   have accrued value the employee has not yet claimed)
    ///
    /// Ineligible streams in the batch are **silently skipped** — the call never
    /// reverts due to a single ineligible entry.
    ///
    /// Expired streams are transitioned to `Exhausted` (not `Cancelled`). The
    /// employee earned nothing at the time of expiry so no cancellation split
    /// is performed. A `stream_expired` event is emitted for each stream that
    /// is transitioned.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce; consumed even if every stream is skipped
    /// - `stream_ids` — IDs of streams to evaluate; ineligible ones are skipped
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match the stored admin
    /// - E009 if `nonce` does not match the stored admin nonce
    pub fn expire_streams(env: Env, admin: Address, nonce: u64, stream_ids: Vec<u64>) {
        admin.require_auth();
        let stored_admin = get_admin(&env);
        assert_eq!(admin, stored_admin, "{}", ERR_NOT_ADMIN);
        consume_admin_nonce(&env, nonce);

        let now = env.ledger().timestamp();

        for stream_id in stream_ids.iter() {
            // Load stream; skip if not found (defensive — IDs are caller-supplied).
            let mut stream = match load_stream(&env, stream_id) {
                Some(s) => s,
                None => continue,
            };

            // Eligibility check — skip ineligible streams without reverting.
            // 1. Must have a hard stop_time.
            if stream.stop_time == 0 {
                continue;
            }
            // 2. Grace period must have elapsed.
            if now <= stream.stop_time.saturating_add(GRACE_PERIOD) {
                continue;
            }
            // 3. Must still be Active (Paused streams could have pending accrual).
            if stream.status != StreamStatus::Active {
                continue;
            }
            // 4. No tokens left for the employee to claim.
            if stream_claimable_amount(&env, &stream, now) > 0 {
                continue;
            }

            set_stream_ended_at(&env, stream_id, now);
            stream.status = StreamStatus::Exhausted;
            save_stream(&env, &stream);
            events::stream_expired(&env, stream_id);
        }
    }

    /// Read the full state of a stream by ID.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to read
    ///
    /// # Returns
    /// The [`Stream`] struct.
    ///
    /// # Errors
    /// - Panics if stream not found
    pub fn get_stream(env: Env, stream_id: u64) -> Stream {
        load_stream_any(&env, stream_id).expect(ERR_STREAM_NOT_FOUND)
    }

    /// Read the full state of up to 50 streams in a single call.
    ///
    /// # Parameters
    /// - `stream_ids` — IDs of the streams to read (max 50)
    ///
    /// # Returns
    /// `Vec<Stream>` in the same order as `stream_ids`.
    ///
    /// # Errors
    /// - Panics if `stream_ids` has more than 50 entries
    /// - Panics if any stream is not found
    pub fn get_streams_batch(env: Env, stream_ids: Vec<u64>) -> Vec<Stream> {
        assert!(stream_ids.len() <= 50, "stream_ids length exceeds 50");
        let mut streams: Vec<Stream> = Vec::new(&env);
        for stream_id in stream_ids.iter() {
            streams.push_back(load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND));
        }
        streams
    }

    /// Query only the status of a stream by ID.
    ///
    /// Lighter than [`get_stream`] because it loads the full [`Stream`] struct
    /// from persistent storage but only returns the `status` field. Off-chain
    /// indexers that poll many stream statuses frequently should prefer this
    /// over `get_stream` to reduce resource consumption (SC-06).
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to query
    ///
    /// # Returns
    /// The [`StreamStatus`] of the stream.
    ///
    /// # Errors
    /// - Panics with "stream not found" if no stream exists for `stream_id`
    pub fn stream_status(env: Env, stream_id: u64) -> StreamStatus {
        load_stream(&env, stream_id)
            .expect("stream not found")
            .status
    }

    /// Return the stream's type. Existing streams default to `Payroll`.
    pub fn get_stream_type(env: Env, stream_id: u64) -> StreamType {
        load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        get_stream_type(&env, stream_id)
    }

    /// Return linear vest terms, or `None` for a payroll stream.
    pub fn get_linear_vest_schedule(env: Env, stream_id: u64) -> Option<LinearVestSchedule> {
        load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        get_linear_vest_schedule(&env, stream_id)
    }

    /// Query how many tokens the employee can withdraw right now.
    ///
    /// Returns 0 for Cancelled or Exhausted streams.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to query
    ///
    /// # Returns
    /// Claimable token amount as `i128`.
    ///
    /// # Errors
    /// - Panics if stream not found
    pub fn claimable(env: Env, stream_id: u64) -> i128 {
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        stream_claimable_amount(&env, &stream, env.ledger().timestamp())
    }

    /// Proactively checks whether an Active stream is approaching or has passed
    /// its `stop_time`, emitting an event so off-chain keeper bots can monitor
    /// streams that need attention. This function makes no state changes.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to check
    ///
    /// # Events
    /// - `stream_near_expiry` — if `stop_time > 0`, stream is `Active`, and
    ///   `stop_time - now < 86400` (24 hours)
    /// - `stream_expired_unresolved` — if `stop_time > 0`, stream is `Active`,
    ///   and `now > stop_time`
    ///
    /// # Errors
    /// - Panics if stream not found
    pub fn check_stream_expiry(env: Env, stream_id: u64) {
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.status, StreamStatus::Active, "stream is not active");
        if stream.stop_time == 0 {
            return;
        }

        let now = env.ledger().timestamp();
        let seconds_remaining = stream.stop_time.saturating_sub(now) as i128;

        if now > stream.stop_time {
            events::stream_expired_unresolved(&env, stream_id, stream.stop_time);
        } else if seconds_remaining < SECONDS_PER_DAY as i128 {
            events::stream_near_expiry(&env, stream_id, seconds_remaining);
        }
    }

    /// Query claimable amounts for up to 50 streams in a single call.
    ///
    /// Missing stream IDs return 0. Results preserve the order of `stream_ids`.
    ///
    /// # Parameters
    /// - `stream_ids` — IDs of the streams to query (max 50)
    ///
    /// # Returns
    /// `Vec<i128>` of claimable amounts in the same order as `stream_ids`.
    ///
    /// # Errors
    /// - Panics if `stream_ids` has more than 50 entries
    /// - E004 if claimable amount arithmetic overflows
    pub fn claimable_batch(env: Env, stream_ids: Vec<u64>) -> Vec<i128> {
        assert!(stream_ids.len() <= 50, "stream_ids length exceeds 50");
        let now = env.ledger().timestamp();
        let mut amounts: Vec<i128> = Vec::new(&env);
        for stream_id in stream_ids.iter() {
            let amount = load_stream(&env, stream_id)
                .map(|stream| stream_claimable_amount(&env, &stream, now))
                .unwrap_or(0);
            amounts.push_back(amount);
        }
        amounts
    }

    /// Query how many tokens would be claimable at an arbitrary timestamp.
    ///
    /// Useful for off-chain projections without advancing ledger time.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to query
    /// - `timestamp` — hypothetical ledger timestamp (seconds)
    ///
    /// # Returns
    /// Claimable amount at `timestamp` as `i128`.
    ///
    /// # Errors
    /// - Panics if stream not found
    pub fn claimable_at(env: Env, stream_id: u64, timestamp: u64) -> i128 {
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        stream_claimable_amount(&env, &stream, timestamp)
    }

    /// Check if a stream's remaining deposit is below the low-balance threshold.
    /// Anyone can call this function to emit an on-demand low-balance alert (PROD-21).
    ///
    /// If the stream has a configured `low_balance_threshold` and the remaining
    /// deposit (total deposit minus already withdrawn) falls below that threshold,
    /// emits a `low_bal` event. This allows keeper bots to trigger alerts without
    /// waiting for an employee withdrawal.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to check
    ///
    /// # Returns
    /// `true` if the threshold was breached (remaining < threshold), `false` otherwise.
    /// Returns `false` if no threshold is configured (threshold = 0).
    ///
    /// # Errors
    /// - E015 if stream not found
    pub fn check_low_balance(env: Env, stream_id: u64) -> bool {
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);

        // No threshold configured
        if stream.low_balance_threshold == 0 {
            return false;
        }

        let remaining = stream.deposit - stream.withdrawn;
        let threshold_breached = remaining < stream.low_balance_threshold;

        if threshold_breached {
            events::low_balance_check(
                &env,
                stream_id,
                &stream.employer,
                remaining,
                stream.low_balance_threshold,
            );
        }

        threshold_breached
    }

    /// Estimate when a stream will be exhausted based on its current rate and
    /// remaining deposit. The on-chain complement to `scripts/project-stream.sh`
    /// (SC-40). Returns `None` for streams that will never exhaust (already
    /// terminal, zero rate, or nothing remaining).
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to project
    ///
    /// # Returns
    /// `Option<u64>` — estimated ledger timestamp of exhaustion, or `None`
    pub fn estimated_exhaustion_time(env: Env, stream_id: u64) -> Option<u64> {
        let stream = load_stream_any(&env, stream_id)?;
        match stream.status {
            StreamStatus::Cancelled | StreamStatus::Exhausted => return None,
            _ => {}
        }
        let now = env.ledger().timestamp();
        let remaining = stream
            .deposit
            .checked_sub(stream.withdrawn)
            .unwrap_or(0)
            .max(0);
        if remaining <= 0 || stream.rate_per_second <= 0 {
            return None;
        }
        let base = if stream.status == StreamStatus::Paused {
            stream.last_withdraw_time
        } else {
            now
        };
        let seconds: u64 = (remaining / stream.rate_per_second) as u64;
        base.checked_add(seconds)
    }

    /// Return the current holder of the stream's receipt (PROD-01).
    ///
    /// The holder is the only address allowed to [`withdraw`] from the stream.
    ///
    /// # Errors
    /// - E015 if the stream does not exist
    pub fn receipt_owner(env: Env, stream_id: u64) -> Address {
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        receipt_owner(&env, &stream)
    }

    /// Transfer a stream's receipt, and with it the right to withdraw future
    /// earnings, from `from` to `to` (PROD-01).
    ///
    /// # Parameters
    /// - `from` — current receipt holder (requires auth)
    /// - `to` — new receipt holder
    /// - `stream_id` — stream whose receipt is transferred
    ///
    /// # Errors
    /// - E015 if the stream does not exist
    /// - E029 if `from` does not hold the receipt
    pub fn transfer_receipt(env: Env, from: Address, to: Address, stream_id: u64) {
        from.require_auth();
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(
            receipt_owner(&env, &stream),
            from,
            "{}",
            ERR_NOT_RECEIPT_OWNER
        );
        set_receipt_owner(&env, stream_id, &to);
        index_employee_stream(&env, &to, stream_id);
        events::receipt_transferred(&env, stream_id, &from, &to);
    }

    /// Save reusable stream parameters under `template_id` (PROD-03).
    ///
    /// Overwrites any existing template with the same ID. The `employee` field
    /// of `params` is ignored when creating from the template.
    ///
    /// # Parameters
    /// - `employer` — template owner (requires auth)
    /// - `template_id` — employer-chosen identifier
    /// - `params` — stream parameters to store
    pub fn save_template(env: Env, employer: Address, template_id: u32, params: StreamParams) {
        employer.require_auth();
        save_template(&env, &employer, template_id, &params);
    }

    /// Create a stream for `employee` using a saved template (PROD-03).
    ///
    /// Equivalent to [`create_stream`] with the template's token, deposit,
    /// rate and stop time.
    ///
    /// # Errors
    /// - E028 if no template exists for `(employer, template_id)`
    /// - Same validations as [`create_stream`]
    pub fn create_stream_from_template(
        env: Env,
        employer: Address,
        employee: Address,
        template_id: u32,
    ) -> u64 {
        let t = load_template(&env, &employer, template_id).expect(ERR_TEMPLATE_NOT_FOUND);
        Self::create_stream(
            env,
            employer,
            employee,
            t.token,
            t.deposit,
            t.rate_per_second,
            t.stop_time,
            None,
            t.cliff_time,
        )
    }

    /// Delete a saved stream template (PROD-20).
    ///
    /// Templates are per-employer: an employer can only delete their own, and
    /// deleting does not affect streams already created from the template.
    ///
    /// # Parameters
    /// - `employer` — template owner (requires auth)
    /// - `template_id` — identifier passed to [`save_template`]
    ///
    /// # Errors
    /// - E031 if `employer` has no template with this ID
    pub fn delete_template(env: Env, employer: Address, template_id: u32) {
        employer.require_auth();
        assert!(
            delete_template(&env, &employer, template_id),
            "{}",
            ERR_TEMPLATE_NOT_FOUND
        );
    }

    /// Return the template saved by `employer` under `template_id`, if any (PROD-20).
    pub fn get_template(env: Env, employer: Address, template_id: u32) -> Option<StreamParams> {
        load_template(&env, &employer, template_id)
    }

    /// Cliff timestamp of a stream, or `None` if it was created without one (PROD-17).
    pub fn get_cliff_time(env: Env, stream_id: u64) -> Option<u64> {
        get_stream_cliff(&env, stream_id)
    }

    /// Return all templates saved by `employer` as `(template_id, params)` pairs (PROD-03).
    pub fn list_templates(env: Env, employer: Address) -> Vec<(u32, StreamParams)> {
        let mut out = Vec::new(&env);
        for id in get_template_ids(&env, &employer).iter() {
            if let Some(p) = load_template(&env, &employer, id) {
                out.push_back((id, p));
            }
        }
        out
    }

    /// Admin upgrades the contract WASM in-place.
    ///
    /// The new WASM must be uploaded to the network before calling this.
    /// After upgrading, call [`migrate`] to confirm the new WASM is operational.
    ///
    /// # Parameters
    /// - `new_wasm_hash` — 32-byte hash of the uploaded WASM blob
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - Panics if admin auth fails
    /// - E009 if `nonce` is wrong
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>, nonce: u64) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect(ERR_ADMIN_NOT_SET);
        admin.require_auth();
        consume_admin_nonce(&env, nonce);
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Step 1 of two-step upgrade: propose a WASM upgrade with a 48-hour time-lock.
    ///
    /// Stores the hash and an unlock timestamp of `now + TIMELOCK_DELAY`.
    /// The upgrade cannot be executed until [`execute_upgrade`] is called after
    /// the delay has elapsed, giving users time to exit if the upgrade is
    /// malicious.
    ///
    /// # Parameters
    /// - `new_wasm_hash` — 32-byte hash of the uploaded WASM blob
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - Panics if admin auth fails
    /// - E009 if `nonce` is wrong
    pub fn propose_upgrade(env: Env, new_wasm_hash: BytesN<32>, nonce: u64) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect(ERR_ADMIN_NOT_SET);
        admin.require_auth();
        consume_admin_nonce(&env, nonce);
        let unlock_time = env.ledger().timestamp() + TIMELOCK_DELAY;
        let pending = PendingUpgrade {
            wasm_hash: new_wasm_hash.clone(),
            unlock_time,
        };
        set_pending_upgrade(&env, &pending);
        events::upgrade_proposed(&env, &new_wasm_hash, unlock_time);
    }

    /// Step 2 of two-step upgrade: execute the pending upgrade after the time-lock has expired.
    ///
    /// # Parameters
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - Panics if admin auth fails
    /// - E009 if `nonce` is wrong
    /// - E025 if no pending upgrade exists
    /// - E026 if the time-lock has not yet expired
    pub fn execute_upgrade(env: Env, nonce: u64) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect(ERR_ADMIN_NOT_SET);
        admin.require_auth();
        consume_admin_nonce(&env, nonce);
        let pending = get_pending_upgrade(&env).expect(ERR_NO_PENDING_UPGRADE);
        assert!(
            env.ledger().timestamp() >= pending.unlock_time,
            "{}",
            ERR_UPGRADE_TIMELOCK_ACTIVE
        );
        clear_pending_upgrade(&env);
        events::upgrade_executed(&env, &pending.wasm_hash);
        env.deployer()
            .update_current_contract_wasm(pending.wasm_hash);
    }

    /// Cancel a pending upgrade before it is executed.
    ///
    /// # Parameters
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - Panics if admin auth fails
    /// - E009 if `nonce` is wrong
    /// - E025 if no pending upgrade exists
    pub fn cancel_upgrade(env: Env, nonce: u64) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect(ERR_ADMIN_NOT_SET);
        admin.require_auth();
        consume_admin_nonce(&env, nonce);
        let _ = get_pending_upgrade(&env).expect(ERR_NO_PENDING_UPGRADE);
        clear_pending_upgrade(&env);
        events::upgrade_cancelled(&env);
    }

    /// No-op migration hook called by the admin after an upgrade.
    ///
    /// Confirms the new WASM is operational and the admin key is still valid.
    /// Also writes the compile-time [`CONTRACT_VERSION`] constant into instance
    /// storage so that `version()` can be queried off-chain for upgrade
    /// verification and version-gated feature flags.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    pub fn migrate(env: Env, admin: Address) {
        admin.require_auth();
        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("admin not set");
        assert_eq!(admin, stored_admin, "not the admin");
        env.storage()
            .instance()
            .set(&DataKey::Version, &CONTRACT_VERSION);
    }

    /// Return the contract version stored by the last `migrate` call.
    ///
    /// Returns `0` if `migrate` has never been called (pre-upgrade state).
    /// After the initial `migrate` call this will return `1`, and subsequent
    /// upgrades should increment [`CONTRACT_VERSION`] so callers can detect
    /// which WASM revision is running.
    ///
    /// # Returns
    /// Version as `u32`.
    pub fn version(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Version).unwrap_or(0)
    }

    /// Return the total number of streams ever created.
    ///
    /// IDs are assigned sequentially starting at 1, so this also equals the
    /// highest stream ID in existence.
    ///
    /// # Returns
    /// Stream count as `u64`.
    pub fn stream_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::StreamCount)
            .unwrap_or(0)
    }

    /// Query the pending admin address set by [`propose_admin`].
    ///
    /// Returns `None` if no two-step admin transfer is in progress.
    /// Off-chain governance tools use this to verify the nominee before
    /// calling [`accept_admin`].
    ///
    /// # Returns
    /// `Some(Address)` of the pending admin, or `None`.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        storage::get_pending_admin(&env)
    }

    /// Return the current admin nonce.
    ///
    /// Use this to build the `nonce` argument for the next admin transaction
    /// (`pause_contract`, `unpause_contract`, `set_min_deposit`,
    /// `set_protocol_fee`, `upgrade`).
    ///
    /// # Returns
    /// Current nonce as `u64`.
    pub fn admin_nonce(env: Env) -> u64 {
        get_admin_nonce(&env)
    }

    /// Return the configured protocol fee in basis points and its treasury.
    ///
    /// Returns `(0, None)` when no fee has been configured.
    pub fn get_protocol_fee(env: Env) -> (u32, Option<Address>) {
        get_protocol_fee(&env)
    }

    /// Return all stream IDs owned by `employer`.
    ///
    /// # Parameters
    /// - `employer` — employer address to query
    ///
    /// # Returns
    /// `Vec<u64>` of stream IDs; empty if the address has no streams.
    pub fn stream_ids_by_employer(env: Env, employer: Address) -> Vec<u64> {
        stream_ids_by_employer(&env, &employer)
    }

    /// Return all stream IDs paying `employee`.
    ///
    /// # Parameters
    /// - `employee` — employee address to query
    ///
    /// # Returns
    /// `Vec<u64>` of stream IDs; empty if the address receives no streams.
    pub fn stream_ids_by_employee(env: Env, employee: Address) -> Vec<u64> {
        stream_ids_by_employee(&env, &employee)
    }

    /// Deprecated alias for [`stream_ids_by_employer`].
    #[deprecated(note = "use stream_ids_by_employer")]
    pub fn streams_by_employer(env: Env, employer: Address) -> Vec<u64> {
        Self::stream_ids_by_employer(env, employer)
    }

    /// Deprecated alias for [`stream_ids_by_employee`].
    #[deprecated(note = "use stream_ids_by_employee")]
    pub fn streams_by_employee(env: Env, employee: Address) -> Vec<u64> {
        Self::stream_ids_by_employee(env, employee)
    }

    /// Return a page of stream IDs paying `employee`.
    ///
    /// `limit` is capped at 50. If `offset` is at or beyond the number of
    /// indexed streams, an empty vector is returned.
    ///
    /// # Parameters
    /// - `employee` — employee address to query
    /// - `offset` — number of stream IDs to skip
    /// - `limit` — maximum number of IDs to return (capped at 50)
    ///
    /// # Returns
    /// `Vec<u64>` page of stream IDs in creation order.
    pub fn streams_by_employee_paginated(
        env: Env,
        employee: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<u64> {
        let stream_ids = stream_ids_by_employee(&env, &employee);
        let mut page: Vec<u64> = Vec::new(&env);
        if offset >= stream_ids.len() {
            return page;
        }

        let end = offset.saturating_add(limit.min(50));
        let mut position = 0u32;
        for stream_id in stream_ids.iter() {
            if position >= end {
                break;
            }
            if position >= offset {
                page.push_back(stream_id);
            }
            position += 1;
        }
        page
    }

    /// Return all stream IDs created within the time range [`from_ts`, `to_ts`],
    /// capped at 30 days and sorted in ascending stream-ID order.
    ///
    /// Streams are indexed by day-bucket (UTC day) at creation time. This makes
    /// efficient catch-up queries for dashboards and indexers without scanning
    /// every stream in storage.
    ///
    /// # Parameters
    /// - `from_ts` — inclusive lower bound (Unix timestamp in seconds)
    /// - `to_ts`   — exclusive upper bound (Unix timestamp in seconds)
    ///
    /// # Returns
    /// `Vec<u64>` of unique stream IDs in ascending order; empty if no streams
    /// were created in the effective range.
    pub fn streams_in_time_range(env: Env, from_ts: u64, to_ts: u64) -> Vec<u64> {
        get_streams_in_time_range(&env, from_ts, to_ts)
    }

    /// Deprecated alias for [`streams_in_time_range`].
    #[deprecated(note = "use streams_in_time_range")]
    pub fn streams_in_range(env: Env, from_ts: u64, to_ts: u64) -> Vec<u64> {
        Self::streams_in_time_range(env, from_ts, to_ts)
    }

    /// Return whether the contract is currently paused.
    ///
    /// # Returns
    /// `true` if the contract is paused, `false` otherwise.
    pub fn is_paused(env: Env) -> bool {
        get_paused(&env)
    }

    /// Return whether a stream has expired and is eligible for cleanup.
    ///
    /// A stream is considered expired when ALL of the following hold:
    /// - `stop_time > 0` (stream has a hard stop)
    /// - `now >= stop_time + GRACE_PERIOD` (grace period has elapsed)
    /// - `claimable == 0` (no tokens remain for the employee to claim)
    ///
    /// Returns `false` for streams with no `stop_time`, streams that still
    /// have remaining claimable tokens, or streams in Paused/Cancelled status
    /// (they may have pending accrual). No auth required.
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to query
    ///
    /// # Returns
    /// `true` if the stream is expired and clean, `false` otherwise.
    ///
    /// # Errors
    /// - Panics if stream not found (E015)
    pub fn is_stream_expired(env: Env, stream_id: u64) -> bool {
        let stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);

        // No hard stop means the stream never expires this way.
        if stream.stop_time == 0 {
            return false;
        }

        // Paused and Cancelled streams are excluded: they may have claimable accrual.
        if stream.status == StreamStatus::Paused || stream.status == StreamStatus::Cancelled {
            return false;
        }

        let now = env.ledger().timestamp();

        // Grace period must have elapsed since stop_time.
        if now < stream.stop_time.saturating_add(GRACE_PERIOD) {
            return false;
        }

        // Any remaining claimable amount means the stream is not yet expired.
        let claimable = stream_claimable_amount(&env, &stream, now);
        claimable == 0
    }
    /// Return the current contract admin address.
    ///
    /// Off-chain tools use this to discover the admin without decoding raw
    /// ledger state.
    ///
    /// # Returns
    /// The admin [`Address`].
    ///
    /// # Errors
    /// - Panics with "admin not set" (E021) if the contract has not been initialised.
    pub fn admin(env: Env) -> Address {
        get_admin(&env)
    }

    /// Return the number of streams owned by `employer`.
    ///
    /// Equivalent to `stream_ids_by_employer(employer).len()` but avoids loading
    /// the full ID vector — useful for pagination and dashboards.
    ///
    /// # Parameters
    /// - `employer` — employer address to query
    ///
    /// # Returns
    /// `u64` count; 0 if the address has no streams.
    pub fn stream_count_by_employer(env: Env, employer: Address) -> u64 {
        stream_ids_by_employer(&env, &employer).len() as u64
    }

    /// Return the number of streams paying `employee`.
    ///
    /// Equivalent to `stream_ids_by_employee(employee).len()` but avoids loading
    /// the full ID vector — useful for pagination and dashboards.
    ///
    /// # Parameters
    /// - `employee` — employee address to query
    ///
    /// # Returns
    /// `u64` count; 0 if the address has no streams.
    pub fn stream_count_by_employee(env: Env, employee: Address) -> u64 {
        stream_ids_by_employee(&env, &employee).len() as u64
    }

    /// Return a page of stream IDs funded with `token`.
    ///
    /// `limit` is capped at 50. If `offset` is at or beyond the number of
    /// indexed streams, an empty vector is returned. No authorization is required.
    ///
    /// # Parameters
    /// - `token` — token contract address to query
    /// - `offset` — number of stream IDs to skip
    /// - `limit` — maximum number of IDs to return (capped at 50)
    ///
    /// # Returns
    /// `Vec<u64>` page of stream IDs in creation order.
    pub fn stream_ids_by_token(env: Env, token: Address, offset: u64, limit: u64) -> Vec<u64> {
        let stream_ids = get_token_streams(&env, &token);
        let mut page: Vec<u64> = Vec::new(&env);
        if offset >= stream_ids.len() as u64 {
            return page;
        }

        let end = offset.saturating_add(limit.min(50));
        let mut position = 0u64;
        for stream_id in stream_ids.iter() {
            if position >= end {
                break;
            }
            if position >= offset {
                page.push_back(stream_id);
            }
            position += 1;
        }
        page
    }

    /// Return the number of streams funded with `token`.
    ///
    /// Equivalent to the length of the token's stream index, without returning
    /// the full ID list. No authorization is required.
    ///
    /// # Parameters
    /// - `token` — token contract address to query
    ///
    /// # Returns
    /// `u64` count; 0 if the token has no streams.
    pub fn stream_count_by_token(env: Env, token: Address) -> u64 {
        get_token_streams(&env, &token).len() as u64
    }

    /// Compute a health score for a stream (0–100).
    ///
    /// Returns a quick at-a-glance indicator of stream health:
    /// - **0** = Critical (cancelled, exhausted, or nearly depleted)
    /// - **50** = At-risk (approaching low balance threshold or paused)
    /// - **100** = Healthy (active with substantial runway)
    ///
    /// The score factors in:
    /// 1. **Status**: Cancelled/Exhausted streams score 0. Paused streams score ≤ 50.
    /// 2. **Time to exhaustion**: Compares remaining deposit against a 30-day burn rate.
    /// 3. **Low balance proximity**: Distance to the configured `low_balance_threshold`.
    ///
    /// No authorization is required. The function never panics; returns 0 for
    /// cancelled or exhausted streams.
    ///
    /// # Parameters
    /// - `stream_id` — the stream to score
    ///
    /// # Returns
    /// A `u32` in the range [0, 100] representing health. Returns 0 if the stream
    /// is not found (treated as critical).
    pub fn stream_health_score(env: Env, stream_id: u64) -> u32 {
        let stream = match load_stream(&env, stream_id) {
            Some(s) => s,
            None => return 0, // Stream not found
        };

        // Cancelled or Exhausted streams are critical
        match stream.status {
            StreamStatus::Cancelled | StreamStatus::Exhausted => return 0,
            StreamStatus::Paused => {
                // Paused streams start at 50 (at-risk) and degrade based on remaining deposit
                let remaining = stream.deposit - stream.withdrawn;
                if remaining <= 0 {
                    return 0;
                }
                // Scale towards 50 based on proximity to low balance threshold
                if stream.low_balance_threshold > 0 && remaining <= stream.low_balance_threshold {
                    return 25;
                }
                return 50;
            }
            StreamStatus::Active => {}
        }

        // For Active streams: compute time to exhaustion vs. 30-day burn
        let remaining = stream.deposit - stream.withdrawn;
        if remaining <= 0 {
            return 0;
        }

        // Calculate 30-day burn rate: rate_per_second * 86400 * 30 seconds
        let thirty_day_burn = match stream.rate_per_second.checked_mul(86400 * 30) {
            Some(burn) => burn,
            None => return 100, // Overflow? Conservative: assume healthy
        };

        // If burn rate is 0 or negative, the stream won't exhaust (infinite runway)
        if thirty_day_burn <= 0 {
            return 100;
        }

        // Runway in hundredths of a 30-day period, in integer math (no floats
        // on-chain): 100 = one period of runway left.
        let runway_centi = match remaining.checked_mul(100) {
            Some(r) => r / thirty_day_burn,
            None => return 100,
        };

        // Score based on runway:
        // - < 0.1 periods (3 days): Score 0–20 (critical)
        // - 0.1–1.0 periods (3–30 days): Score 20–50 (at-risk)
        // - 1.0–10.0 periods (1–10 months): Score 50–90 (warning to healthy)
        // - > 10.0 periods (>10 months): Score 90–100 (very healthy)
        let score: u32 = if runway_centi < 100 {
            // Less than one 30-day period: map [0, 1) to [0, 20)
            (runway_centi * 20 / 100) as u32
        } else if runway_centi < 1_000 {
            // 1–10 periods: map [1, 10) to [50, 90)
            50 + ((runway_centi - 100) * 40 / 900) as u32
        } else {
            // 10+ periods: map [10, 20+) to [90, 100]
            90 + ((runway_centi - 1_000) * 10 / 1_000).min(10) as u32
        };

        // Apply low balance threshold penalty
        if stream.low_balance_threshold > 0 && remaining <= stream.low_balance_threshold * 2 {
            // Halve the score if approaching the low balance threshold
            return (score / 2).max(1);
        }

        score.min(100)
    }
}
