// SPDX-License-Identifier: Apache-2.0

use soroban_sdk::{contracttype, Address, BytesN, String};

#[contracttype]
pub enum TokenDataKey {
    Balance(Address),
    Allowance(Address, Address),
    TotalSupply,
    Admin,
    PendingAdmin,
    AdminNonce,
    Paused,
    MaxSupply,
    Metadata,
    PendingUpgrade,
    Version,
    /// TOK-33: optional admin-configured per-recipient cap for `mint_to_batch`.
    RecipientCap,
}

/// SEP-41 token metadata (TOK-32): name, symbol and decimals in one struct.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenMetadata {
    pub decimal: u32,
    pub name: String,
    pub symbol: String,
}

/// TOK-18/TOK-10: a scheduled contract WASM upgrade, pending its time-lock.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingUpgrade {
    pub wasm_hash: BytesN<32>,
    pub unlock_time: u64,
}

/// Typed error codes for the token contract.
///
/// Each variant carries a unique, stable numeric code so SDK consumers can
/// match on `Result` values instead of parsing panic strings. The numeric
/// values are the authoritative token error codes referenced by the
/// documentation (DOC-37).
///
/// Panic policy:
/// - Authorization failures (missing/invalid auth, unauthorized admin actions)
///   are surfaced by the Soroban host as `Error(Auth, InvalidAction)` when the
///   `require_auth` call fails. These are intentionally *not* mapped to a
///   `TokenError` variant because the host aborts before contract code can
///   return; callers should treat host auth errors as distinct from the codes
///   below.
/// - All other documented failure cases are returned as `Result<_, TokenError>`
///   from public entrypoints where the SDK supports it.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TokenError {
    /// T001: arithmetic overflow or underflow.
    Overflow = 1,
    /// T002: contract is paused; state-changing operations are disabled.
    Paused = 2,
    /// T003: batch operation exceeds the maximum allowed size.
    BatchTooLarge = 3,
    /// T004: insufficient balance for the requested transfer/burn.
    InsufficientBalance = 4,
    /// T005: insufficient allowance for the requested transfer.
    InsufficientAllowance = 5,
    /// T006: caller is not the current admin.
    NotAdmin = 6,
    /// T007: no pending admin transfer is in progress.
    NoPendingAdmin = 7,
    /// T008: supplied admin nonce does not match the expected nonce.
    InvalidNonce = 8,
    /// T009: contract has already been initialized.
    AlreadyInitialized = 9,
    /// T010: contract has not been initialized.
    NotInitialized = 10,
    /// T011: requested upgrade is not permitted / invalid.
    InvalidUpgrade = 11,
    /// T012: amount must be non-negative / greater than zero.
    InvalidAmount = 12,
    /// T013: address argument is invalid for this operation.
    InvalidAddress = 13,
    /// T014: operation is not supported.
    Unsupported = 14,
}
</｜｜DSML｜｜ parameter>
</｜｜DSML｜｜ invoke>
</｜｜DSML｜｜ calls>
