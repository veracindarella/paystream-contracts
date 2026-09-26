// SPDX-License-Identifier: Apache-2.0

use crate::types::{
    ERR_AMOUNT_NOT_POSITIVE, ERR_BELOW_MIN_DEPOSIT, ERR_INVALID_CLIFF, ERR_INVALID_RATE,
    ERR_STOP_TIME_PAST, ERR_ZERO_DEPOSIT, ERR_ZERO_RATE,
};
use soroban_sdk::Address;

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
    }
    assert!(employer != employee, "employer and employee must differ");
}

/// Validate a top-up amount.
pub fn validate_top_up(amount: i128) {
    assert!(amount > 0, "{}", ERR_AMOUNT_NOT_POSITIVE);
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

/// Validate a vesting `cliff_time` (0 = no cliff).
///
/// - E026 if `cliff_time` is in the past or after a non-zero `stop_time`
pub fn validate_cliff(cliff_time: u64, stop_time: u64, now: u64) {
    if cliff_time > 0 {
        assert!(cliff_time >= now, "{}", ERR_INVALID_CLIFF);
        if stop_time > 0 {
            assert!(cliff_time <= stop_time, "{}", ERR_INVALID_CLIFF);
        }
    }
}
