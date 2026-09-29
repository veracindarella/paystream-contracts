// SPDX-License-Identifier: Apache-2.0

use soroban_sdk::{contracttype, Address};

/// Status of a salary stream.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum StreamStatus {
    Active,
    Paused,
    Cancelled,
    Exhausted,
}

/// A salary stream: employer deposits funds, employee withdraws per-second.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Stream {
    pub id: u64,
    pub employer: Address,
    pub employee: Address,
    pub token: Address,        // SAC token contract address
    pub deposit: i128,         // total deposited amount
    pub withdrawn: i128,       // total already withdrawn
    pub rate_per_second: i128, // tokens streamed per second
    pub start_time: u64,       // ledger timestamp when stream started
    pub stop_time: u64,        // 0 = no end, else hard stop timestamp
    pub last_withdraw_time: u64,
    pub status: StreamStatus,
    /// Tokens accrued at the previous rate and not yet withdrawn.
    /// Populated by `update_rate` before changing `rate_per_second` so that
    /// the employee can still claim earnings from before the rate change.
    /// Cleared (decremented) when `withdraw` pays it out.
    pub pending_accrual: i128,
    /// Reentrancy guard: true while a withdraw cross-contract call is in flight.
    /// Soroban executes contracts atomically within a single transaction, so
    /// cross-contract callbacks cannot interleave with the current frame.
    /// This flag is kept as a defence-in-depth measure and documents the
    /// analysis: no reentrant path exists in the current call graph because
    /// `token::transfer` is a leaf call that cannot call back into this
    /// contract.  If a future upgrade introduces a callback hook the guard
    /// will catch it.
    pub locked: bool,
    /// Vesting cliff timestamp; nothing accrues as claimable before it (0 = no cliff).
    pub cliff_time: u64,
    /// Milestone-unlocked amount awaiting withdrawal, tracked separately from
    /// time-based accrual.
    pub unlocked: i128,
}

/// Parameters for a single stream in a batch create call.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamParams {
    pub employee: Address,
    pub token: Address,
    pub deposit: i128,
    pub rate_per_second: i128,
    pub stop_time: u64,
}

/// Storage keys.
#[contracttype]
pub enum DataKey {
    Stream(u64),
    StreamCount,
    Admin,
    /// Minimum deposit enforced on create_stream.
    MinDeposit,
    /// Monotonically-increasing nonce for admin operations (replay protection).
    AdminNonce,
    /// Contract-wide pause flag.
    Paused,
    /// Pending admin for two-step admin transfer.
    PendingAdmin,
    /// Nonce bound to the pending admin proposal (front-running protection).
    /// Cleared together with PendingAdmin when accept_admin completes.
    PendingAdminNonce,
    /// Index: employer address → Vec<u64> of stream IDs they own.
    EmployerStreams(Address),
    /// Index: employee address → Vec<u64> of stream IDs paying them.
    EmployeeStreams(Address),
    /// Contract version written by migrate() for off-chain upgrade verification.
    Version,
    /// Pending emergency drain proposal: stores (recipient: Address, nonce: u64).
    /// Set by propose_emergency_drain; cleared by emergency_drain after execution.
    /// See SEC-03 / issue #32.
    PendingDrain,
    /// Time-based index: day-bucket (unix_timestamp / 86400) → Vec<u64> of stream IDs
    /// created within that day. Enables efficient time-range queries without a full scan.
    StreamsByTimestamp(u64),
}

/// Contract error codes – panic messages reference these names so callers can
/// match on a stable string.
///
/// | Code | Constant                | Meaning                                            |
/// |------|-------------------------|----------------------------------------------------|
/// | E001 | ERR_ZERO_RATE           | `rate_per_second` must be > 0                      |
/// | E002 | ERR_ZERO_DEPOSIT        | `deposit` must be > 0                              |
/// | E003 | ERR_REENTRANT           | Reentrant withdraw detected                        |
/// | E004 | ERR_OVERFLOW            | Arithmetic overflow in claimable calculation       |
/// | E005 | ERR_STREAM_CANCELLED    | Cannot top up a cancelled stream                   |
/// | E006 | ERR_STREAM_EXHAUSTED    | Cannot top up an exhausted stream                  |
/// | E007 | ERR_BELOW_MIN_DEPOSIT   | Deposit below minimum                              |
/// | E008 | ERR_INVALID_RATE        | `rate_per_second` exceeds maximum                  |
/// | E009 | ERR_BAD_NONCE           | Invalid admin nonce                                |
/// | E010 | ERR_NO_PENDING_ADMIN    | No pending admin set                               |
/// | E011 | ERR_NOT_PENDING_ADMIN   | Caller does not match the pending admin            |
/// | E012 | ERR_NOT_ADMIN           | Caller is not the contract admin                   |
/// | E013 | ERR_CONTRACT_PAUSED     | Contract is paused                                 |
/// | E014 | ERR_EMPTY_PARAMS        | Batch params list must not be empty                |
/// | E015 | ERR_STREAM_NOT_FOUND    | Stream ID does not exist                           |
/// | E016 | ERR_NOT_EMPLOYEE        | Caller is not the stream's employee                |
/// | E017 | ERR_STREAM_NOT_ACTIVE   | Stream is not in Active or Exhausted status        |
/// | E018 | ERR_NOT_EMPLOYER        | Caller is not the stream's employer                |
/// | E019 | ERR_STREAM_NOT_PAUSED   | Stream is not in Paused status                     |
/// | E020 | ERR_STREAM_ALREADY_ENDED| Stream is already Cancelled or Exhausted           |
/// | E021 | ERR_ADMIN_NOT_SET       | Admin has not been initialised                     |
/// | E022 | ERR_STOP_TIME_PAST      | `stop_time` must be in the future                  |
/// | E023 | ERR_AMOUNT_NOT_POSITIVE | Amount must be positive                            |
/// | E025 | ERR_FEE_TOO_HIGH        | `fee_bps` exceeds MAX_PROTOCOL_FEE_BPS             |
/// | E026 | ERR_INVALID_CLIFF       | `cliff_time` not between now and `stop_time`      |
/// | E027 | ERR_MILESTONE_EXCEEDS   | Milestone exceeds the remaining deposit            |
pub const ERR_ZERO_RATE: &str = "E001: rate_per_second must be greater than zero";
pub const ERR_ZERO_DEPOSIT: &str = "E002: deposit must be positive";
pub const ERR_REENTRANT: &str = "E003: reentrant withdraw detected";
pub const ERR_OVERFLOW: &str = "E004: arithmetic overflow in claimable calculation";
pub const ERR_STREAM_CANCELLED: &str = "E005: cannot top up a cancelled stream";
pub const ERR_STREAM_EXHAUSTED: &str = "E006: cannot top up an exhausted stream";
pub const ERR_BELOW_MIN_DEPOSIT: &str = "E007: deposit below minimum";
pub const ERR_INVALID_RATE: &str = "E008: rate_per_second exceeds maximum";
pub const ERR_BAD_NONCE: &str = "E009: invalid admin nonce";
pub const ERR_NO_PENDING_ADMIN: &str = "E010: no pending admin set";
pub const ERR_NOT_PENDING_ADMIN: &str = "E011: not the pending admin";
pub const ERR_NOT_ADMIN: &str = "E012: caller is not the contract admin";
pub const ERR_CONTRACT_PAUSED: &str = "E013: contract is paused";
pub const ERR_EMPTY_PARAMS: &str = "E014: batch params list must not be empty";
pub const ERR_STREAM_NOT_FOUND: &str = "E015: stream not found";
pub const ERR_NOT_EMPLOYEE: &str = "E016: caller is not the stream employee";
pub const ERR_STREAM_NOT_ACTIVE: &str = "E017: stream is not active";
pub const ERR_NOT_EMPLOYER: &str = "E018: caller is not the stream employer";
pub const ERR_STREAM_NOT_PAUSED: &str = "E019: stream is not paused";
pub const ERR_STREAM_ALREADY_ENDED: &str = "E020: stream already ended";
pub const ERR_ADMIN_NOT_SET: &str = "E021: admin has not been initialised";
pub const ERR_STOP_TIME_PAST: &str = "E022: stop_time must be in the future";
pub const ERR_AMOUNT_NOT_POSITIVE: &str = "E023: amount must be positive";
pub const ERR_BAD_PENDING_NONCE: &str = "E024: invalid pending admin nonce";
/// E025: no pending upgrade proposal exists.
pub const ERR_NO_PENDING_UPGRADE: &str = "E025: no pending upgrade proposal";
/// E026: emergency_drain requires the contract to be hard-paused first (SEC-03 / #32).
pub const ERR_DRAIN_NOT_PAUSED: &str = "E026: contract must be paused before emergency drain";
/// E027: no pending emergency drain proposal exists (SEC-03 / #32).
pub const ERR_NO_PENDING_DRAIN: &str = "E027: no pending emergency drain proposal";
/// E028: new_employer must differ from the stream's employee.
pub const ERR_NEW_EMPLOYER_IS_EMPLOYEE: &str = "E028: new_employer must differ from the stream employee";
