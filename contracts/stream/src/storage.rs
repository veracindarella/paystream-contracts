use soroban_sdk::{contracttype, Address, Env, Map, Symbol};

/// Minimum delay between proposing and executing an emergency drain (24 hours).
pub const DRAIN_TIMELOCK_SECONDS: u64 = 86_400;

/// Default maximum number of items processed by a single batch entrypoint.
pub const DEFAULT_MAX_BATCH_SIZE: u32 = 50;

/// Lower bound for the admin-configurable batch size.
pub const MIN_MAX_BATCH_SIZE: u32 = 1;

/// Upper bound for the admin-configurable batch size.
pub const MAX_MAX_BATCH_SIZE: u32 = 200;

/// Maximum number of entries allowed in a stream's metadata map.
pub const MAX_METADATA_ENTRIES: u32 = 8;

/// Error returned when a metadata map exceeds `MAX_METADATA_ENTRIES`.
pub const ERR_METADATA_TOO_LARGE: u32 = 0x1003;

/// Error returned when a configured batch size is outside the allowed bounds.
pub const ERR_MAX_BATCH_SIZE_OUT_OF_BOUNDS: u32 = 0x1004;

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

/// Persists a stream's bounded metadata map in persistent storage, extending its TTL.
///
/// Panics with `ERR_METADATA_TOO_LARGE` when the map has more than
/// `MAX_METADATA_ENTRIES` entries.
pub fn set_stream_metadata(env: &Env, stream_id: u64, metadata: &Map<Symbol, Symbol>) {
    if metadata.len() > MAX_METADATA_ENTRIES {
        panic!("{}", ERR_METADATA_TOO_LARGE);
    }
    let key = crate::DataKey::StreamMetadata(stream_id);
    env.storage().persistent().set(&key, metadata);
    env.storage().persistent().extend_ttl(
        &key,
        crate::PERSISTENT_TTL_THRESHOLD,
        crate::PERSISTENT_TTL_EXTEND_TO,
    );
}

/// Returns the metadata map stored for `stream_id`, if any.
pub fn get_stream_metadata(env: &Env, stream_id: u64) -> Option<Map<Symbol, Symbol>> {
    let key = crate::DataKey::StreamMetadata(stream_id);
    let metadata: Option<Map<Symbol, Symbol>> = env.storage().persistent().get(&key);
    if metadata.is_some() {
        env.storage().persistent().extend_ttl(
            &key,
            crate::PERSISTENT_TTL_THRESHOLD,
            crate::PERSISTENT_TTL_EXTEND_TO,
        );
    }
    metadata
}

/// Stores the admin-configurable maximum batch size in instance storage.
///
/// Panics with `ERR_MAX_BATCH_SIZE_OUT_OF_BOUNDS` when `max` is outside
/// `MIN_MAX_BATCH_SIZE..=MAX_MAX_BATCH_SIZE`.
pub fn set_max_batch_size(env: &Env, max: u32) {
    if max < MIN_MAX_BATCH_SIZE || max > MAX_MAX_BATCH_SIZE {
        panic!("{}", ERR_MAX_BATCH_SIZE_OUT_OF_BOUNDS);
    }
    env.storage()
        .instance()
        .set(&crate::DataKey::MaxBatchSize, &max);
}

/// Returns the configured maximum batch size, defaulting to `DEFAULT_MAX_BATCH_SIZE`.
pub fn get_max_batch_size(env: &Env) -> u32 {
    env.storage()
        .instance()
        .get(&crate::DataKey::MaxBatchSize)
        .unwrap_or(DEFAULT_MAX_BATCH_SIZE)
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
