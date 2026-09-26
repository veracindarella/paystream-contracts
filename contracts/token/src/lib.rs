// SPDX-License-Identifier: Apache-2.0

#![no_std]

mod events;
mod storage;
mod types;

#[cfg(test)]
mod test;

use crate::storage::{
    allowance, balance_of, get_admin, get_name, get_symbol, set_admin, set_allowance, set_balance,
    set_metadata, set_total_supply, total_supply,
};
use soroban_sdk::{contract, contractimpl, Address, Env, String};

/// Number of decimal places used by the token (SEP-41 `decimals`).
pub const DECIMALS: u32 = 7;

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
    /// - `name` — human-readable token name (SEP-41 metadata)
    /// - `symbol` — token ticker symbol (SEP-41 metadata)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails
    pub fn initialize(
        env: Env,
        admin: Address,
        initial_supply: i128,
        name: String,
        symbol: String,
    ) {
        admin.require_auth();
        set_admin(&env, &admin);
        set_metadata(&env, &name, &symbol);
        set_balance(&env, &admin, initial_supply);
        set_total_supply(&env, initial_supply);
        events::mint(&env, &admin, &admin, initial_supply);
    }

    /// Return the token name (SEP-41).
    pub fn name(env: Env) -> String {
        get_name(&env)
    }

    /// Return the token symbol (SEP-41).
    pub fn symbol(env: Env) -> String {
        get_symbol(&env)
    }

    /// Return the number of decimals used by the token (SEP-41).
    pub fn decimals(_env: Env) -> u32 {
        DECIMALS
    }

    /// Return the amount `spender` may transfer on behalf of `owner`.
    ///
    /// # Returns
    /// Remaining allowance as `i128`; 0 if none has been set.
    pub fn allowance(env: Env, owner: Address, spender: Address) -> i128 {
        allowance(&env, &owner, &spender)
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
        set_balance(&env, &to, balance_of(&env, &to) + amount);
        events::transfer(&env, &from, &to, amount);
    }

    /// Approve `spender` to transfer up to `amount` tokens on behalf of `owner`.
    ///
    /// Overwrites any existing allowance. Set `amount` to 0 to revoke.
    ///
    /// # Parameters
    /// - `owner` — token owner (requires auth)
    /// - `spender` — address being approved
    /// - `amount` — new allowance
    pub fn approve(env: Env, owner: Address, spender: Address, amount: i128) {
        owner.require_auth();
        set_allowance(&env, &owner, &spender, amount);
        events::approve(&env, &owner, &spender, amount);
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
    /// - Panics if `spender`'s allowance for `from` is insufficient
    /// - Panics if `from` has insufficient balance
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        spender.require_auth();
        let allowed = allowance(&env, &from, &spender);
        assert!(allowed >= amount, "allowance exceeded");
        let from_bal = balance_of(&env, &from);
        assert!(from_bal >= amount, "insufficient balance");
        set_allowance(&env, &from, &spender, allowed - amount);
        set_balance(&env, &from, from_bal - amount);
        set_balance(&env, &to, balance_of(&env, &to) + amount);
        events::transfer(&env, &from, &to, amount);
    }

    /// Mint `amount` new tokens to `to`, increasing total supply.
    ///
    /// Only the admin may call this function.
    ///
    /// # Parameters
    /// - `admin` — must match the stored admin (requires auth)
    /// - `to` — recipient of minted tokens
    /// - `amount` — number of tokens to mint (must be > 0)
    ///
    /// # Errors
    /// - Panics if `admin` auth fails or does not match stored admin
    /// - Panics if `amount` ≤ 0
    pub fn mint(env: Env, admin: Address, to: Address, amount: i128) {
        admin.require_auth();
        assert_eq!(get_admin(&env), admin, "not admin");
        assert!(amount > 0, "amount must be positive");
        set_balance(&env, &to, balance_of(&env, &to) + amount);
        set_total_supply(&env, total_supply(&env) + amount);
        events::mint(&env, &admin, &to, amount);
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
        events::burn(&env, &from, amount);
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
    /// - Panics if `spender`'s allowance for `from` is insufficient
    /// - Panics if `from` has insufficient balance
    pub fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        assert!(amount > 0, "amount must be positive");
        let allowed = allowance(&env, &from, &spender);
        assert!(allowed >= amount, "allowance exceeded");
        let bal = balance_of(&env, &from);
        assert!(bal >= amount, "insufficient balance");
        set_allowance(&env, &from, &spender, allowed - amount);
        set_balance(&env, &from, bal - amount);
        set_total_supply(&env, total_supply(&env) - amount);
        events::burn(&env, &from, amount);
    }
}
