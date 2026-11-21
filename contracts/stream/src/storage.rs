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

const TOKEN_ALLOWLIST_ENABLED_KEY: &str = "token_allowlist_enabled";
const TOKEN_ALLOWLIST_KEY: &str = "token_allowlist";

/// Returns true when the admin has enabled the token allowlist.
pub fn is_token_allowlist_enabled(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&TOKEN_ALLOWLIST_ENABLED_KEY)
        .unwrap_or(false)
}

pub fn set_token_allowlist_enabled(env: &Env, enabled: bool) {
    env.storage()
        .instance()
        .set(&TOKEN_ALLOWLIST_ENABLED_KEY, &enabled);
}

/// Returns true when `token` is present in the stored allowlist.
pub fn is_token_in_allowlist(env: &Env, token: &Address) -> bool {
    env.storage()
        .persistent()
        .get(&(TOKEN_ALLOWLIST_KEY, token.clone()))
        .unwrap_or(false)
}

pub fn add_token_to_allowlist(env: &Env, token: &Address) {
    env.storage()
        .persistent()
        .set(&(TOKEN_ALLOWLIST_KEY, token.clone()), &true);
}

pub fn remove_token_from_allowlist(env: &Env, token: &Address) {
    env.storage()
        .persistent()
        .remove(&(TOKEN_ALLOWLIST_KEY, token.clone()));
}

/// Replaces the entire allowlist with `tokens` and enables it when non-empty.
/// Passing an empty vector disables the allowlist (permissive default).
pub fn set_token_allowlist(env: &Env, tokens: &soroban_sdk::Vec<Address>) {
    for token in tokens.iter() {
        add_token_to_allowlist(env, &token);
    }
    set_token_allowlist_enabled(env, !tokens.is_empty());
}

/// Returns true when `token` may be used for a stream.
///
/// The allowlist is permissive by default: when it is disabled (or empty) any
/// token is accepted. When enabled, only explicitly allowed tokens pass.
pub fn is_token_allowed(env: &Env, token: &Address) -> bool {
    if !is_token_allowlist_enabled(env) {
        return true;
    }
    is_token_in_allowlist(env, token)
}

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
