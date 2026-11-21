// SPDX-License-Identifier: Apache-2.0

#![deny(missing_docs)]

use crate::types::{
    Error, MAX_STOP_TIME_OFFSET,
};
use soroban_sdk::Address;

/// Maximum allowed rate_per_second (1 billion tokens/s — prevents overflow in
/// claimable_amount for any realistic elapsed time up to ~292 years).
pub const MAX_RATE_PER_SECOND: i128 = 1_000_000_000_i128;

/// Validate stream creation parameters.
///
/// # Errors
/// - [`Error::ZeroDeposit`] if `deposit` ≤ 0
/// - [`Error::BelowMinDeposit`] if `deposit` < `min_deposit`
/// - [`Error::ZeroRate`] if `rate_per_second` ≤ 0
/// - [`Error::InvalidRate`] if `rate_per_second` > MAX_RATE_PER_SECOND
/// - [`Error::StopTimePast`] if `stop_time` is non-zero and in the past
/// - [`Error::StopTimeTooFar`] if `stop_time` exceeds 10-year window from now
/// - [`Error::SameEmployerEmployee`] if `employer` == `employee`
pub fn validate_create_stream(
    deposit: i128,
    min_deposit: i128,
    rate_per_second: i128,
    stop_time: u64,
    now: u64,
    employer: &Address,
    employee: &Address,
) -> Result<(), Error> {
    if deposit <= 0 {
        return Err(Error::ZeroDeposit);
    }
    if deposit < min_deposit {
        return Err(Error::BelowMinDeposit);
    }
    if rate_per_second <= 0 {
        return Err(Error::ZeroRate);
    }
    if rate_per_second > MAX_RATE_PER_SECOND {
        return Err(Error::InvalidRate);
    }
    if stop_time > 0 {
        if stop_time <= now {
            return Err(Error::StopTimePast);
        }
        // Enforce maximum 10-year window from now
        if stop_time > now.saturating_add(MAX_STOP_TIME_OFFSET) {
            return Err(Error::StopTimeTooFar);
        }
    }
    if employer == employee {
        return Err(Error::SameEmployerEmployee);
    }
    Ok(())
}

/// Validate an optional cliff against the stream's stop time (PROD-17).
///
/// # Errors
/// - [`Error::CliffAfterStop`] if `stop_time` is non-zero and `cliff_time > stop_time`
pub fn validate_cliff(cliff_time: Option<u64>, stop_time: u64) -> Result<(), Error> {
    if let Some(cliff) = cliff_time {
        if stop_time > 0 && cliff > stop_time {
            return Err(Error::CliffAfterStop);
        }
    }
    Ok(())
}

/// Validate a top-up amount.
///
/// # Errors
/// - [`Error::AmountNotPositive`] if `amount` ≤ 0
pub fn validate_top_up(amount: i128) -> Result<(), Error> {
    if amount <= 0 {
        return Err(Error::AmountNotPositive);
    }
    Ok(())
}

/// Validate a new rate passed to `update_rate`.
///
/// Applies the same rules as `create_stream`:
/// - [`Error::ZeroRate`] if `rate` ≤ 0
/// - [`Error::InvalidRate`] if `rate` > MAX_RATE_PER_SECOND
pub fn validate_rate(rate: i128) -> Result<(), Error> {
    if rate <= 0 {
        return Err(Error::ZeroRate);
    }
    if rate > MAX_RATE_PER_SECOND {
        return Err(Error::InvalidRate);
    }
    Ok(())
}

/// Validate split stream beneficiary configuration.
///
/// # Errors
/// - [`Error::SplitTooFew`] if fewer than 2 beneficiaries
/// - [`Error::SplitTooMany`] if more than 20 beneficiaries
/// - [`Error::SplitBasisInvalid`] if basis points do not sum to exactly 10000 (100%)
pub fn validate_split_beneficiaries(
    beneficiaries: &soroban_sdk::Vec<crate::types::SplitBeneficiary>,
) -> Result<(), Error> {
    use crate::types::MAX_SPLIT_BENEFICIARIES;

    let count = beneficiaries.len();
    if count < 2 {
        return Err(Error::SplitTooFew);
    }
    if count > MAX_SPLIT_BENEFICIARIES {
        return Err(Error::SplitTooMany);
    }

    let mut total_bp: u64 = 0;
    for benef in beneficiaries.iter() {
        total_bp += benef.basis_points as u64;
    }
    if total_bp != 10000 {
        return Err(Error::SplitBasisInvalid);
    }
    Ok(())
}
