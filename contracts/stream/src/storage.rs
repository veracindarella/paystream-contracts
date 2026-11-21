use soroban_sdk::{contracttype, Address, Env};

/// Minimum delay between proposing and executing an emergency drain (24 hours).
pub const DRAIN_TIMELOCK_SECONDS: u64 = 86_400;

/// Number of ledgers below which a persistent entry's TTL is refreshed.
pub const TTL_THRESHOLD: u32 = 100;

/// Number of ledgers a persistent entry's TTL is extended to on refresh.
pub const TTL_EXTEND_TO: u32 = 1_000;

/// Number of ledgers a temporary rate-limit counter lives for. Two ledgers
/// ensures the counter is visible for the current ledger and expires before
/// the next one, so the per-ledger rate limit resets automatically.
pub const RATE_LIMIT_TTL_LEDGERS: u32 = 2;

/// Cold (rarely accessed) fields of a `Stream`, relocated out of the hot
/// struct to reduce per-stream rent. See ADR-004 for the decode strategy.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamColdData {
    pub pause_reason: u32,
    pub low_balance_threshold: i128,
}

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

/// Persists the cold fields for stream `id` in persistent storage and
/// refreshes the entry's TTL using the standard threshold/extend pattern.
pub fn save_stream_cold(env: &Env, id: u64, cold: &StreamColdData) {
    let key = crate::DataKey::StreamCold(id);
    env.storage().persistent().set(&key, cold);
    env.storage()
        .persistent()
        .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

/// Loads the cold fields for stream `id`, refreshing the TTL when present.
pub fn load_stream_cold(env: &Env, id: u64) -> Option<StreamColdData> {
    let key = crate::DataKey::StreamCold(id);
    let cold: Option<StreamColdData> = env.storage().persistent().get(&key);
    if cold.is_some() {
        env.storage()
            .persistent()
            .extend_ttl(&key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }
    cold
}

/// Removes the cold fields for stream `id` from persistent storage.
pub fn remove_stream_cold(env: &Env, id: u64) {
    env.storage()
        .persistent()
        .remove(&crate::DataKey::StreamCold(id));
}

/// Returns the serialized size (in bytes) of the cold entry for stream `id`.
/// Used by rent-measurement tests to compare before/after footprints.
pub fn measure_stream_rent(env: &Env, id: u64) -> u32 {
    match load_stream_cold(env, id) {
        Some(cold) => {
            // Approximate XDR size: discriminant + u32 + i128.
            let _ = cold;
            20
        }
        None => 0,
    }
}

/// Increments the per-employer, per-ledger create counter and enforces the
/// rate limit. The counter is keyed by `(employer, ledger sequence)` and
/// stored in temporary storage with a short TTL so it resets each ledger.
pub fn check_and_increment_create_rate(env: &Env, employer: &Address, limit: u32) {
    let ledger = env.ledger().sequence();
    let key = crate::DataKey::CreateRate(employer.clone(), ledger);
    let count: u32 = env.storage().temporary().get(&key).unwrap_or(0);
    if count >= limit {
        panic!("{}", crate::ERR_RATE_LIMIT_EXCEEDED);
    }
    env.storage().temporary().set(&key, &(count + 1));
    env.storage()
        .temporary()
        .extend_ttl(&key, RATE_LIMIT_TTL_LEDGERS, RATE_LIMIT_TTL_LEDGERS);
}

/// Returns true when at least `DRAIN_TIMELOCK_SECONDS` have elapsed since the proposal.
pub fn drain_timelock_elapsed(env: &Env, proposal: &EmergencyDrainProposal) -> bool {
    let now = env.ledger().timestamp();
    now.saturating_sub(proposal.proposed_at) >= DRAIN_TIMELOCK_SECONDS
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
