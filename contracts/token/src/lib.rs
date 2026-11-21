// SPDX-License-Identifier: Apache-2.0

#![no_std]

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

use crate::storage::{
    admin_nonce, allowance, balance_of, clear_pending_admin, consume_admin_nonce, get_admin,
    get_pending_admin, has_admin, is_paused, set_admin, set_allowance, set_balance,
    set_paused, set_pending_admin, set_total_supply, total_supply,
};
use soroban_sdk::{contract, contractimpl, symbol_short, Address, BytesN, Env, Symbol, Vec};

pub use crate::storage::{TTL_EXTEND_TO, TTL_THRESHOLD};
pub use crate::types::TokenError;

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
        set_version(&env, TOKEN_CONTRACT_VERSION);
        extend_instance_ttl(&env);
        Ok(())
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

    /// Number of decimals used to display amounts (SEP-41).
    pub fn decimals(env: Env) -> u32 {
        metadata(&env).decimal
    }

    /// Human-readable token name (SEP-41).
    pub fn name(env: Env) -> String {
        metadata(&env).name
    }

    /// Ticker symbol (SEP-41).
    pub fn symbol(env: Env) -> String {
        metadata(&env).symbol
    }

    /// Set token metadata once (admin only).
    ///
    /// Until called, `decimals` / `name` / `symbol` return the defaults (7,
    /// "PayStream Token", "PST"). Metadata is write-once so wallets and
    /// indexers can cache it.
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
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        if has_metadata(&env) {
            return Err(TokenError::MetadataAlreadySet);
        }
        set_metadata(&env, &TokenMetadata { decimal, name, symbol });
        Ok(())
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
    /// - [`TokenError::NotAdmin`] if `admin` does not match the stored admin
    /// - [`TokenError::InvalidNonce`] if `nonce` does not match the stored admin nonce
    /// - [`TokenError::BatchTooLarge`] if `recipients` has more than [`MAX_MINT_BATCH`] entries
    /// - [`TokenError::InvalidAmount`] if any amount ≤ 0
    /// - [`TokenError::Overflow`] if any balance or the total supply would overflow
    /// - Panics if `admin` auth fails (see the panic policy above)
    pub fn mint_to_batch(
        env: Env,
        admin: Address,
        nonce: u64,
        recipients: Vec<(Address, i128)>,
    ) -> Result<(), TokenError> {
        admin.require_auth();
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        if recipients.len() > MAX_MINT_BATCH {
            return Err(TokenError::BatchTooLarge);
        }
        consume_admin_nonce(&env, nonce)?;
        let mut supply = total_supply(&env);
        for (to, amount) in recipients.iter() {
            if amount <= 0 {
                return Err(TokenError::InvalidAmount);
            }
            supply = supply.checked_add(amount).ok_or(TokenError::Overflow)?;
            let to_bal = balance_of(&env, &to)
                .checked_add(amount)
                .ok_or(TokenError::Overflow)?;
            set_balance(&env, &to, to_bal);
            emit_mint(&env, &admin, &to, amount);
        }
        set_total_supply(&env, supply);
        Ok(())
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
        if get_admin(&env) != admin {
            return Err(TokenError::NotAdmin);
        }
        consume_admin_nonce(&env, nonce)?;
        set_version(&env, TOKEN_CONTRACT_VERSION);
        extend_instance_ttl(&env);
        Ok(())
    }

    /// Return the stored contract version; 0 if never stamped.
    pub fn version(env: Env) -> u32 {
        version(&env)
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
