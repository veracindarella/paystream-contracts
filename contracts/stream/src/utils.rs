// SPDX-License-Identifier: Apache-2.0

#![deny(missing_docs)]

use soroban_sdk::{Address, Bytes, Env};

/// Get the native XLM SAC (Stellar Asset Contract) address.
///
/// Native XLM on Stellar is wrapped as a SAC with a specific address derived
/// from the native asset ID. This utility looks up the native XLM SAC address
/// programmatically without hardcoding it, making it portable across networks.
///
/// # Parameters
/// - `env` — Soroban environment
///
/// # Returns
/// The `Address` of the native XLM SAC contract.
///
/// # Example
/// ```ignore
/// let native_xlm = get_native_xlm_sac(&env);
/// let token_client = token::Client::new(&env, &native_xlm);
/// ```
pub fn get_native_xlm_sac(env: &Env) -> Address {
    // XDR of `Asset::Native` is the 4-byte discriminant 0. The SAC address is
    // derived deterministically from it and the network passphrase.
    let native_asset_xdr = Bytes::from_array(env, &[0u8, 0, 0, 0]);
    env.deployer()
        .with_stellar_asset(native_asset_xdr)
        .deployed_address()
}

/// Compute the amount to refund to the employer when a stream is cancelled.
///
/// The refund is exactly `deposit - total_withdrawn`, i.e. the entire remaining
/// balance of the stream. Because integer accrual during withdrawals may leave
/// tiny rounding dust in the contract, this helper deliberately refunds the
/// full remainder rather than a recomputed accrual amount: no dust is retained
/// by the contract and no funds earned by the employee are ever locked.
///
/// # Parameters
/// - `deposit` — the original amount deposited by the employer
/// - `total_withdrawn` — the cumulative amount already withdrawn by the employee
///
/// # Returns
/// The remaining balance to refund to the employer.
///
/// # Panics
/// Panics if `total_withdrawn` exceeds `deposit`, which would indicate corrupt
/// accounting state.
pub fn compute_cancel_refund(deposit: i128, total_withdrawn: i128) -> i128 {
    deposit
        .checked_sub(total_withdrawn)
        .expect("withdrawn exceeds deposit")
}

#[cfg(test)]
mod tests {
    use soroban_sdk::Env;

    use super::*;

    #[test]
    fn test_get_native_xlm_sac_returns_valid_address() {
        let env = Env::default();
        let native_xlm = get_native_xlm_sac(&env);
        // Verify it returns an Address (any valid address from StellarAssetClient::native)
        // The function should not panic and return a valid contract address.
        // On-chain, this would resolve to the actual native XLM SAC address.
        let _client = soroban_sdk::token::Client::new(&env, &native_xlm);
        // If we got here, the address is valid and usable
    }

    #[test]
    fn test_cancel_refund_full_when_nothing_withdrawn() {
        assert_eq!(compute_cancel_refund(1_000, 0), 1_000);
    }

    #[test]
    fn test_cancel_refund_zero_when_fully_withdrawn() {
        assert_eq!(compute_cancel_refund(1_000, 1_000), 0);
    }

    #[test]
    fn test_cancel_refund_includes_rounding_dust() {
        // Employee withdrew a rounded-down amount; the 1-unit dust must be
        // refunded to the employer rather than locked in the contract.
        assert_eq!(compute_cancel_refund(1_000, 999), 1);
    }

    #[test]
    fn test_cancel_refund_handles_large_values() {
        let deposit = i128::MAX;
        let withdrawn = i128::MAX - 7;
        assert_eq!(compute_cancel_refund(deposit, withdrawn), 7);
    }

    #[test]
    #[should_panic(expected = "withdrawn exceeds deposit")]
    fn test_cancel_refund_panics_on_invalid_state() {
        compute_cancel_refund(100, 101);
    }
}
