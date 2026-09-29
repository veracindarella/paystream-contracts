// SPDX-License-Identifier: Apache-2.0

use soroban_sdk::{contracttype, Address};

#[contracttype]
pub enum TokenDataKey {
    Balance(Address),
    Allowance(Address, Address),
    TotalSupply,
    Admin,
    PendingAdmin,
    AdminNonce,
}
