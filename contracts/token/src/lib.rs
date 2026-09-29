// SPDX-License-Identifier: Apache-2.0

#![no_std]

mod storage;
mod types;

#[cfg(test)]
mod test;

use crate::storage::{
    admin_nonce, allowance, balance_of, clear_pending_admin, consume_admin_nonce, get_admin,
    get_pending_admin, set_admin, set_allowance, set_balance, set_pending_admin, set_total_supply,
    total_supply,
};
use soroban_sdk::{contract, contractimpl, Address, Env};

/// T001: token arithmetic overflow.
const ERR_OVERFLOW: &str = "T001: token arithmetic overflow";

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
    /// - Panics if the contract has already been initialised ("already initialized")
    /// - Panics if `admin` auth fails
    pub fn initialize(env: Env, admin: Address, initial_supply: i128) {
        admin.require_auth();
        assert!(!has_admin(&env), "already initialized");
        set_admin(&env, &admin);
        set_balance(&env, &admin, initial_supply);
        set_total_supply(&env, initial_supply);
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

    /// Transfer `amount` tokens from `from` to `to`.
    ///
    /// # Parameters
    /// - `from` — sender (requires auth)
    /// - `to` — recipient
    /// - `amount` — number of tokens to transfer (must be > 0)
    ///
    /// # Errors
    /// - Panics if `amount` ≤ 0
    /// - Panics if `from` has insufficient balance
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        assert!(amount > 0, "amount must be positive");
        let from_bal = balance_of(&env, &from);
        assert!(from_bal >= amount, "insufficient balance");
        set_balance(&env, &from, from_bal - amount);
        let to_bal = balance_of(&env, &to)
            .checked_add(amount)
            .expect(ERR_OVERFLOW);
        set_balance(&env, &to, to_bal);
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
    /// - Panics if `amount` > 0 and `expiration_ledger` is before the current ledger
    pub fn approve(
        env: Env,
        owner: Address,
        spender: Address,
        amount: i128,
        expiration_ledger: u32,
    ) {
        owner.require_auth();
        assert!(
            amount == 0 || expiration_ledger >= env.ledger().sequence(),
            "expiration_ledger is in the past"
        );
        set_allowance(&env, &owner, &spender, amount, expiration_ledger);
    }

    /// Return the allowance granted by `owner` to `spender`.
    ///
    /// # Returns
    /// `(amount, expiration_ledger)`; `(0, 0)` if no allowance exists.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> (i128, u32) {
        allowance(&env, &owner, &spender)
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
    /// - Panics if `spender`'s allowance for `from` has expired
    /// - Panics if `spender`'s allowance for `from` is insufficient
    /// - Panics if `from` has insufficient balance
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        let (allowed, expiration_ledger) = allowance(&env, &from, &spender);
        assert!(
            env.ledger().sequence() <= expiration_ledger,
            "allowance expired"
        );
        assert!(allowed >= amount, "allowance exceeded");
        let from_bal = balance_of(&env, &from);
        assert!(from_bal >= amount, "insufficient balance");
        set_allowance(&env, &from, &spender, allowed - amount, expiration_ledger);
        set_balance(&env, &from, from_bal - amount);
        let to_bal = balance_of(&env, &to)
            .checked_add(amount)
            .expect(ERR_OVERFLOW);
        set_balance(&env, &to, to_bal);
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
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `nonce` does not match the stored admin nonce
    /// - Panics if `amount` ≤ 0
    /// - T001 if the balance or total supply would overflow
    pub fn mint(env: Env, admin: Address, to: Address, amount: i128, nonce: u64) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        consume_admin_nonce(&env, nonce);
        assert!(amount > 0, "amount must be positive");
        let new_supply = total_supply(&env).checked_add(amount).expect(ERR_OVERFLOW);
        let to_bal = balance_of(&env, &to)
            .checked_add(amount)
            .expect(ERR_OVERFLOW);
        set_balance(&env, &to, to_bal);
        set_total_supply(&env, new_supply);
    }

    /// Return the current admin nonce expected by [`mint`].
    pub fn admin_nonce(env: Env) -> u64 {
        admin_nonce(&env)
    }

    /// Step 1 of two-step admin transfer: current admin nominates `new_admin`.
    ///
    /// # Parameters
    /// - `new_admin` — address being nominated as the next admin
    ///
    /// # Errors
    /// - Panics if the current admin auth fails
    pub fn propose_admin(env: Env, new_admin: Address) {
        get_admin(&env).require_auth();
        set_pending_admin(&env, &new_admin);
    }

    /// Step 2 of two-step admin transfer: nominated address accepts and becomes admin.
    ///
    /// # Parameters
    /// - `new_admin` — must match the address set by [`propose_admin`] (requires auth)
    ///
    /// # Errors
    /// - Panics if there is no pending admin
    /// - Panics if `new_admin` does not match the pending admin
    pub fn accept_admin(env: Env, new_admin: Address) {
        new_admin.require_auth();
        let pending = get_pending_admin(&env).expect("no pending admin");
        assert_eq!(pending, new_admin, "not pending admin");
        set_admin(&env, &new_admin);
        clear_pending_admin(&env);
    }

    /// Burn `amount` tokens from `from`'s own balance, reducing total supply.
    ///
    /// # Parameters
    /// - `from` — address whose tokens are burned (requires auth)
    /// - `amount` — number of tokens to burn (must be > 0)
    ///
    /// # Errors
    /// - Panics if `amount` ≤ 0
    /// - Panics if `from` has insufficient balance
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        assert!(amount > 0, "amount must be positive");
        let bal = balance_of(&env, &from);
        assert!(bal >= amount, "insufficient balance");
        set_balance(&env, &from, bal - amount);
        set_total_supply(&env, total_supply(&env) - amount);
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
    /// - Panics if `amount` ≤ 0
    /// - Panics if `spender`'s allowance for `from` has expired
    /// - Panics if `spender`'s allowance for `from` is insufficient
    /// - Panics if `from` has insufficient balance
    pub fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        assert!(amount > 0, "amount must be positive");
        let (allowed, expiration_ledger) = allowance(&env, &from, &spender);
        assert!(
            env.ledger().sequence() <= expiration_ledger,
            "allowance expired"
        );
        assert!(allowed >= amount, "allowance exceeded");
        let bal = balance_of(&env, &from);
        assert!(bal >= amount, "insufficient balance");
        set_allowance(&env, &from, &spender, allowed - amount, expiration_ledger);
        set_balance(&env, &from, bal - amount);
        set_total_supply(&env, total_supply(&env) - amount);
    }
}
