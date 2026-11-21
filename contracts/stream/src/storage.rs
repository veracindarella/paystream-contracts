use soroban_sdk::{contracttype, Address, Env};

/// Minimum delay between proposing and executing an emergency drain (24 hours).
pub const DRAIN_TIMELOCK_SECONDS: u64 = 86_400;

/// Error returned when an emergency drain is attempted before the time-lock elapses.
pub const ERR_DRAIN_TIMELOCK_ACTIVE: u32 = 0x1001;

/// Error returned when a WASM upgrade would downgrade or re-apply the current version.
pub const ERR_VERSION_DOWNGRADE: u32 = 0x1002;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmergencyDrainProposal {
    pub recipient: Address,
    pub proposed_at: u64,
}

const DRAIN_PROPOSAL_KEY: &str = "drain_proposal";

pub fn set_drain_proposal(env: &Env, proposal: &EmergencyDrainProposal) {
    env.storage().instance().set(&DRAIN_PROPOSAL_KEY, proposal);
}

pub fn get_drain_proposal(env: &Env) -> Option<EmergencyDrainProposal> {
    env.storage().instance().get(&DRAIN_PROPOSAL_KEY)
}

pub fn clear_drain_proposal(env: &Env) {
    env.storage().instance().remove(&DRAIN_PROPOSAL_KEY);
}

/// Returns true when at least `DRAIN_TIMELOCK_SECONDS` have elapsed since the proposal.
pub fn drain_timelock_elapsed(env: &Env, proposal: &EmergencyDrainProposal) -> bool {
    let now = env.ledger().timestamp();
    now.saturating_sub(proposal.proposed_at) >= DRAIN_TIMELOCK_SECONDS
}

/// Returns the optional lifetime-withdrawable cap for a stream, if set.
pub fn get_max_withdrawable(env: &Env, stream_id: u64) -> Option<i128> {
    env.storage()
        .persistent()
        .get(&crate::DataKey::MaxWithdrawable(stream_id))
}

/// Stores the optional lifetime-withdrawable cap for a stream.
pub fn set_max_withdrawable(env: &Env, stream_id: u64, max_withdrawable: Option<i128>) {
    match max_withdrawable {
        Some(cap) => env
            .storage()
            .persistent()
            .set(&crate::DataKey::MaxWithdrawable(stream_id), &cap),
        None => env
            .storage()
            .persistent()
            .remove(&crate::DataKey::MaxWithdrawable(stream_id)),
    }
}

/// Removes the stored cap for a stream (used on cancel).
pub fn clear_max_withdrawable(env: &Env, stream_id: u64) {
    env.storage()
        .persistent()
        .remove(&crate::DataKey::MaxWithdrawable(stream_id));
}

/// Returns the remaining withdrawable amount under the optional cap, given the
/// amount already withdrawn. Returns `None` when no cap is set.
pub fn remaining_cap(env: &Env, stream_id: u64, withdrawn: i128) -> Option<i128> {
    get_max_withdrawable(env, stream_id).map(|cap| cap.saturating_sub(withdrawn).max(0))
}

/// Caps `claimable` by the remaining lifetime-withdrawable allowance.
/// Returns 0 once the cap has been reached.
pub fn apply_withdrawable_cap(
    env: &Env,
    stream_id: u64,
    withdrawn: i128,
    claimable: i128,
) -> i128 {
    match remaining_cap(env, stream_id, withdrawn) {
        Some(remaining) => claimable.min(remaining).max(0),
        None => claimable,
    }
}

/// Returns the unused portion of the cap for refund on cancel, i.e. the
/// remaining allowance that was never withdrawn. Returns `None` when no cap
/// is set (nothing to refund).
pub fn unused_cap(env: &Env, stream_id: u64, withdrawn: i128) -> Option<i128> {
    remaining_cap(env, stream_id, withdrawn)
}

/// Panics with `ERR_VERSION_DOWNGRADE` unless `new_version` is strictly greater
/// than the stored `DataKey::Version`. Re-applying the same version is rejected.
pub fn assert_version_upgrade(env: &Env, new_version: u32) {
    let stored: u32 = env
        .storage()
        .instance()
        .get(&crate::DataKey::Version)
        .unwrap_or(0);
    if new_version <= stored {
        panic!("{}", ERR_VERSION_DOWNGRADE);
    }
}

#[cfg(test)]
mod version_tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    #[should_panic]
    fn downgrade_attempt_panics() {
        let env = Env::default();
        env.storage().instance().set(&crate::DataKey::Version, &2u32);
        assert_version_upgrade(&env, 1);
    }

    #[test]
    #[should_panic]
    fn same_version_upgrade_panics() {
        let env = Env::default();
        env.storage().instance().set(&crate::DataKey::Version, &2u32);
        assert_version_upgrade(&env, 2);
    }

    #[test]
    fn higher_version_upgrade_allowed() {
        let env = Env::default();
        env.storage().instance().set(&crate::DataKey::Version, &2u32);
        assert_version_upgrade(&env, 3);
    }
}
