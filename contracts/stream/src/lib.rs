// SPDX-License-Identifier: Apache-2.0

#![no_std]

mod events;
pub mod storage;
pub mod types;
mod validate;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, token, Address, BytesN, Env, Vec};

/// Compile-time contract version.  Increment this constant with every
/// WASM upgrade so that `migrate` stamps the new version into instance
/// storage and `version()` can be queried off-chain.
pub const CONTRACT_VERSION: u32 = 1;
use storage::{
    claimable_amount, clear_pending_admin, consume_admin_nonce, get_admin, get_admin_nonce,
    get_employee_streams, get_employer_streams, get_min_deposit, get_pending_admin,
    get_pending_admin_nonce, get_protocol_fee_bps, get_treasury, index_employee_stream,
    index_employer_stream, load_stream, next_id, save_stream, set_admin, set_min_deposit,
    set_pending_admin, set_pending_admin_nonce, set_protocol_fee_bps, set_treasury,
};
use types::{
    DataKey, Stream, StreamParams, StreamStatus, ERR_BAD_PENDING_NONCE, ERR_FEE_TOO_HIGH,
    ERR_MILESTONE_EXCEEDS, ERR_NOT_PENDING_ADMIN, ERR_NO_PENDING_ADMIN, ERR_REENTRANT,
    ERR_STREAM_CANCELLED, ERR_STREAM_EXHAUSTED, ERR_ZERO_DEPOSIT,
};
use validate::{validate_cliff, validate_create_stream, validate_rate, validate_top_up};

/// Maximum protocol fee: 100 bps (1%).
pub const MAX_PROTOCOL_FEE_BPS: u32 = 100;

/// Pull `deposit` from `employer`: the protocol fee goes to the treasury and the
/// remainder into escrow. Returns the post-fee amount held by the contract.
fn collect_deposit(
    env: &Env,
    token_client: &token::Client,
    employer: &Address,
    deposit: i128,
) -> i128 {
    let fee = deposit
        .checked_mul(get_protocol_fee_bps(env) as i128)
        .expect("fee overflow")
        / 10_000;
    if fee > 0 {
        token_client.transfer(employer, &get_treasury(env), &fee);
    }
    let net = deposit - fee;
    token_client.transfer(employer, &env.current_contract_address(), &net);
    net
}

fn get_paused(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false)
}

fn set_paused(env: &Env, paused: bool) {
    env.storage().instance().set(&DataKey::Paused, &paused);
}

#[contract]
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

    /// Admin sets the protocol fee (in basis points) charged on stream creation.
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - E009 if `nonce` is wrong
    /// - E025 if `fee_bps` > [`MAX_PROTOCOL_FEE_BPS`]
    pub fn set_protocol_fee(env: Env, admin: Address, nonce: u64, fee_bps: u32) {
        admin.require_auth();
        assert_eq!(admin, get_admin(&env), "{}", ERR_NOT_ADMIN);
        consume_admin_nonce(&env, nonce);
        assert!(fee_bps <= MAX_PROTOCOL_FEE_BPS, "{}", ERR_FEE_TOO_HIGH);
        set_protocol_fee_bps(&env, fee_bps);
    }

    /// Admin sets the treasury address receiving protocol fees (defaults to admin).
    pub fn set_treasury(env: Env, admin: Address, nonce: u64, treasury: Address) {
        admin.require_auth();
        assert_eq!(admin, get_admin(&env), "{}", ERR_NOT_ADMIN);
        consume_admin_nonce(&env, nonce);
        set_treasury(&env, &treasury);
    }

    /// Current protocol fee in basis points.
    pub fn protocol_fee(env: Env) -> u32 {
        get_protocol_fee_bps(&env)
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
    /// - `deposit` — total tokens to lock in escrow (must be ≥ min deposit)
    /// - `rate_per_second` — tokens streamed per second (1 – 1,000,000,000)
    /// - `stop_time` — hard stop timestamp in seconds; 0 means indefinite
    /// - `cliff_time` — vesting cliff; nothing is claimable before it (0 = no cliff)
    ///
    /// The protocol fee (if any) is deducted from `deposit` and sent to the
    /// treasury; `stream.deposit` stores the post-fee amount.
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
    /// - E026 if `cliff_time` is non-zero and not between now and `stop_time`
    /// - Panics if `employer` == `employee`
    /// - Panics if the token transfer fails
    pub fn create_stream(
        env: Env,
        employer: Address,
        employee: Address,
        token_address: Address,
        deposit: i128,
        rate_per_second: i128,
        stop_time: u64,
        cliff_time: u64,
    ) -> u64 {
        employer.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);

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
        validate_cliff(cliff_time, stop_time, now);

        let token_client = token::Client::new(&env, &token_address);
        token_client.balance(&employer); // SEP-41 probe
        let deposit = collect_deposit(&env, &token_client, &employer, deposit);

        let id = next_id(&env);
        let stream = Stream {
            id,
            employer: employer.clone(),
            employee: employee.clone(),
            token: token_address,
            deposit,
            withdrawn: 0,
            rate_per_second,
            start_time: now,
            stop_time,
            last_withdraw_time: now,
            status: StreamStatus::Active,
            locked: false,
            cliff_time,
            unlocked: 0,
        };
        save_stream(&env, &stream);
        index_employer_stream(&env, &employer, id);
        index_employee_stream(&env, &employee, id);
        events::stream_created(&env, id, &employer, &employee, rate_per_second);
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
    /// - `params` — list of [`StreamParams`]; must not be empty
    ///
    /// # Returns
    /// `Vec<u64>` of new stream IDs in the same order as `params`.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - Panics if `params` is empty
    /// - Same per-stream validations as [`create_stream`]
    pub fn create_streams_batch(
        env: Env,
        employer: Address,
        params: Vec<StreamParams>,
    ) -> Vec<u64> {
        employer.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);
        assert!(!params.is_empty(), "{}", ERR_EMPTY_PARAMS);

        let now = env.ledger().timestamp();
        let min_deposit = get_min_deposit(&env);
        let mut ids: Vec<u64> = Vec::new(&env);

        for p in params.iter() {
            validate_create_stream(
                p.deposit,
                min_deposit,
                p.rate_per_second,
                p.stop_time,
                now,
                &employer,
                &p.employee,
            );

            let token_client = token::Client::new(&env, &p.token);
            token_client.balance(&employer); // SEP-41 probe
            let deposit = collect_deposit(&env, &token_client, &employer, p.deposit);

            let id = next_id(&env);
            let stream = Stream {
                id,
                employer: employer.clone(),
                employee: p.employee.clone(),
                token: p.token.clone(),
                deposit,
                withdrawn: 0,
                rate_per_second: p.rate_per_second,
                start_time: now,
                stop_time: p.stop_time,
                last_withdraw_time: now,
                status: StreamStatus::Active,
                locked: false,
                cliff_time: 0,
                unlocked: 0,
            };
            save_stream(&env, &stream);
            index_employer_stream(&env, &employer, id);
            index_employee_stream(&env, &p.employee, id);
            events::stream_created(&env, id, &employer, &p.employee, p.rate_per_second);
            ids.push_back(id);
        }

        ids
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
    /// - `employee` — must match the stream's employee (requires auth)
    /// - `stream_id` — ID of the stream to withdraw from
    ///
    /// # Returns
    /// Amount transferred as `i128`; 0 if nothing was claimable.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employee
    /// - Panics if stream is not Active or Exhausted
    /// - E003 if a reentrant withdraw is detected
    pub fn withdraw(env: Env, employee: Address, stream_id: u64) -> i128 {
        employee.require_auth();
        assert!(!get_paused(&env), "{}", ERR_CONTRACT_PAUSED);
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employee, employee, "{}", ERR_NOT_EMPLOYEE);
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Exhausted,
            "{}",
            ERR_STREAM_NOT_ACTIVE
        );

        let now = env.ledger().timestamp();
        let amount = claimable_amount(&stream, now);

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
            .expect("withdrawn overflow");
        stream.last_withdraw_time = now;
        stream.unlocked = 0;
        if stream.withdrawn >= stream.deposit {
            stream.status = StreamStatus::Exhausted;
        }

        let token_client = token::Client::new(&env, &stream.token);
        token_client.transfer(&env.current_contract_address(), &employee, &amount);

        stream.locked = false;
        save_stream(&env, &stream);
        events::withdrawn(&env, stream_id, &employee, amount);
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
    /// `Vec<(u64, i128)>` — list of `(stream_id, amount_withdrawn)` pairs for
    /// every stream from which tokens were actually transferred. Empty if
    /// nothing was claimable.
    ///
    /// # Errors
    /// - Panics if contract is paused
    /// - E003 if a reentrant withdraw is detected on any stream
    pub fn withdraw_all(env: Env, employee: Address) -> Vec<(u64, i128)> {
        employee.require_auth();
        assert!(!get_paused(&env), "contract is paused");

        let stream_ids = get_employee_streams(&env, &employee);
        let mut results: Vec<(u64, i128)> = Vec::new(&env);
        let now = env.ledger().timestamp();

        for stream_id in stream_ids.iter() {
            let mut stream = match load_stream(&env, stream_id) {
                Some(s) => s,
                None => continue,
            };

            // Skip streams that cannot be withdrawn from
            if stream.status == StreamStatus::Cancelled || stream.status == StreamStatus::Paused {
                continue;
            }

            // Only Active and Exhausted streams are eligible
            if stream.status != StreamStatus::Active && stream.status != StreamStatus::Exhausted {
                continue;
            }

            let amount = claimable_amount(&stream, now);
            if amount == 0 {
                continue;
            }

            assert!(!stream.locked, "{}", ERR_REENTRANT);
            stream.locked = true;
            save_stream(&env, &stream);

            stream.withdrawn = stream
                .withdrawn
                .checked_add(amount)
                .expect("withdrawn overflow");
            stream.last_withdraw_time = now;
            stream.unlocked = 0;
            if stream.withdrawn >= stream.deposit {
                stream.status = StreamStatus::Exhausted;
            }

            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &employee, &amount);

            stream.locked = false;
            save_stream(&env, &stream);
            events::withdrawn(&env, stream_id, &employee, amount);

            results.push_back((stream_id, amount));
        }

        results
    }

    /// Employer unlocks a fixed `amount` of the deposit for immediate withdrawal
    /// (milestone-based unlock), tracked separately from time-based accrual.
    ///
    /// Multiple milestones may be set as long as the total unlocked amount does
    /// not exceed the remaining (unwithdrawn) deposit. The employee claims the
    /// unlocked amount via the normal [`withdraw`] call.
    ///
    /// # Errors
    /// - Panics if stream not found or caller is not the stream's employer
    /// - Panics if stream is Cancelled or Exhausted
    /// - E023 if `amount` ≤ 0
    /// - E027 if the total unlocked amount would exceed the remaining deposit
    pub fn set_milestone(env: Env, employer: Address, stream_id: u64, amount: i128) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
            "{}",
            ERR_STREAM_ALREADY_ENDED
        );
        validate_top_up(amount);
        let unlocked = stream
            .unlocked
            .checked_add(amount)
            .expect("unlocked overflow");
        assert!(
            unlocked <= stream.deposit - stream.withdrawn,
            "{}",
            ERR_MILESTONE_EXCEEDS
        );
        stream.unlocked = unlocked;
        save_stream(&env, &stream);
        events::milestone_unlocked(&env, stream_id, &employer, amount);
    }

    /// Employer tops up an active stream with additional funds.
    ///
    /// Increases `deposit` by `amount`. The stream's rate and timeline are
    /// unchanged; the extra funds simply extend how long the stream can run.
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
    /// - Panics if the token transfer fails
    pub fn top_up(env: Env, employer: Address, stream_id: u64, amount: i128) {
        employer.require_auth();
        validate_top_up(amount);
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

        let token_client = token::Client::new(&env, &stream.token);
        token_client.transfer(&employer, &env.current_contract_address(), &amount);

        stream.deposit = stream.deposit.checked_add(amount).expect(ERR_OVERFLOW);
        save_stream(&env, &stream);
        events::topped_up(&env, stream_id, &employer, amount);
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
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - Panics if stream is not Active
    pub fn pause_stream(env: Env, employer: Address, stream_id: u64) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert_eq!(
            stream.status,
            StreamStatus::Active,
            "{}",
            ERR_STREAM_NOT_ACTIVE
        );
        stream.status = StreamStatus::Paused;
        save_stream(&env, &stream);
        events::stream_status_changed(&env, stream_id, &StreamStatus::Paused);
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
        stream.last_withdraw_time = env.ledger().timestamp();
        stream.status = StreamStatus::Active;
        save_stream(&env, &stream);
        events::stream_status_changed(&env, stream_id, &StreamStatus::Active);
    }

    /// Employer updates the `rate_per_second` on an Active or Paused stream.
    ///
    /// Before changing the rate, any tokens accrued since the last withdrawal
    /// are settled by resetting `last_withdraw_time` to the current ledger
    /// timestamp.  This ensures the employee is credited at the old rate for
    /// all elapsed time and will accrue at the new rate going forward.
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

        let mut stream = load_stream(&env, stream_id).expect("stream not found");
        assert_eq!(stream.employer, employer, "not the employer");
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
            "stream not active or paused"
        );

        // Settle accrued-but-not-withdrawn tokens by snapshotting last_withdraw_time
        // to now.  The next claimable calculation will start from this point at
        // the new rate.  (For a Paused stream elapsed is already 0 so this is a
        // no-op in terms of accrual, but we still reset for consistency.)
        stream.last_withdraw_time = env.ledger().timestamp();

        let old_rate = stream.rate_per_second;
        stream.rate_per_second = new_rate;
        save_stream(&env, &stream);
        events::rate_updated(&env, stream_id, old_rate, new_rate);
    }

    /// Employer cancels a stream and reclaims unstreamed funds.    ///
    /// The employee receives all tokens earned up to the cancellation time.
    /// The employer is refunded the remaining deposit. Works on both Active
    /// and Paused streams.
    ///
    /// # Parameters
    /// - `employer` — must match the stream's employer (requires auth)
    /// - `stream_id` — ID of the stream to cancel
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if caller is not the stream's employer
    /// - Panics if stream is already Cancelled or Exhausted
    pub fn cancel_stream(env: Env, employer: Address, stream_id: u64) {
        employer.require_auth();
        let mut stream = load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND);
        assert_eq!(stream.employer, employer, "{}", ERR_NOT_EMPLOYER);
        assert!(
            stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
            "{}",
            ERR_STREAM_ALREADY_ENDED
        );

        let now = env.ledger().timestamp();
        let claimable = claimable_amount(&stream, now);
        let token_client = token::Client::new(&env, &stream.token);

        if claimable > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &stream.employee,
                &claimable,
            );
            stream.withdrawn = stream
                .withdrawn
                .checked_add(claimable)
                .expect("withdrawn overflow");
        }

        let refund = stream
            .deposit
            .checked_sub(stream.withdrawn)
            .unwrap_or(0)
            .max(0);
        if refund > 0 {
            token_client.transfer(&env.current_contract_address(), &employer, &refund);
        }

        stream.status = StreamStatus::Cancelled;
        save_stream(&env, &stream);
        events::stream_cancelled(
            &env,
            stream_id,
            &employer,
            &stream.employee,
            claimable,
            refund,
        );
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
    /// - `stream_ids` — IDs of the streams to cancel; must not be empty
    ///
    /// # Errors
    /// - Panics if `stream_ids` is empty
    /// - Panics if any stream is not found
    /// - Panics if any stream does not belong to `employer`
    /// - Panics if any stream is already Cancelled or Exhausted
    pub fn cancel_streams_batch(env: Env, employer: Address, stream_ids: Vec<u64>) {
        employer.require_auth();
        assert!(!stream_ids.is_empty(), "stream_ids must not be empty");

        let now = env.ledger().timestamp();

        for stream_id in stream_ids.iter() {
            let mut stream = load_stream(&env, stream_id).expect("stream not found");
            assert_eq!(stream.employer, employer, "not the employer");
            assert!(
                stream.status == StreamStatus::Active || stream.status == StreamStatus::Paused,
                "stream already ended"
            );

            let claimable = claimable_amount(&stream, now);
            let token_client = token::Client::new(&env, &stream.token);

            if claimable > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &stream.employee,
                    &claimable,
                );
                stream.withdrawn = stream
                    .withdrawn
                    .checked_add(claimable)
                    .expect("withdrawn overflow");
            }

            let refund = stream
                .deposit
                .checked_sub(stream.withdrawn)
                .unwrap_or(0)
                .max(0);
            if refund > 0 {
                token_client.transfer(&env.current_contract_address(), &employer, &refund);
            }

            stream.status = StreamStatus::Cancelled;
            save_stream(&env, &stream);
            events::stream_status_changed(&env, stream_id, &StreamStatus::Cancelled);
        }
    }

    /// Settle a stream that has passed its `stop_time` but whose status is still Active.
    ///
    /// Callable by anyone. Transitions the stream from Active to Exhausted when
    /// `stop_time > 0 && now >= stop_time && claimable == 0`. This allows off-chain
    /// indexers that rely on `status == Active` as a proxy for "has remaining value"
    /// to get an accurate status without requiring the employee to call `withdraw`
    /// (SC-02).
    ///
    /// # Parameters
    /// - `stream_id` — ID of the stream to settle
    ///
    /// # Errors
    /// - Panics if stream not found
    /// - Panics if stream is not Active
    /// - Panics if `stop_time` is 0 or has not yet been reached
    /// - Panics if there are still tokens claimable (stream is not yet fully exhausted)
    pub fn settle_stream(env: Env, stream_id: u64) {
        let mut stream = load_stream(&env, stream_id).expect("stream not found");
        assert_eq!(stream.status, StreamStatus::Active, "stream not active");

        let now = env.ledger().timestamp();
        assert!(
            stream.stop_time > 0 && now >= stream.stop_time,
            "stop_time not reached"
        );

        let remaining = claimable_amount(&stream, now);
        assert!(remaining == 0, "stream still has claimable tokens");

        stream.status = StreamStatus::Exhausted;
        save_stream(&env, &stream);
        events::stream_status_changed(&env, stream_id, &StreamStatus::Exhausted);
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
        load_stream(&env, stream_id).expect(ERR_STREAM_NOT_FOUND)
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
        claimable_amount(&stream, env.ledger().timestamp())
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
        claimable_amount(&stream, timestamp)
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
    /// (`pause_contract`, `unpause_contract`, `set_min_deposit`, `upgrade`).
    ///
    /// # Returns
    /// Current nonce as `u64`.
    pub fn admin_nonce(env: Env) -> u64 {
        get_admin_nonce(&env)
    }

    /// Return all stream IDs owned by `employer`.
    ///
    /// # Parameters
    /// - `employer` — employer address to query
    ///
    /// # Returns
    /// `Vec<u64>` of stream IDs; empty if the address has no streams.
    pub fn streams_by_employer(env: Env, employer: Address) -> Vec<u64> {
        get_employer_streams(&env, &employer)
    }

    /// Return all stream IDs paying `employee`.
    ///
    /// # Parameters
    /// - `employee` — employee address to query
    ///
    /// # Returns
    /// `Vec<u64>` of stream IDs; empty if the address receives no streams.
    pub fn streams_by_employee(env: Env, employee: Address) -> Vec<u64> {
        get_employee_streams(&env, &employee)
    }

    /// Return the number of streams owned by `employer`.
    ///
    /// Equivalent to `streams_by_employer(employer).len()` but avoids loading
    /// the full ID vector — useful for pagination and dashboards.
    ///
    /// # Parameters
    /// - `employer` — employer address to query
    ///
    /// # Returns
    /// `u64` count; 0 if the address has no streams.
    pub fn stream_count_by_employer(env: Env, employer: Address) -> u64 {
        get_employer_streams(&env, &employer).len() as u64
    }

    /// Return the number of streams paying `employee`.
    ///
    /// Equivalent to `streams_by_employee(employee).len()` but avoids loading
    /// the full ID vector — useful for pagination and dashboards.
    ///
    /// # Parameters
    /// - `employee` — employee address to query
    ///
    /// # Returns
    /// `u64` count; 0 if the address has no streams.
    pub fn stream_count_by_employee(env: Env, employee: Address) -> u64 {
        get_employee_streams(&env, &employee).len() as u64
    }
}
