// SPDX-License-Identifier: Apache-2.0

use soroban_sdk::{contracttype, Address, BytesN};

#[contracttype]
pub enum TokenDataKey {
    Balance(Address),
    Allowance(Address, Address),
    TotalSupply,
    Admin,
    PendingAdmin,
    AdminNonce,
    Paused,
    // #495 TOK-29: multi-sig admin configuration (SEC-01 alignment)
    AdminSigners,
    AdminThreshold,
    // #496 TOK-30: pending emergency drain proposal (SEC-03 alignment)
    PendingDrain,
}

/// Multi-sig admin configuration for the token contract.
///
/// Aligns token admin operations with the stream contract's mainnet
/// multi-sig requirement (SEC-01). On mainnet, admin operations must be
/// approved by at least `threshold` of the configured `signers`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminConfig {
    /// Ordered set of authorized admin signers.
    pub signers: soroban_sdk::Vec<Address>,
    /// Minimum number of signer approvals required to execute an admin op.
    pub threshold: u32,
}

/// A pending emergency drain proposal.
///
/// Mirrors the stream contract's SEC-03 emergency drain: a drain must be
/// proposed by the admin multi-sig, then executed only after the configured
/// time-lock has elapsed. The `nonce` binds the proposal to the admin
/// approval flow so stale or replayed proposals cannot be executed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingDrain {
    /// Recipient of the drained funds.
    pub recipient: Address,
    /// Admin nonce at proposal time; used to invalidate stale proposals.
    pub nonce: u64,
    /// Ledger timestamp (seconds) at which the proposal was created.
    pub proposed_at: u64,
}