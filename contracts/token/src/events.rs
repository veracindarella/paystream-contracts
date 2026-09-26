// SPDX-License-Identifier: Apache-2.0

//! SEP-41 token events.

use soroban_sdk::{symbol_short, Address, Env};

pub fn transfer(env: &Env, from: &Address, to: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("transfer"), from.clone(), to.clone()),
        amount,
    );
}

pub fn approve(env: &Env, owner: &Address, spender: &Address, amount: i128) {
    env.events().publish(
        (symbol_short!("approve"), owner.clone(), spender.clone()),
        amount,
    );
}

pub fn mint(env: &Env, admin: &Address, to: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("mint"), admin.clone(), to.clone()), amount);
}

pub fn burn(env: &Env, from: &Address, amount: i128) {
    env.events()
        .publish((symbol_short!("burn"), from.clone()), amount);
}
