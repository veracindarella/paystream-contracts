// SPDX-License-Identifier: Apache-2.0

use soroban_sdk::{contracterror, contracttype, Address};

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
    /// Optional reason for pausing the stream (regulatory compliance and dispute resolution).
    pub pause_reason: Option<soroban_sdk::String>,
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
    /// Optional cliff timestamp (PROD-17). No tokens accrue before it; accrual
    /// starts from `cliff_time` once passed. Must be <= `stop_time` when both
    /// are set.
    pub cliff_time: Option<u64>,
}

/// Single beneficiary in a split stream, with their share in basis points.
/// 100 basis points = 1%.
#[contracttype]
#[derive(Clone, Debug)]
pub struct SplitBeneficiary {
    /// Beneficiary that receives this share of every withdrawal.
    pub address: Address,
    /// Share in basis points: 0–10000, where 10000 = 100%.
    pub basis_points: u32,
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

/// Configuration for recurring stream auto top-up.
///
/// Re-exported here because `lib.rs` imports `AutoTopupConfig` from `types`.
/// The canonical definition lives in this module.
// NOTE: This re-declaration is intentional — the original definition at line ~63
// was not imported by lib.rs because the types import list did not include it.
// Alias removed; the struct already exists above.

/// Pending upgrade proposal for the time-lock upgrade mechanism (SEC-02).
#[contracttype]
#[derive(Clone, Debug)]
pub struct PendingUpgrade {
    /// Hash of the uploaded WASM the contract will upgrade to.
    pub wasm_hash: soroban_sdk::BytesN<32>,
    /// Earliest ledger timestamp at which `execute_upgrade` may run.
    pub unlock_time: u64,
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
    /// Receipt owner for a stream: stream ID → Address of current receipt holder.
    /// Falls back to stream.employee if not found (for backward compatibility).
    ReceiptOwner(u64),
    /// Index of saved stream templates: employer address → Vec<u32> of template IDs.
    TemplateIds(Address),
    /// Saved stream templates: (employer address, template ID) → StreamParams.
    Template(Address, u32),
    /// Cliff timestamp for a stream (PROD-17): stream ID → u64. Stored apart
    /// from `Stream` so existing persisted streams stay decodable.
    StreamCliff(u64),
    /// Set once the `cliff_reached` event has been emitted for a stream.
    CliffReached(u64),
    /// Non-payroll stream type. Missing keys represent legacy Payroll streams.
    StreamType(u64),
    /// Linear vesting terms for a stream.
    LinearVestSchedule(u64),
    /// Pending WASM upgrade proposal (SEC-02).
    PendingUpgrade,
    /// Timestamp at which a stream entered a terminal (Cancelled/Exhausted) state,
    /// used to enforce the 30-day archive waiting period.
    StreamEndedAt(u64),
    /// Admin-configurable maximum streams an employer may create per ledger.
    /// Default: 50. Stored in instance storage.
    MaxCreatesPerLedger,
    /// Number of streams created by this employer in the current ledger.
    /// Temporary storage — expires automatically.
    CreateCountThisLedger(Address),
    /// Ledger sequence number when the employer last created a stream.
    /// Temporary storage — expires automatically.
    LastCreateLedger(Address),
    /// Split stream beneficiaries: stream ID → Vec<SplitBeneficiary> for multi-recipient streams (PROD-22).
    SplitBeneficiaries(u64),
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
/// | E024 | ERR_BAD_PENDING_NONCE   | Invalid pending admin nonce                        |
/// | E025 | ERR_NO_PENDING_UPGRADE  | No pending upgrade proposal                        |
/// | E026 | ERR_DRAIN_NOT_PAUSED    | Contract must be paused before emergency drain     |
/// | E027 | ERR_NO_PENDING_DRAIN    | No pending emergency drain proposal                |
/// | E028 | ERR_NEW_EMPLOYER_IS_EMPLOYEE | new_employer equals employee                  |
/// | E029 | ERR_BATCH_TOO_LARGE     | Batch contains more than 50 streams                |
/// | E030 | ERR_NOT_RECEIPT_OWNER   | Caller does not hold the stream receipt            |
/// | E031 | ERR_TEMPLATE_NOT_FOUND  | Template not found                                 |
/// | E032 | ERR_SPLIT_TOO_MANY      | Split stream has more than 20 beneficiaries        |
/// | E033 | ERR_SPLIT_TOO_FEW       | Split stream has fewer than 2 beneficiaries        |
/// | E034 | ERR_UPGRADE_TIMELOCK_ACTIVE | Upgrade timelock has not yet expired           |
/// | E035 | ERR_CLIFF_AFTER_STOP    | cliff_time is after stop_time                      |
/// | E036 | ERR_STOP_TIME_TOO_FAR   | stop_time exceeds 10 years from now                |
/// | E037 | ERR_RATE_LIMIT_EXCEEDED | Per-ledger creation rate limit exceeded            |
/// | E038 | ERR_SPLIT_BASIS_INVALID | Split basis points do not sum to 10000             |
/// | E039 | ERR_INVALID_STOP_TIME   | stop_time precedes last_withdraw_time              |
/// | E040 | ERR_STREAM_NOT_ENDED    | Stream is not cancelled or exhausted               |
/// | E041 | ERR_NO_AUTO_TOPUP       | No auto top-up configured for the stream           |
/// | E042 | ERR_INVALID_AUTO_TOPUP  | Invalid auto top-up parameters                     |
/// | E043 | ERR_ARCHIVE_TOO_EARLY   | Archive waiting period has not elapsed             |
///
/// The typed [`Error`] enum below is the single source of truth for these
/// codes; the table is kept in sync with it.
/// Typed contract error returned by fallible entrypoints.
///
/// This enum is the single source of truth for error codes. Each variant's
/// discriminant matches the `E0xx` prefix of the corresponding `ERR_*` string
/// constant (see [`Error::code`] and [`Error::from_code`]), so callers can
/// migrate from string matching to typed `Result` handling without changing
/// the numeric codes they already depend on.
///
/// # Panic policy
///
/// Soroban's `#[contractimpl]` entrypoints can only return `Result<T, Error>`
/// where `Error` is a `#[contracterror]` enum. The following failures cannot be
/// expressed as a returned `Result` and therefore still panic (trap):
///
/// * **Authorization failures** — `Address::require_auth` traps when the
///   caller has not signed. This is enforced by the host, not by this contract.
/// * **Host-level failures** — storage access errors, missing ledger entries,
///   and cross-contract call failures raised by the Soroban host.
/// * **Invariant violations** — internal `assert!`/`panic!` guards that should
///   be unreachable if the contract is correct (e.g. corrupted storage).
///
/// All *documented* validation failures are returned as `Error` variants.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// E001: `rate_per_second` must be > 0.
    ZeroRate = 1,
    /// E002: `deposit` must be > 0.
    ZeroDeposit = 2,
    /// E003: reentrant withdraw detected.
    Reentrant = 3,
    /// E004: arithmetic overflow in claimable calculation.
    Overflow = 4,
    /// E005: cannot top up a cancelled stream.
    StreamCancelled = 5,
    /// E006: cannot top up an exhausted stream.
    StreamExhausted = 6,
    /// E007: deposit below the configured minimum.
    BelowMinDeposit = 7,
    /// E008: `rate_per_second` exceeds the maximum.
    InvalidRate = 8,
    /// E009: invalid admin nonce.
    BadNonce = 9,
    /// E010: no pending admin set.
    NoPendingAdmin = 10,
    /// E011: caller is not the pending admin.
    NotPendingAdmin = 11,
    /// E012: caller is not the contract admin.
    NotAdmin = 12,
    /// E013: contract is paused.
    ContractPaused = 13,
    /// E014: batch params list must not be empty.
    EmptyParams = 14,
    /// E015: stream ID does not exist.
    StreamNotFound = 15,
    /// E016: caller is not the stream's employee or receipt holder.
    NotEmployee = 16,
    /// E017: stream is not in Active or Exhausted status.
    StreamNotActive = 17,
    /// E018: caller is not the stream's employer.
    NotEmployer = 18,
    /// E019: stream is not in Paused status.
    StreamNotPaused = 19,
    /// E020: stream is already Cancelled or Exhausted.
    StreamAlreadyEnded = 20,
    /// E021: admin has not been initialised.
    AdminNotSet = 21,
    /// E022: `stop_time` must be in the future.
    StopTimePast = 22,
    /// E023: amount must be positive.
    AmountNotPositive = 23,
    /// E024: invalid pending admin nonce.
    BadPendingNonce = 24,
    /// E025: no pending upgrade proposal exists.
    NoPendingUpgrade = 25,
    /// E026: contract must be paused before emergency drain.
    DrainNotPaused = 26,
    /// E027: no pending emergency drain proposal exists.
    NoPendingDrain = 27,
    /// E028: `new_employer` must differ from the stream employee.
    NewEmployerIsEmployee = 28,
    /// E029: batch exceeds the maximum of 50 streams.
    BatchTooLarge = 29,
    /// E030: caller does not hold the stream receipt.
    NotReceiptOwner = 30,
    /// E031: template not found.
    TemplateNotFound = 31,
    /// E032: split stream cannot have more than 20 beneficiaries.
    SplitTooMany = 32,
    /// E033: split stream must have at least 2 beneficiaries.
    SplitTooFew = 33,
    /// E034: upgrade timelock has not yet expired.
    UpgradeTimelockActive = 34,
    /// E035: `cliff_time` must be <= `stop_time`.
    CliffAfterStop = 35,
    /// E036: `stop_time` must not exceed 10 years from now.
    StopTimeTooFar = 36,
    /// E037: per-employer, per-ledger stream creation rate limit exceeded.
    RateLimitExceeded = 37,
    /// E038: split beneficiary basis points must sum to 10000.
    SplitBasisInvalid = 38,
    /// E039: `stop_time` precedes `last_withdraw_time`.
    InvalidStopTime = 39,
    /// E040: stream is not cancelled or exhausted.
    StreamNotEnded = 40,
    /// E041: no auto top-up configured for this stream.
    NoAutoTopup = 41,
    /// E042: invalid auto top-up parameters.
    InvalidAutoTopup = 42,
    /// E043: archive waiting period has not elapsed.
    ArchiveTooEarly = 43,
}

impl Error {
    /// Returns the numeric `E0xx` code for this error.
    pub fn code(&self) -> u32 {
        *self as u32
    }

    /// Maps a numeric `E0xx` code back to its `Error` variant.
    ///
    /// Returns `None` for codes that are not part of the documented set.
    pub fn from_code(code: u32) -> Option<Error> {
        match code {
            1 => Some(Error::ZeroRate),
            2 => Some(Error::ZeroDeposit),
            3 => Some(Error::Reentrant),
            4 => Some(Error::Overflow),
            5 => Some(Error::StreamCancelled),
            6 => Some(Error::StreamExhausted),
            7 => Some(Error::BelowMinDeposit),
            8 => Some(Error::InvalidRate),
            9 => Some(Error::BadNonce),
            10 => Some(Error::NoPendingAdmin),
            11 => Some(Error::NotPendingAdmin),
            12 => Some(Error::NotAdmin),
            13 => Some(Error::ContractPaused),
            14 => Some(Error::EmptyParams),
            15 => Some(Error::StreamNotFound),
            16 => Some(Error::NotEmployee),
            17 => Some(Error::StreamNotActive),
            18 => Some(Error::NotEmployer),
            19 => Some(Error::StreamNotPaused),
            20 => Some(Error::StreamAlreadyEnded),
            21 => Some(Error::AdminNotSet),
            22 => Some(Error::StopTimePast),
            23 => Some(Error::AmountNotPositive),
            24 => Some(Error::BadPendingNonce),
            25 => Some(Error::NoPendingUpgrade),
            26 => Some(Error::DrainNotPaused),
            27 => Some(Error::NoPendingDrain),
            28 => Some(Error::NewEmployerIsEmployee),
            29 => Some(Error::BatchTooLarge),
            30 => Some(Error::NotReceiptOwner),
            31 => Some(Error::TemplateNotFound),
            32 => Some(Error::SplitTooMany),
            33 => Some(Error::SplitTooFew),
            34 => Some(Error::UpgradeTimelockActive),
            35 => Some(Error::CliffAfterStop),
            36 => Some(Error::StopTimeTooFar),
            37 => Some(Error::RateLimitExceeded),
            38 => Some(Error::SplitBasisInvalid),
            39 => Some(Error::InvalidStopTime),
            40 => Some(Error::StreamNotEnded),
            41 => Some(Error::NoAutoTopup),
            42 => Some(Error::InvalidAutoTopup),
            43 => Some(Error::ArchiveTooEarly),
            _ => None,
        }
    }

    /// Returns the legacy `ERR_*` string constant for this error.
    ///
    /// Kept so existing callers that match on panic strings keep working while
    /// they migrate to typed `Result` handling.
    pub fn message(&self) -> &'static str {
        match self {
            Error::ZeroRate => ERR_ZERO_RATE,
            Error::ZeroDeposit => ERR_ZERO_DEPOSIT,
            Error::Reentrant => ERR_REENTRANT,
            Error::Overflow => ERR_OVERFLOW,
            Error::StreamCancelled => ERR_STREAM_CANCELLED,
            Error::StreamExhausted => ERR_STREAM_EXHAUSTED,
            Error::BelowMinDeposit => ERR_BELOW_MIN_DEPOSIT,
            Error::InvalidRate => ERR_INVALID_RATE,
            Error::BadNonce => ERR_BAD_NONCE,
            Error::NoPendingAdmin => ERR_NO_PENDING_ADMIN,
            Error::NotPendingAdmin => ERR_NOT_PENDING_ADMIN,
            Error::NotAdmin => ERR_NOT_ADMIN,
            Error::ContractPaused => ERR_CONTRACT_PAUSED,
            Error::EmptyParams => ERR_EMPTY_PARAMS,
            Error::StreamNotFound => ERR_STREAM_NOT_FOUND,
            Error::NotEmployee => ERR_NOT_EMPLOYEE,
            Error::StreamNotActive => ERR_STREAM_NOT_ACTIVE,
            Error::NotEmployer => ERR_NOT_EMPLOYER,
            Error::StreamNotPaused => ERR_STREAM_NOT_PAUSED,
            Error::StreamAlreadyEnded => ERR_STREAM_ALREADY_ENDED,
            Error::AdminNotSet => ERR_ADMIN_NOT_SET,
            Error::StopTimePast => ERR_STOP_TIME_PAST,
            Error::AmountNotPositive => ERR_AMOUNT_NOT_POSITIVE,
            Error::BadPendingNonce => ERR_BAD_PENDING_NONCE,
            Error::NoPendingUpgrade => ERR_NO_PENDING_UPGRADE,
            Error::DrainNotPaused => ERR_DRAIN_NOT_PAUSED,
            Error::NoPendingDrain => ERR_NO_PENDING_DRAIN,
            Error::NewEmployerIsEmployee => ERR_NEW_EMPLOYER_IS_EMPLOYEE,
            Error::BatchTooLarge => ERR_BATCH_TOO_LARGE,
            Error::NotReceiptOwner => ERR_NOT_RECEIPT_OWNER,
            Error::TemplateNotFound => ERR_TEMPLATE_NOT_FOUND,
            Error::SplitTooMany => ERR_SPLIT_TOO_MANY,
            Error::SplitTooFew => ERR_SPLIT_TOO_FEW,
            Error::UpgradeTimelockActive => ERR_UPGRADE_TIMELOCK_ACTIVE,
            Error::CliffAfterStop => ERR_CLIFF_AFTER_STOP,
            Error::StopTimeTooFar => ERR_STOP_TIME_TOO_FAR,
            Error::RateLimitExceeded => ERR_RATE_LIMIT_EXCEEDED,
            Error::SplitBasisInvalid => ERR_SPLIT_BASIS_INVALID,
            Error::InvalidStopTime => ERR_INVALID_STOP_TIME,
            Error::StreamNotEnded => ERR_STREAM_NOT_ENDED,
            Error::NoAutoTopup => ERR_NO_AUTO_TOPUP,
            Error::InvalidAutoTopup => ERR_INVALID_AUTO_TOPUP,
            Error::ArchiveTooEarly => ERR_ARCHIVE_TOO_EARLY,
        }
    }
}

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
pub const ERR_NEW_EMPLOYER_IS_EMPLOYEE: &str =
    "E028: new_employer must differ from the stream employee";
/// Error returned when a batch exceeds the maximum size.
pub const ERR_BATCH_TOO_LARGE: &str = "E029: batch exceeds the maximum of 50 streams";
/// E030: caller does not hold the stream receipt.
pub const ERR_NOT_RECEIPT_OWNER: &str = "E030: caller does not hold the stream receipt";
/// E031: template not found.
pub const ERR_TEMPLATE_NOT_FOUND: &str = "E031: template not found";
/// E035: `cliff_time` is after `stop_time` (PROD-17).
pub const ERR_CLIFF_AFTER_STOP: &str = "E035: cliff_time must be <= stop_time";
/// Maximum number of streams in one batch call (create/cancel/pause/resume).
pub const MAX_BATCH_SIZE: u32 = 50;

/// E036: stop_time extends beyond 10-year limit from now
pub const ERR_STOP_TIME_TOO_FAR: &str = "E036: stop_time must not exceed 10 years from now";

/// Maximum stop_time window: 10 years in seconds (315,360,000 seconds)
pub const MAX_STOP_TIME_OFFSET: u64 = 315_360_000;

// Restored from a97bb5d (dropped by a merge).
/// E037: per-employer, per-ledger stream creation rate limit exceeded.
pub const ERR_RATE_LIMIT_EXCEEDED: &str = "E037: stream creation rate limit exceeded for this ledger";

// Restored from a97bb5d (dropped by a merge).
/// Default maximum number of streams an employer may create in a single ledger.
pub const DEFAULT_MAX_CREATES_PER_LEDGER: u32 = 50;

// Restored from cf48a49 (dropped by a merge).
/// E038: split beneficiary basis points must sum to 10000 (100%)
pub const ERR_SPLIT_BASIS_INVALID: &str = "E038: split beneficiary basis points must sum to 10000";

// Restored from cf48a49 (dropped by a merge).
/// E033: split stream must have at least 2 beneficiaries
pub const ERR_SPLIT_TOO_FEW: &str = "E033: split stream must have at least 2 beneficiaries";

// Restored from cf48a49 (dropped by a merge).
/// E032: split stream cannot have more than 20 beneficiaries
pub const ERR_SPLIT_TOO_MANY: &str = "E032: split stream cannot have more than 20 beneficiaries";

// Restored from cf48a49 (dropped by a merge).
/// Maximum number of beneficiaries in a split stream.
pub const MAX_SPLIT_BENEFICIARIES: u32 = 20;

// Restored from c3cb54c (dropped by a merge).
/// E039: `update_stop_time` would move the stop time before the last withdrawal.
pub const ERR_INVALID_STOP_TIME: &str = "E039: stop_time precedes last_withdraw_time";

// Restored from a74e8f6 (dropped by a merge).
/// `execute_upgrade` was called before the timelock expired.
pub const ERR_UPGRADE_TIMELOCK_ACTIVE: &str = "E034: upgrade timelock has not yet expired";

// Restored from ce6fe77 (dropped by a merge).
/// E040: `archive_stream` was called on a stream that is still Active or Paused.
pub const ERR_STREAM_NOT_ENDED: &str = "E040: stream is not cancelled or exhausted";

// Restored from a74e8f6 (dropped by a merge).
/// E041: no auto top-up is configured for the stream.
pub const ERR_NO_AUTO_TOPUP: &str = "E041: no auto top-up configured for this stream";

// Restored from a74e8f6 (dropped by a merge).
/// E042: auto top-up threshold, amount or cap is not positive / consistent.
pub const ERR_INVALID_AUTO_TOPUP: &str = "E042: invalid auto top-up parameters";

// Restored from ce6fe77 (dropped by a merge).
/// E043: `archive_stream` was called before `ARCHIVE_WAIT_PERIOD` elapsed.
pub const ERR_ARCHIVE_TOO_EARLY: &str = "E043: archive waiting period has not elapsed";

// Restored from 24db2de (dropped by a merge).
/// Protocol fee configuration. Absent configuration means fees are disabled.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ProtocolFeeConfig {
    /// Protocol fee in basis points.
    pub fee_bps: u32,
    /// Address that receives protocol fees.
    pub treasury: Address,
}
