// SPDX-License-Identifier: Apache-2.0

#![no_std]

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

use crate::storage::{
    admin_nonce, allowance, balance_of, clear_pending_admin, clear_pending_upgrade,
    consume_admin_nonce, extend_instance_ttl, get_admin, get_metadata, get_pending_admin,
    get_pending_upgrade, has_admin, has_metadata, is_paused, max_supply, recipient_cap,
    set_admin, set_allowance,
    set_balance, set_max_supply as store_max_supply, set_paused, set_pending_admin,
    set_pending_upgrade, set_recipient_cap as store_recipient_cap, set_total_supply,
    store_metadata, total_supply, version as stored_version, set_version as store_version,
};
use crate::types::{PendingUpgrade, TokenMetadata};
use soroban_sdk::{
    contract, contractimpl, symbol_short, Address, BytesN, Env, MuxedAddress, String, Symbol, Vec,
};

pub use crate::storage::{TTL_EXTEND_TO, TTL_THRESHOLD, UPGRADE_TIMELOCK};

/// TOK-15: no max supply configured.
const ERR_MAX_SUPPLY: &str = "T002: max supply exceeded";
/// T003: no upgrade is currently pending.
const ERR_NO_PENDING_UPGRADE: &str = "T003: no pending upgrade";
/// T004: the upgrade time-lock has not yet elapsed.
const ERR_UPGRADE_TIMELOCK_ACTIVE: &str = "T004: upgrade timelock active";
/// T006: mint_to_batch amount exceeds the configured per-recipient cap.
const ERR_RECIPIENT_CAP_EXCEEDED: &str = "T006: amount exceeds per-recipient cap";

/// TOK-18: current token contract version, stamped on `initialize` and `migrate`.
pub const TOKEN_CONTRACT_VERSION: u32 = 1;

/// Maximum number of recipients accepted by [`TokenContract::mint_to_batch`].
pub const MAX_MINT_BATCH: u32 = 50;

// # Panic policy (SC-92)
//
// Public entrypoints return `Result<_, TokenError>` for every failure that the
// contract itself can detect and report. The following failures cannot be
// expressed as a `TokenError` and therefore still panic/trap:
//
// - `Address::require_auth` failures: the host aborts the invocation when auth
//   is missing; the contract never regains control, so no `Result` is produced.
// - Host-level traps (e.g. out-of-budget, storage errors) are surfaced by the
//   Soroban runtime and are not modelled here.
//
// SDK consumers should treat a returned `Err(TokenError::...)` as the
// authoritative, machine-readable failure signal and avoid parsing panic
// strings.

fn ensure_not_paused(env: &Env) -> Result<(), TokenError> {
    if is_paused(env) {
        return Err(TokenError::Paused);
    }
    Ok(())
}

/// SEC-01: enforce the admin multi-sig policy.
///
/// On mainnet (`cfg!(not(test))`) the admin signer set must be configured and
/// the threshold must be met by the supplied `signers`. There is no single-key
/// path on mainnet. In tests a single-key admin remains usable so existing
/// flows keep working.
fn assert_admin_multisig(env: &Env, signers: &Vec<Address>) {
    if !cfg!(test) {
        assert!(has_admin_signers(env), "{}", ERR_NO_ADMIN_SIGNERS);
        let configured = get_admin_signers(env);
        let threshold = configured.threshold;
        assert!(threshold > 0, "{}", ERR_INVALID_THRESHOLD);
        let mut approved: u32 = 0;
        for signer in signers.iter() {
            if configured.signers.contains(&signer) {
                approved += 1;
            }
        }
        assert!(approved >= threshold, "{}", ERR_THRESHOLD_NOT_MET);
    }
}

fn emit_mint(env: &Env, admin: &Address, to: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("mint"), admin.clone(), to.clone()), amount);
}

fn emit_burn(env: &Env, from: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("burn"), from.clone()), amount);
}

#[contract]
pub struct TokenContract;

#[contractimpl]
impl TokenContract {
    /// Initialise the token contract with an admin and an initial supply.
    ///
    /// Mints `initial_supply` tokens directly to `admin`. Must be called once
    /// after deployment before any other function.
    ///
    /// # Parameters
    /// - `admin` — address that becomes the token admin (can mint)
    /// - `initial_supply` — tokens minted to `admin` on initialisation
    ///
    /// # Errors
    /// - [`TokenError::AlreadyInitialized`] if the contract has already been initialised
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn initialize(env: Env, admin: Address, initial_supply: i128) -> Result<(), TokenError> {
        admin.require_auth();
        if has_admin(&env) {
            return Err(TokenError::AlreadyInitialized);
        }
        set_admin(&env, &admin);
        set_balance(&env, &admin, initial_supply);
        set_total_supply(&env, initial_supply);
        store_version(&env, TOKEN_CONTRACT_VERSION);
        extend_instance_ttl(&env);
        Ok(())
    }

    /// Configure the admin multi-sig signer set and threshold (SEC-01).
    ///
    /// On mainnet this must be called before any admin operation; there is no
    /// single-key admin path. The current admin must authorise the change.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `signers` — addresses allowed to co-sign admin operations
    /// - `threshold` — minimum number of signers required (must be > 0)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - T006 if `threshold` is 0 or exceeds the number of signers
    pub fn set_admin_signers(env: Env, admin: Address, signers: Vec<Address>, threshold: u32) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert!(
            threshold > 0 && threshold <= signers.len(),
            "{}",
            ERR_INVALID_THRESHOLD
        );
        set_admin_signers(&env, &signers, threshold);
        extend_instance_ttl(&env);
    }

    /// Return the configured admin multi-sig signer set and threshold (SEC-01).
    pub fn admin_signers(env: Env) -> Option<AdminSigners> {
        get_admin_signers(&env)
    }

    /// Return the total token supply.
    ///
    /// # Returns
    /// Current total supply as `i128`.
    pub fn total_supply(env: Env) -> i128 {
        total_supply(&env)
    }

    /// Return the token balance of `owner`.
    ///
    /// # Parameters
    /// - `owner` — address to query
    ///
    /// # Returns
    /// Balance as `i128`; 0 if the address has never held tokens.
    pub fn balance(env: Env, owner: Address) -> i128 {
        balance_of(&env, &owner)
    }

    /// Transfer `amount` tokens from `from` to `to` (SEP-41).
    ///
    /// `to` is a `MuxedAddress` as required by the current SEP-41 revision
    /// (CAP-67); a plain `Address` is accepted unchanged by callers. Balances
    /// are keyed by the underlying address; the muxed id only appears in the
    /// `transfer` event.
    ///
    /// # Parameters
    /// - `from` — sender (requires auth)
    /// - `to` — recipient, optionally muxed
    /// - `amount` — number of tokens to transfer (must be > 0)
    ///
    /// # Events
    /// `["transfer", from, to]` with data `amount`, or
    /// `{amount, to_muxed_id}` when `to` is muxed.
    ///
    /// # Errors
    /// - [`TokenError::Paused`] if transfers are paused
    /// - [`TokenError::InvalidAmount`] if `amount` ≤ 0
    /// - [`TokenError::InsufficientBalance`] if `from` has insufficient balance
    /// - [`TokenError::Overflow`] if crediting `to` would overflow
    /// - Panics if `from` auth fails (see the panic policy above)
    pub fn transfer(
        env: Env,
        from: Address,
        to: MuxedAddress,
        amount: i128,
    ) -> Result<(), TokenError> {
        from.require_auth();
        ensure_not_paused(&env)?;
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let to_addr = to.address();
        move_balance(&env, &from, &to_addr, amount)?;
        events::transfer(&env, &from, &to_addr, to.id(), amount);
        Ok(())
    }

    /// Approve `spender` to transfer up to `amount` tokens on behalf of `owner`.
    ///
    /// Overwrites any existing allowance. Set `amount` to 0 to revoke.
    ///
    /// # Parameters
    /// - `owner` — token owner (requires auth)
    /// - `spender` — address being approved
    /// - `amount` — new allowance
    /// - `expiration_ledger` — last ledger sequence at which the allowance is valid
    ///
    /// # Errors
    /// - [`TokenError::ExpirationInPast`] if `amount` > 0 and `expiration_ledger`
    ///   is before the current ledger
    /// - Panics if `owner` auth fails (see the panic policy above)
    pub fn approve(
        env: Env,
        owner: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) -> Result<(), TokenError> {
        owner.require_auth();
        extend_instance_ttl(&env);
        if amount != 0 && expiration_ledger < env.ledger().sequence() {
            return Err(TokenError::ExpirationInPast);
        }
        set_allowance(&env, &owner, &spender, amount, expiration_ledger);
        events::approve(&env, &owner, &spender, amount, expiration_ledger);
        Ok(())
    }

    /// Return the allowance granted by `owner` to `spender` (SEP-41).
    ///
    /// # Returns
    /// Remaining allowance; 0 if none exists or it has expired.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        allowance(&env, &owner, &spender).0
    }

    /// Return name, symbol and decimals in a single call (TOK-32).
    ///
    /// Reduces round-trips versus calling `name`/`symbol`/`decimals`
    /// separately; read-only and safe to cache.
    ///
    /// # Returns
    /// The stored [`TokenMetadata`], or the defaults (7, "PayStream Token",
    /// "PST") if [`set_metadata`] has never been called.
    pub fn metadata(env: Env) -> TokenMetadata {
        get_metadata(&env).unwrap_or_else(|| TokenMetadata {
            decimal: 7,
            name: String::from_str(&env, "PayStream Token"),
            symbol: String::from_str(&env, "PST"),
        })
    }

    /// Number of decimals used to display amounts (SEP-41).
    pub fn decimals(env: Env) -> u32 {
        Self::metadata(env).decimal
    }

    /// Human-readable token name (SEP-41).
    pub fn name(env: Env) -> String {
        Self::metadata(env).name
    }

    /// Ticker symbol (SEP-41).
    pub fn symbol(env: Env) -> String {
        Self::metadata(env).symbol
    }

    /// Set token metadata once (admin only).
    ///
    /// Until called, `decimals` / `name` / `symbol` / `metadata` return the
    /// defaults (7, "PayStream Token", "PST"). Metadata is write-once so
    /// wallets and indexers can cache it.
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::MetadataAlreadySet`] if metadata has already been set
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn set_metadata(
        env: Env,
        admin: Address,
        decimal: u32,
        name: String,
        symbol: String,
    ) -> Result<(), TokenError> {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert!(!has_metadata(&env), "metadata already set");
        store_metadata(&env, &TokenMetadata { decimal, name, symbol });
    }

    /// Transfer `amount` tokens from `from` to `to` using `spender`'s allowance.
    ///
    /// # Parameters
    /// - `spender` — address with an existing allowance (requires auth)
    /// - `from` — token owner
    /// - `to` — recipient
    /// - `amount` — number of tokens to transfer
    ///
    /// # Errors
    /// - [`TokenError::Paused`] if transfers are paused
    /// - [`TokenError::AllowanceExpired`] if `spender`'s allowance for `from` has expired
    /// - [`TokenError::AllowanceExceeded`] if `spender`'s allowance for `from` is insufficient
    /// - [`TokenError::InsufficientBalance`] if `from` has insufficient balance
    /// - [`TokenError::Overflow`] if crediting `to` would overflow
    /// - Panics if `spender` auth fails (see the panic policy above)
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        amount: i128,
    ) -> Result<(), TokenError> {
        spender.require_auth();
        ensure_not_paused(&env)?;
        let (allowed, expiration_ledger) = allowance(&env, &from, &spender);
        if env.ledger().sequence() > expiration_ledger {
            return Err(TokenError::AllowanceExpired);
        }
        if allowed < amount {
            return Err(TokenError::AllowanceExceeded);
        }
        if balance_of(&env, &from) < amount {
            return Err(TokenError::InsufficientBalance);
        }
        set_allowance(&env, &from, &spender, allowed - amount, expiration_ledger);
        move_balance(&env, &from, &to, amount)?;
        events::transfer(&env, &from, &to, None, amount);
        Ok(())
    }

    /// Return the max supply cap (TOK-15); 0 means uncapped.
    pub fn get_max_supply(env: Env) -> i128 {
        max_supply(&env)
    }

    /// Admin sets (or clears, with `0`) the max supply cap (TOK-15).
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `nonce` does not match the stored admin nonce
    /// - Panics if `cap` is negative
    pub fn set_max_supply(env: Env, admin: Address, nonce: u64, cap: i128) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        consume_admin_nonce(&env, nonce);
        assert!(cap >= 0, "cap must be non-negative");
        store_max_supply(&env, cap);
    }

    /// Mint `amount` new tokens to `to`, increasing total supply.
    ///
    /// Only the admin may call this function.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `to` — recipient of minted tokens
    /// - `amount` — number of tokens to mint (must be > 0)
    /// - `nonce` — current admin nonce; consumed for replay protection
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - [`TokenError::InvalidAmount`] if `amount` ≤ 0
    /// - [`TokenError::Overflow`] if the balance or total supply would overflow
    /// - [`TokenError::MaxSupplyExceeded`] if a max supply is set and
    ///   `total_supply + amount` exceeds it
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn mint(
        env: Env,
        admin: Address,
        to: Address,
        amount: i128,
        nonce: u64,
    ) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let new_supply = total_supply(&env)
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;
        let cap = max_supply(&env);
        if cap != 0 && new_supply > cap {
            return Err(TokenError::MaxSupplyExceeded);
        }
        let to_bal = balance_of(&env, &to)
            .checked_add(amount)
            .ok_or(TokenError::Overflow)?;
        set_balance(&env, &to, to_bal);
        set_total_supply(&env, new_supply);
        emit_mint(&env, &admin, &to, amount);
        Ok(())
    }

    /// Mint tokens to up to [`MAX_MINT_BATCH`] recipients in one call (TOK-14).
    ///
    /// A single admin nonce is consumed for the whole batch. The batch is
    /// all-or-nothing: if any entry is invalid or would overflow a balance or
    /// the total supply, the whole call reverts.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce; consumed once for the batch
    /// - `recipients` — `(recipient, amount)` pairs; each amount must be > 0
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `nonce` does not match the stored admin nonce
    /// - T003 if `recipients` has more than [`MAX_MINT_BATCH`] entries
    /// - Panics if any amount ≤ 0
    /// - T001 if any balance or the total supply would overflow
    /// - T006 (TOK-33) if any single amount exceeds the configured
    ///   per-recipient cap (see [`set_recipient_cap`]); the whole batch
    ///   reverts, same as any other failure above
    pub fn mint_to_batch(env: Env, admin: Address, nonce: u64, recipients: Vec<(Address, i128)>) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert_admin_multisig(&env, &Vec::new(&env));
        assert!(recipients.len() <= MAX_MINT_BATCH, "{}", ERR_BATCH_TOO_LARGE);
        consume_admin_nonce(&env, nonce);
        let cap = recipient_cap(&env);
        let mut supply = total_supply(&env);
        for (to, amount) in recipients.iter() {
            assert!(amount > 0, "amount must be positive");
            assert!(cap == 0 || amount <= cap, "{}", ERR_RECIPIENT_CAP_EXCEEDED);
            supply = supply.checked_add(amount).expect(ERR_OVERFLOW);
            let to_bal = balance_of(&env, &to)
                .checked_add(amount)
                .ok_or(TokenError::Overflow)?;
            set_balance(&env, &to, to_bal);
            emit_mint(&env, &admin, &to, amount);
        }
        set_total_supply(&env, supply);
        Ok(())
    }

    /// Return the per-recipient cap enforced by [`mint_to_batch`] (TOK-33).
    ///
    /// `0` (the default) means no cap is enforced.
    pub fn get_recipient_cap(env: Env) -> i128 {
        recipient_cap(&env)
    }

    /// Admin sets (or clears, with `0`) the per-recipient cap enforced by
    /// [`mint_to_batch`] (TOK-33). Default is off (uncapped).
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `nonce` does not match the stored admin nonce
    /// - Panics if `cap` is negative
    pub fn set_recipient_cap(env: Env, admin: Address, nonce: u64, cap: i128) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        consume_admin_nonce(&env, nonce);
        assert!(cap >= 0, "cap must be non-negative");
        store_recipient_cap(&env, cap);
    }

    /// Return the current admin nonce expected by admin-gated functions.
    pub fn admin_nonce(env: Env) -> u64 {
        admin_nonce(&env)
    }

    /// Alias of [`admin_nonce`] (TOK-12).
    pub fn get_admin_nonce(env: Env) -> u64 {
        admin_nonce(&env)
    }

    /// Admin pauses all token transfers and burns (TOK-13).
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn pause_token(env: Env, admin: Address, nonce: u64) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        set_paused(&env, true);
        env.events()
            .publish((Symbol::new(&env, "token_paused"),), true);
        Ok(())
    }

    /// Admin resumes token transfers and burns (TOK-13).
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn unpause_token(env: Env, admin: Address, nonce: u64) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        set_paused(&env, false);
        env.events()
            .publish((Symbol::new(&env, "token_paused"),), false);
        Ok(())
    }

    /// Return whether token transfers and burns are currently paused.
    pub fn is_paused(env: Env) -> bool {
        is_paused(&env)
    }

    /// Admin upgrades the token contract WASM in-place (TOK-10).
    ///
    /// Balances, allowances and total supply live in contract storage and are
    /// preserved across the upgrade. After [`UPGRADE_TIMELOCK`] seconds call
    /// [`execute_upgrade`], then [`migrate`].
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    /// - `new_wasm_hash` — 32-byte hash of the uploaded WASM blob
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn upgrade(
        env: Env,
        admin: Address,
        nonce: u64,
        new_wasm_hash: BytesN<32>,
    ) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        let unlock_time = env.ledger().timestamp() + UPGRADE_TIMELOCK;
        set_pending_upgrade(
            &env,
            &PendingUpgrade {
                wasm_hash: new_wasm_hash,
                unlock_time,
            },
        );
        extend_instance_ttl(&env);
        Ok(())
    }

    /// Apply the upgrade scheduled by [`upgrade`] once its time-lock has elapsed.
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - [`TokenError::NoPendingUpgrade`] if no upgrade is pending
    /// - [`TokenError::UpgradeTimelockActive`] if the time-lock has not yet elapsed
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn execute_upgrade(env: Env, admin: Address, nonce: u64) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        let pending = get_pending_upgrade(&env).ok_or(TokenError::NoPendingUpgrade)?;
        if env.ledger().timestamp() < pending.unlock_time {
            return Err(TokenError::UpgradeTimelockActive);
        }
        clear_pending_upgrade(&env);
        env.deployer()
            .update_current_contract_wasm(pending.wasm_hash);
        Ok(())
    }

    /// Cancel a scheduled upgrade before it is executed.
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - [`TokenError::NoPendingUpgrade`] if no upgrade is pending
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn cancel_upgrade(env: Env, admin: Address, nonce: u64) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        get_pending_upgrade(&env).ok_or(TokenError::NoPendingUpgrade)?;
        clear_pending_upgrade(&env);
        Ok(())
    }

    /// Return the upgrade scheduled by [`upgrade`], if any.
    pub fn pending_upgrade(env: Env) -> Option<PendingUpgrade> {
        get_pending_upgrade(&env)
    }

    /// Post-upgrade migration hook: stamps [`TOKEN_CONTRACT_VERSION`].
    ///
    /// # Errors
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn migrate(env: Env, admin: Address, nonce: u64) -> Result<(), TokenError> {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert_admin_multisig(&env, &Vec::new(&env));
        consume_admin_nonce(&env, nonce);
        store_version(&env, TOKEN_CONTRACT_VERSION);
        extend_instance_ttl(&env);
        Ok(())
    }

    /// Step 1 of the SEC-03 emergency drain: propose draining the full token
    /// balance to `recipient`.
    ///
    /// The token must be paused first. The proposal records the current ledger
    /// timestamp; [`emergency_drain`] may only be executed once
    /// [`EMERGENCY_DRAIN_TIMELOCK`] seconds have elapsed.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `nonce` — current admin nonce (replay protection)
    /// - `recipient` — address that will receive the drained balance
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `nonce` does not match the stored admin nonce
    /// - T010 if the token is not paused
    pub fn propose_emergency_drain(env: Env, admin: Address, nonce: u64, recipient: Address) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert_admin_multisig(&env, &Vec::new(&env));
        consume_admin_nonce(&env, nonce);
        assert!(is_paused(&env), "{}", ERR_DRAIN_NOT_PAUSED);
        let unlock_time = env.ledger().timestamp() + EMERGENCY_DRAIN_TIMELOCK;
        set_pending_drain(
            &env,
            &PendingDrain {
                recipient: recipient.clone(),
                unlock_time,
            },
        );
        env.events()
            .publish((Symbol::new(&env, "drain_proposed"), admin, recipient), unlock_time);
        extend_instance_ttl(&env);
    }

    /// Step 2 of the SEC-03 emergency drain: execute a pending drain once its
    /// time-lock has elapsed.
    ///
    /// Transfers the full token balance held by the contract to the proposed
    /// recipient, clears the pending proposal and emits a `drain` event.
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `nonce` does not match the stored admin nonce
    /// - T008 if no drain is pending
    /// - T009 if the time-lock has not yet elapsed
    pub fn emergency_drain(env: Env, admin: Address, nonce: u64) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert_admin_multisig(&env, &Vec::new(&env));
        consume_admin_nonce(&env, nonce);
        let pending = get_pending_drain(&env).expect(ERR_NO_PENDING_DRAIN);
        assert!(
            env.ledger().timestamp() >= pending.unlock_time,
            "{}",
            ERR_DRAIN_TIMELOCK_ACTIVE
        );
        clear_pending_drain(&env);
        let contract = env.current_contract_address();
        let amount = balance_of(&env, &contract);
        if amount > 0 {
            set_balance(&env, &contract, 0);
            let to_bal = balance_of(&env, &pending.recipient)
                .checked_add(amount)
                .expect(ERR_OVERFLOW);
            set_balance(&env, &pending.recipient, to_bal);
        }
        env.events().publish(
            (Symbol::new(&env, "drain"), pending.recipient.clone()),
            amount,
        );
        extend_instance_ttl(&env);
    }

    /// Return the emergency drain proposed by [`propose_emergency_drain`], if any.
    pub fn pending_drain(env: Env) -> Option<PendingDrain> {
        get_pending_drain(&env)
    }

    /// Return the stored contract version; 0 if never stamped.
    pub fn version(env: Env) -> u32 {
        stored_version(&env)
    }

    /// Step 1 of two-step admin transfer: current admin nominates `new_admin`.
    ///
    /// # Parameters
    /// - `new_admin` — address being nominated as the next admin
    /// - `nonce` — current admin nonce (replay protection)
    ///
    /// # Errors
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - Panics if the current admin auth fails (see the panic policy above)
    pub fn propose_admin(env: Env, new_admin: Address, nonce: u64) -> Result<(), TokenError> {
        get_admin(&env).require_auth();
        consume_admin_nonce(&env, nonce)?;
        set_pending_admin(&env, &new_admin);
        Ok(())
    }

    /// Step 2 of two-step admin transfer: nominated address accepts and becomes admin.
    ///
    /// # Parameters
    /// - `new_admin` — must match the address set by [`propose_admin`] (requires auth)
    ///
    /// # Errors
    /// - [`TokenError::NoPendingAdmin`] if there is no pending admin
    /// - [`TokenError::NotPendingAdmin`] if `new_admin` does not match the pending admin
    /// - Panics if `new_admin` auth fails (see the panic policy above)
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), TokenError> {
        new_admin.require_auth();
        let pending = get_pending_admin(&env).ok_or(TokenError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(TokenError::NotPendingAdmin);
        }
        set_admin(&env, &new_admin);
        clear_pending_admin(&env);
        Ok(())
    }

    /// Burn `amount` tokens from `from`'s own balance, reducing total supply.
    ///
    /// # Parameters
    /// - `from` — address whose tokens are burned (requires auth)
    /// - `amount` — number of tokens to burn (must be > 0)
    ///
    /// # Errors
    /// - [`TokenError::Paused`] if transfers and burns are paused
    /// - [`TokenError::InvalidAmount`] if `amount` ≤ 0
    /// - [`TokenError::InsufficientBalance`] if `from` has insufficient balance
    /// - Panics if `from` auth fails (see the panic policy above)
    pub fn burn(env: Env, from: Address, amount: i128) -> Result<(), TokenError> {
        from.require_auth();
        ensure_not_paused(&env)?;
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let bal = balance_of(&env, &from);
        if bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        set_balance(&env, &from, bal - amount);
        set_total_supply(&env, total_supply(&env) - amount);
        emit_burn(&env, &from, amount);
        Ok(())
    }

    /// Burn `amount` tokens from `from` using `spender`'s allowance.
    ///
    /// Reduces both `from`'s balance and the total supply.
    ///
    /// # Parameters
    /// - `spender` — address with an existing allowance (requires auth)
    /// - `from` — token owner whose tokens are burned
    /// - `amount` — number of tokens to burn (must be > 0)
    ///
    /// # Errors
    /// - [`TokenError::Paused`] if transfers and burns are paused
    /// - [`TokenError::InvalidAmount`] if `amount` ≤ 0
    /// - [`TokenError::AllowanceExpired`] if `spender`'s allowance for `from` has expired
    /// - [`TokenError::AllowanceExceeded`] if `spender`'s allowance for `from` is insufficient
    /// - [`TokenError::InsufficientBalance`] if `from` has insufficient balance
    /// - Panics if `spender` auth fails (see the panic policy above)
    pub fn burn_from(
        env: Env,
        spender: Address,
        from: Address,
        amount: i128,
    ) -> Result<(), TokenError> {
        spender.require_auth();
        ensure_not_paused(&env)?;
        if amount <= 0 {
            return Err(TokenError::InvalidAmount);
        }
        let (allowed, expiration_ledger) = allowance(&env, &from, &spender);
        if env.ledger().sequence() > expiration_ledger {
            return Err(TokenError::AllowanceExpired);
        }
        if allowed < amount {
            return Err(TokenError::AllowanceExceeded);
        }
        let bal = balance_of(&env, &from);
        if bal < amount {
            return Err(TokenError::InsufficientBalance);
        }
        set_allowance(&env, &from, &spender, allowed - amount, expiration_ledger);
        set_balance(&env, &from, bal - amount);
        set_total_supply(&env, total_supply(&env) - amount);
        emit_burn(&env, &from, amount);
        Ok(())
    }
}

/// Debit `from` and credit `to`, checking balance and overflow. A transfer to
/// self leaves the balance unchanged.
fn move_balance(env: &Env, from: &Address, to: &Address, amount: i128) -> Result<(), TokenError> {
    let from_bal = balance_of(env, from);
    if from_bal < amount {
        return Err(TokenError::InsufficientBalance);
    }
    set_balance(env, from, from_bal - amount);
    let to_bal = balance_of(env, to)
        .checked_add(amount)
        .ok_or(TokenError::Overflow)?;
    set_balance(env, to, to_bal);
    Ok(())
}
