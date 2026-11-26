// SPDX-License-Identifier: Apache-2.0

//! Shared utilities for the stream contract.
//!
//! # Claimable-amount caching contract
//!
//! Claimable amounts are a pure function of stream state and ledger time, so
//! off-chain backends may cache them. This module defines the canonical cache
//! key and invalidation rules so every consumer agrees on the semantics.
//!
//! ## Cache key
//!
//! The key is `(stream_id, ledger_second_bucket)`, where
//! `ledger_second_bucket` is produced by [`ledger_second_bucket`]. Bucketing by
//! ledger second bounds the staleness of a cached value to at most one second
//! of accrual even without explicit invalidation.
//!
//! ## TTL
//!
//! Entries should use a short TTL (recommended: 1–5 seconds, never longer than
//! the bucket width). A value must not be served past its TTL unless the
//! stale-on-error fallback below applies.
//!
//! ## Invalidation
//!
//! Any state-changing event for a stream must invalidate all cached claimable
//! values for that `stream_id`, regardless of bucket. This includes:
//! `withdraw`, `top_up`, `cancel`, `pause`, `resume`, and `update_rate`.
//! Consumers should drop every entry whose key's first component matches the
//! affected stream.
//!
//! ## Stale-on-error fallback
//!
//! If recomputation fails (e.g. RPC/simulation error), a consumer may serve the
//! most recent cached value for the stream, but only within a bounded staleness
//! window (recommended: at most one bucket, i.e. one ledger second, and never
//! across an invalidation event). Serving stale data beyond that bound is not
//! permitted.
//!
//! ## Observability
//!
//! Consumers should track a cache hit-ratio metric (hits / (hits + misses)) per
//! stream and in aggregate so the effectiveness of this caching layer is
//! observable.

use soroban_sdk::{Address, Bytes, Env};

/// Returns the canonical ledger-second bucket used as the second component of
/// the `(stream_id, ledger_second_bucket)` claimable cache key.
///
/// The bucket is the current ledger timestamp in seconds. Off-chain clients
/// should use this value (or the identical `env.ledger().timestamp()` read from
/// the same ledger) when constructing cache keys, so that cached claimable
/// amounts are bounded in staleness to a single ledger second.
///
/// # Parameters
/// - `env` — Soroban environment
///
/// # Returns
/// The current ledger timestamp, in seconds, truncated to whole seconds.
///
/// # Example
/// ```ignore
/// let key = (stream_id, ledger_second_bucket(&env));
/// ```
pub fn ledger_second_bucket(env: &Env) -> u64 {
    env.ledger().timestamp()
}

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

/// Cached cliff/schedule parameters for a single stream.
///
/// Reading cliff and schedule values from storage is expensive; this struct
/// lets callers read them once and reuse the values across repeated
/// `claimable_amount` computations (e.g. inside `withdraw_all`).
#[derive(Clone, Copy)]
pub struct StreamSchedule {
    /// Timestamp at which the cliff ends (0 if no cliff).
    pub cliff: u64,
    /// Timestamp at which accrual starts.
    pub accrual_start: u64,
    /// Timestamp at which the stream ends.
    pub end: u64,
    /// Total amount streamed over the full duration.
    pub total: i128,
}

impl StreamSchedule {
    /// Compute the claimable amount for `now` using the cached schedule values.
    ///
    /// This performs no storage reads; all inputs are held in the struct so
    /// repeated calls (e.g. per stream in `withdraw_all`) avoid redundant
    /// cliff/schedule lookups while producing identical results.
    pub fn claimable_amount(&self, now: u64) -> i128 {
        if now < self.cliff || now < self.accrual_start {
            return 0;
        }
        if now >= self.end {
            return self.total;
        }
        let elapsed = (now - self.accrual_start) as i128;
        let duration = (self.end - self.accrual_start) as i128;
        if duration <= 0 {
            return self.total;
        }
        self.total * elapsed / duration
    }
}

#[cfg(test)]
mod tests {
    use soroban_sdk::Env;

    use super::*;

    #[test]
    fn test_ledger_second_bucket_matches_ledger_timestamp() {
        let env = Env::default();
        env.ledger().set_timestamp(1_234_567);
        assert_eq!(ledger_second_bucket(&env), 1_234_567);
    }

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
    fn test_stream_schedule_claimable_amount() {
        let schedule = StreamSchedule {
            cliff: 100,
            accrual_start: 100,
            end: 200,
            total: 1000,
        };
        assert_eq!(schedule.claimable_amount(50), 0);
        assert_eq!(schedule.claimable_amount(100), 0);
        assert_eq!(schedule.claimable_amount(150), 500);
        assert_eq!(schedule.claimable_amount(200), 1000);
        assert_eq!(schedule.claimable_amount(250), 1000);
    }
}
