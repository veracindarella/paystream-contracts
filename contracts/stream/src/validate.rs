// SPDX-License-Identifier: Apache-2.0

#![deny(missing_docs)]

use crate::types::{
    ERR_AMOUNT_NOT_POSITIVE, ERR_BELOW_MIN_DEPOSIT, ERR_CLIFF_AFTER_STOP, ERR_INVALID_RATE, ERR_STOP_TIME_PAST,
    ERR_ZERO_DEPOSIT, ERR_ZERO_RATE, ERR_STOP_TIME_TOO_FAR, MAX_STOP_TIME_OFFSET,
};
use soroban_sdk::{Address, Map, Symbol};

/// Maximum allowed rate_per_second (1 billion tokens/s — prevents overflow in
/// claimable_amount for any realistic elapsed time up to ~292 years).
pub const MAX_RATE_PER_SECOND: i128 = 1_000_000_000_i128;

/// Validate stream creation parameters.
///
/// # Panics
/// - E002 if `deposit` ≤ 0
/// - E007 if `deposit` < `min_deposit`
/// - E001 if `rate_per_second` ≤ 0
/// - E008 if `rate_per_second` > MAX_RATE_PER_SECOND
/// - E022 if `stop_time` is non-zero and in the past
/// - E030 if `stop_time` exceeds 10-year window from now
/// - if `employer` == `employee`
pub fn validate_create_stream(
    deposit: i128,
    min_deposit: i128,
    rate_per_second: i128,
    stop_time: u64,
    now: u64,
    employer: &Address,
    employee: &Address,
) {
    assert!(deposit > 0, "{}", ERR_ZERO_DEPOSIT);
    assert!(deposit >= min_deposit, "{}", ERR_BELOW_MIN_DEPOSIT);
    assert!(rate_per_second > 0, "{}", ERR_ZERO_RATE);
    assert!(
        rate_per_second <= MAX_RATE_PER_SECOND,
        "{}",
        ERR_INVALID_RATE
    );
    if stop_time > 0 {
        assert!(stop_time > now, "{}", ERR_STOP_TIME_PAST);
        // Enforce maximum 10-year window from now
        assert!(
            stop_time <= now.saturating_add(MAX_STOP_TIME_OFFSET),
            "{}",
            ERR_STOP_TIME_TOO_FAR
        );
    }
    assert!(employer != employee, "employer and employee must differ");
}

/// Validate an optional cliff against the stream's stop time (PROD-17).
///
/// # Panics
/// - E032 if `stop_time` is non-zero and `cliff_time > stop_time`
pub fn validate_cliff(cliff_time: Option<u64>, stop_time: u64) {
    if let Some(cliff) = cliff_time {
        if stop_time > 0 {
            assert!(cliff <= stop_time, "{}", ERR_CLIFF_AFTER_STOP);
        }
    }
}

/// Validate a top-up amount.
pub fn validate_top_up(amount: i128) {
    assert!(amount > 0, "{}", ERR_AMOUNT_NOT_POSITIVE);
}

/// Maximum number of entries allowed in a stream metadata map (#467).
pub const MAX_METADATA_ENTRIES: u32 = 8;

/// Maximum length (in bytes) of a metadata key symbol (#467).
pub const MAX_METADATA_KEY_LEN: u32 = 32;

/// Maximum length (in bytes) of a metadata value symbol (#467).
pub const MAX_METADATA_VALUE_LEN: u32 = 64;

/// Validate a stream metadata map (#467).
///
/// # Panics
/// - E040 if the map has more than `MAX_METADATA_ENTRIES` entries
/// - E041 if any key exceeds `MAX_METADATA_KEY_LEN` bytes
/// - E042 if any value exceeds `MAX_METADATA_VALUE_LEN` bytes
pub fn validate_metadata(metadata: &Map<Symbol, Symbol>) {
    use crate::types::{
        ERR_METADATA_KEY_TOO_LONG, ERR_METADATA_TOO_MANY, ERR_METADATA_VALUE_TOO_LONG,
    };

    assert!(
        metadata.len() <= MAX_METADATA_ENTRIES,
        "{}",
        ERR_METADATA_TOO_MANY
    );
    for (key, value) in metadata.iter() {
        assert!(
            key.len() <= MAX_METADATA_KEY_LEN,
            "{}",
            ERR_METADATA_KEY_TOO_LONG
        );
        assert!(
            value.len() <= MAX_METADATA_VALUE_LEN,
            "{}",
            ERR_METADATA_VALUE_TOO_LONG
        );
    }
}

/// Validate a configurable max batch size (#483).
///
/// # Panics
/// - E043 if `max` is outside the allowed range `1..=200`
pub fn validate_max_batch_size(max: u32) {
    use crate::types::{ERR_MAX_BATCH_SIZE_INVALID, MAX_BATCH_SIZE_LOWER, MAX_BATCH_SIZE_UPPER};

    assert!(
        max >= MAX_BATCH_SIZE_LOWER && max <= MAX_BATCH_SIZE_UPPER,
        "{}",
        ERR_MAX_BATCH_SIZE_INVALID
    );
}

/// Validate a new rate passed to `update_rate`.
///
/// Applies the same rules as `create_stream`:
/// - E001 if `rate` ≤ 0
/// - E008 if `rate` > MAX_RATE_PER_SECOND
pub fn validate_rate(rate: i128) {
    assert!(rate > 0, "{}", ERR_ZERO_RATE);
    assert!(rate <= MAX_RATE_PER_SECOND, "{}", ERR_INVALID_RATE);
}

/// Validate split stream beneficiary configuration.
///
/// # Panics
/// - E031 if fewer than 2 beneficiaries
/// - E032 if more than 20 beneficiaries
/// - E030 if basis points do not sum to exactly 10000 (100%)
pub fn validate_split_beneficiaries(
    beneficiaries: &soroban_sdk::Vec<crate::types::SplitBeneficiary>,
) {
    use crate::types::{
        ERR_SPLIT_BASIS_INVALID, ERR_SPLIT_TOO_FEW, ERR_SPLIT_TOO_MANY, MAX_SPLIT_BENEFICIARIES,
    };

    let count = beneficiaries.len();
    assert!(count >= 2, "{}", ERR_SPLIT_TOO_FEW);
    assert!(count <= MAX_SPLIT_BENEFICIARIES, "{}", ERR_SPLIT_TOO_MANY);
    
    let mut total_bp: u64 = 0;
    for benef in beneficiaries.iter() {
        total_bp += benef.basis_points as u64;
    }
    assert!(total_bp == 10000, "{}", ERR_SPLIT_BASIS_INVALID);
}
