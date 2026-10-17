// SPDX-License-Identifier: Apache-2.0

use soroban_sdk::{contracttype, Address};

/// Status of a salary stream.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum StreamStatus {
    /// The stream is currently accruing tokens.
    Active,
    /// The stream is temporarily stopped by its employer.
    Paused,
    /// The stream was cancelled and its remaining balance settled.
    Cancelled,
    /// The stream has no further tokens available to withdraw.
    Exhausted,
}

/// Determines how a stream's grant becomes claimable.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum StreamType {
    /// Tokens become claimable at a fixed per-second rate.
    Payroll,
    /// A fixed grant becomes claimable linearly over a vesting period.
    LinearVest,
}

/// A salary stream: employer deposits funds, employee withdraws per-second.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Stream {
    /// Unique sequential stream identifier.
    pub id: u64,
    /// Address that funded and owns the stream.
    pub employer: Address,
    /// Original employee address and initial receipt holder.
    pub employee: Address,
    /// SAC token contract address.
    pub token: Address,
    /// Net amount held for the stream after any creation fee.
    pub deposit: i128,
    /// Total amount already withdrawn or settled to the employee.
    pub withdrawn: i128,
    /// Payroll rate in tokens per second; zero for linear vest streams.
    pub rate_per_second: i128,
    /// Ledger timestamp when the stream started.
    pub start_time: u64,
    /// Hard stop timestamp, or 0 when there is no hard stop.
    pub stop_time: u64,
    /// Last accrual timestamp, also used to track a linear vest pause start.
    pub last_withdraw_time: u64,
    /// Current lifecycle state.
    pub status: StreamStatus,
    /// Payroll tokens accrued at a previous rate and not yet withdrawn.
    pub pending_accrual: i128,
    /// Whether a withdraw cross-contract call is currently in flight.
    pub locked: bool,
    /// Remaining-deposit threshold for low-balance events; 0 disables them.
    pub low_balance_threshold: i128,
}

/// Vesting terms stored separately so existing persisted `Stream` values remain decodable.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct LinearVestSchedule {
    /// Net grant amount available over the schedule.
    pub total_grant: i128,
    /// Total vesting duration in seconds.
    pub vest_period: u64,
    /// Optional duration before accrued vesting becomes claimable.
    pub cliff: Option<u64>,
}

/// Parameters for a single stream in a batch create call.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StreamParams {
    /// Employee who receives the stream.
    pub employee: Address,
    /// SEP-41 token contract address.
    pub token: Address,
    /// Gross amount to fund this stream.
    pub deposit: i128,
    /// Payroll rate in tokens per second.
    pub rate_per_second: i128,
    /// Hard stop timestamp, or 0 for no hard stop.
    pub stop_time: u64,
}

/// Configuration for recurring stream auto top-up.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AutoTopupConfig {
    /// Remaining balance below which a top-up is triggered.
    pub trigger_threshold: i128,
    /// Amount transferred for each top-up.
    pub topup_amount: i128,
    /// Maximum cumulative amount that may be topped up.
    pub max_total: i128,
    /// Amount already transferred through automatic top-ups.
    pub total_topped_up: i128,
}

/// Protocol fee configuration. Absent configuration means fees are disabled.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolFeeConfig {
    /// Protocol fee in basis points.
    pub fee_bps: u16,
    /// Address that receives protocol fees.
    pub treasury: Address,
}

/// Storage keys.
#[contracttype]
pub enum DataKey {
    /// Persisted stream data keyed by stream ID.
    Stream(u64),
    /// Number of streams created so far.
    StreamCount,
    /// Contract administrator address.
    Admin,
    /// Auto top-up configuration for a stream: stream ID → AutoTopupConfig.
    AutoTopup(u64),
    /// Minimum deposit enforced on create_stream.
    MinDeposit,
    /// Optional protocol fee configuration.
    ProtocolFee,
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
    /// Contract version written by `migrate` for off-chain upgrade verification.
    Version,
    /// Pending emergency drain proposal: stores (recipient: Address, nonce: u64).
    /// Set by propose_emergency_drain; cleared by emergency_drain after execution.
    /// See SEC-03 / issue #32.
    PendingDrain,
    /// Time-based index: day-bucket (unix_timestamp / 86400) → Vec<u64> of stream IDs
    /// created within that day. Enables efficient time-range queries without a full scan.
    StreamsByTimestamp(u64),
    /// Index: token address → Vec<u64> of streams funded with that token.
    TokenStreams(Address),
    /// Non-payroll stream type. Missing keys represent legacy Payroll streams.
    StreamType(u64),
    /// Linear vesting terms for a stream.
    LinearVestSchedule(u64),
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
/// | E025 | ERR_INVALID_AUTO_TOPUP  | Invalid auto top-up parameters                     |
/// | E026 | ERR_NO_AUTO_TOPUP       | No auto top-up configured for the stream           |
/// | E029 | ERR_BATCH_TOO_LARGE     | Batch contains more than 50 streams                |
/// Error returned when a payroll rate is zero.
pub const ERR_ZERO_RATE: &str = "E001: rate_per_second must be greater than zero";
/// Error returned when a deposit is not positive.
pub const ERR_ZERO_DEPOSIT: &str = "E002: deposit must be positive";
/// Error returned when a reentrant withdraw is detected.
pub const ERR_REENTRANT: &str = "E003: reentrant withdraw detected";
/// Error returned when checked arithmetic overflows.
pub const ERR_OVERFLOW: &str = "E004: arithmetic overflow in claimable calculation";
/// Error returned when topping up a cancelled stream.
pub const ERR_STREAM_CANCELLED: &str = "E005: cannot top up a cancelled stream";
/// Error returned when topping up an exhausted stream.
pub const ERR_STREAM_EXHAUSTED: &str = "E006: cannot top up an exhausted stream";
/// Error returned when a deposit is below the configured minimum.
pub const ERR_BELOW_MIN_DEPOSIT: &str = "E007: deposit below minimum";
/// Error returned when a payroll rate exceeds its maximum.
pub const ERR_INVALID_RATE: &str = "E008: rate_per_second exceeds maximum";
/// Error returned when an admin nonce does not match the expected value.
pub const ERR_BAD_NONCE: &str = "E009: invalid admin nonce";
/// Error returned when no pending admin is set.
pub const ERR_NO_PENDING_ADMIN: &str = "E010: no pending admin set";
/// Error returned when the caller is not the pending admin.
pub const ERR_NOT_PENDING_ADMIN: &str = "E011: not the pending admin";
/// Error returned when the caller is not the contract admin.
pub const ERR_NOT_ADMIN: &str = "E012: caller is not the contract admin";
/// Error returned when an operation is blocked by the contract pause state.
pub const ERR_CONTRACT_PAUSED: &str = "E013: contract is paused";
/// Error returned when a required batch is empty.
pub const ERR_EMPTY_PARAMS: &str = "E014: batch params list must not be empty";
/// Error returned when a stream ID does not exist.
pub const ERR_STREAM_NOT_FOUND: &str = "E015: stream not found";
/// Error returned when the caller is not the stream employee or receipt holder.
pub const ERR_NOT_EMPLOYEE: &str = "E016: caller is not the stream employee";
/// Error returned when a stream is not active.
pub const ERR_STREAM_NOT_ACTIVE: &str = "E017: stream is not active";
/// Error returned when the caller is not the stream employer.
pub const ERR_NOT_EMPLOYER: &str = "E018: caller is not the stream employer";
/// Error returned when a stream is not paused.
pub const ERR_STREAM_NOT_PAUSED: &str = "E019: stream is not paused";
/// Error returned when a stream is already cancelled or exhausted.
pub const ERR_STREAM_ALREADY_ENDED: &str = "E020: stream already ended";
/// Error returned when the contract administrator has not been initialized.
pub const ERR_ADMIN_NOT_SET: &str = "E021: admin has not been initialised";
/// Error returned when a configured stop time is not in the future.
pub const ERR_STOP_TIME_PAST: &str = "E022: stop_time must be in the future";
/// Error returned when an amount is not positive.
pub const ERR_AMOUNT_NOT_POSITIVE: &str = "E023: amount must be positive";
/// Error returned when a pending-admin nonce is invalid.
pub const ERR_BAD_PENDING_NONCE: &str = "E024: invalid pending admin nonce";
/// E025: no pending upgrade proposal exists.
pub const ERR_NO_PENDING_UPGRADE: &str = "E025: no pending upgrade proposal";
/// E026: emergency_drain requires the contract to be hard-paused first (SEC-03 / #32).
pub const ERR_DRAIN_NOT_PAUSED: &str = "E026: contract must be paused before emergency drain";
/// E027: no pending emergency drain proposal exists (SEC-03 / #32).
pub const ERR_NO_PENDING_DRAIN: &str = "E027: no pending emergency drain proposal";
/// E028: new_employer must differ from the stream's employee.
pub const ERR_NEW_EMPLOYER_IS_EMPLOYEE: &str = "E028: new_employer must differ from the stream employee";
/// Error returned when a batch exceeds the maximum size.
pub const ERR_BATCH_TOO_LARGE: &str = "E029: batch exceeds the maximum of 50 streams";
/// Maximum number of streams accepted in a batch operation.
pub const MAX_BATCH_SIZE: u32 = 50;
