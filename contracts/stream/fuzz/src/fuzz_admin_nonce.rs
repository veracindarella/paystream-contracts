// SPDX-License-Identifier: Apache-2.0

//! Fuzz target for admin nonce replay-attack protection (issue #55 / SEC-09).
//!
//! The admin nonce is a monotonically-increasing counter that prevents
//! replaying admin operations.  `consume_admin_nonce(env, nonce)` asserts that
//! `nonce == current_nonce`, then atomically increments the stored value.
//!
//! This target exercises two properties with 500 000 proptest iterations:
//!
//! 1. **Only the expected nonce is accepted.**
//!    For every generated nonce value that is NOT the current expected value,
//!    `consume_admin_nonce` must panic with `E009`.
//!
//! 2. **Strict monotonic increase after each accepted nonce.**
//!    After a successful call with nonce `n`, the stored nonce must equal
//!    `n + 1`.  A second call with `n` (replay) must also panic with `E009`.

#![allow(dead_code)]

use paystream_stream::storage::{consume_admin_nonce, get_admin_nonce};
use proptest::prelude::*;
use soroban_sdk::Env;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500_000))]

    /// Only the currently expected nonce is accepted; everything else panics
    /// with E009.  After acceptance, the nonce increments by exactly 1.
    #[test]
    fn fuzz_admin_nonce_only_expected_accepted(
        // Generate a sequence of up to 8 nonce attempts before the correct one.
        wrong_attempts in prop::collection::vec(0u64..u64::MAX, 0..8),
        // A small starting nonce so we can run a few acceptance cycles.
        start_nonce in 0u64..100u64,
        // How many successful nonce advances to perform (1–4).
        advances in 1u64..=4u64,
    ) {
        let env = Env::default();

        // Seed the nonce counter by consuming nonces 0..start_nonce.
        // This simulates a contract that has already processed `start_nonce`
        // admin operations before our fuzz iteration begins.
        for n in 0..start_nonce {
            consume_admin_nonce(&env, n);
        }

        let mut expected = start_nonce;

        // --- Phase 1: wrong attempts must all panic with E009 ---
        for &wrong in &wrong_attempts {
            if wrong == expected {
                // Skip: this would be the correct nonce, not a replay attack.
                continue;
            }

            // Attempt to consume an incorrect nonce — must panic with E009.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Each catch_unwind needs a fresh Env because the Soroban SDK
                // may leave the environment in an inconsistent state after a
                // panic.  We re-seed the counter in the new env.
                let env2 = Env::default();
                for n in 0..expected {
                    consume_admin_nonce(&env2, n);
                }
                consume_admin_nonce(&env2, wrong);
            }));

            prop_assert!(
                result.is_err(),
                "consume_admin_nonce({wrong}) should panic when expected nonce is {expected}"
            );

            // Verify the panic message contains E009.
            if let Err(e) = result {
                let msg = if let Some(s) = e.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = e.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    String::new()
                };
                prop_assert!(
                    msg.contains("E009"),
                    "panic message must contain E009, got: {msg:?}"
                );
            }
        }

        // --- Phase 2: correct nonce must succeed and increment atomically ---
        for _ in 0..advances {
            // Before consumption: stored nonce == expected.
            let before = get_admin_nonce(&env);
            prop_assert_eq!(
                before, expected,
                "stored nonce must equal expected before consumption"
            );

            // Consume the correct nonce — must NOT panic.
            consume_admin_nonce(&env, expected);

            // After consumption: stored nonce must be exactly expected + 1.
            let after = get_admin_nonce(&env);
            prop_assert_eq!(
                after,
                expected + 1,
                "stored nonce must be exactly expected+1 after consumption"
            );

            // --- Phase 2b: replay of the just-consumed nonce must panic with E009 ---
            let replay_nonce = expected; // already consumed
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let env3 = Env::default();
                // Advance env3 to the post-advance state.
                for n in 0..=expected {
                    consume_admin_nonce(&env3, n);
                }
                // Replay: try to consume the same nonce again.
                consume_admin_nonce(&env3, replay_nonce);
            }));

            prop_assert!(
                result.is_err(),
                "replaying nonce {replay_nonce} must panic"
            );

            expected += 1;
        }

        // --- Phase 3: final nonce value is strictly monotonically increasing ---
        prop_assert_eq!(
            get_admin_nonce(&env),
            expected,
            "final nonce must equal start_nonce + advances"
        );
    }

    /// A stream of purely sequential nonces (0, 1, 2, …) must all succeed and
    /// each increment the counter exactly once.
    #[test]
    fn fuzz_admin_nonce_sequential_always_succeeds(
        count in 1u64..=20u64,
    ) {
        let env = Env::default();

        for n in 0..count {
            prop_assert_eq!(get_admin_nonce(&env), n);
            consume_admin_nonce(&env, n);
            prop_assert_eq!(get_admin_nonce(&env), n + 1);
        }
    }

    /// Any nonce other than the current one must be rejected with E009,
    /// regardless of whether it is less than or greater than the expected value.
    #[test]
    fn fuzz_admin_nonce_non_sequential_rejected(
        current in 0u64..=50u64,
        offset  in 1u64..=u64::MAX / 2,
        add     in proptest::bool::ANY,
    ) {
        // Compute a nonce that is definitely wrong (either current+offset or
        // current.saturating_sub(offset), both != current).
        let wrong = if add {
            current.saturating_add(offset)
        } else {
            current.saturating_sub(offset)
        };

        if wrong == current {
            // Edge case: saturating_sub(0) == 0 == current. Skip.
            return Ok(());
        }

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let env = Env::default();
            // Advance to `current`.
            for n in 0..current {
                consume_admin_nonce(&env, n);
            }
            // Attempt wrong nonce — must panic with E009.
            consume_admin_nonce(&env, wrong);
        }));

        prop_assert!(
            result.is_err(),
            "consume_admin_nonce({wrong}) with expected={current} must panic"
        );
    }
}

fn main() {}
