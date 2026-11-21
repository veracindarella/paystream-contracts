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

/// Reads the current protocol fee config.
///
/// Returns `Some((fee_bps, treasury))` when a fee is configured and enabled,
/// or `None` when the fee is disabled (key absent or `fee_bps == 0`).
/// Read-only: never mutates storage.
pub fn get_protocol_fee_config(env: &Env) -> Option<(u32, Address)> {
    let config: Option<(u32, Address)> = env.storage().instance().get(&crate::DataKey::ProtocolFee);
    match config {
        Some((fee_bps, treasury)) if fee_bps > 0 => Some((fee_bps, treasury)),
        _ => None,
    }
}

/// TTL (in ledgers) applied to archived index entries in temporary storage.
pub const INDEX_ARCHIVE_TTL_LEDGERS: u32 = 30 * 24 * 60 * 12; // ~30 days at 5s ledgers

/// Temporary-storage archive key for a given address's stream index.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexArchiveKey {
    Employer(Address),
    Employee(Address),
    Token(Address),
}

/// Estimates the rent (in stroops) paid by the persistent index entries for
/// the given addresses. Used to quantify savings before archiving.
pub fn estimate_index_rent(env: &Env, employers: &[Address], employees: &[Address], tokens: &[Address]) -> u64 {
    let mut total: u64 = 0;
    for addr in employers {
        total = total.saturating_add(env.storage().persistent().get_rent(&crate::DataKey::EmployerStreams(addr.clone())).unwrap_or(0));
    }
    for addr in employees {
        total = total.saturating_add(env.storage().persistent().get_rent(&crate::DataKey::EmployeeStreams(addr.clone())).unwrap_or(0));
    }
    for addr in tokens {
        total = total.saturating_add(env.storage().persistent().get_rent(&crate::DataKey::TokenStreams(addr.clone())).unwrap_or(0));
    }
    total
}

/// Moves `completed_ids` out of the hot persistent index for `addr` into a
/// temporary-storage archive with a TTL, reducing persistent rent.
pub fn archive_completed_ids(env: &Env, key: IndexArchiveKey, completed_ids: &[u64]) {
    if completed_ids.is_empty() {
        return;
    }
    let archive_key = key.clone();
    let mut archived: soroban_sdk::Vec<u64> = env
        .storage()
        .temporary()
        .get(&archive_key)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));
    for id in completed_ids {
        archived.push_back(*id);
    }
    env.storage().temporary().set(&archive_key, &archived);
    env.storage().temporary().extend_ttl(&archive_key, INDEX_ARCHIVE_TTL_LEDGERS, INDEX_ARCHIVE_TTL_LEDGERS);

    // Remove archived IDs from the hot persistent index.
    let live_key = match &key {
        IndexArchiveKey::Employer(a) => crate::DataKey::EmployerStreams(a.clone()),
        IndexArchiveKey::Employee(a) => crate::DataKey::EmployeeStreams(a.clone()),
        IndexArchiveKey::Token(a) => crate::DataKey::TokenStreams(a.clone()),
    };
    let live: soroban_sdk::Vec<u64> = env
        .storage()
        .persistent()
        .get(&live_key)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));
    let mut remaining: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(env);
    for id in live.iter() {
        if !completed_ids.contains(&id) {
            remaining.push_back(id);
        }
    }
    env.storage().persistent().set(&live_key, &remaining);
}

/// Returns the union of live persistent IDs and archived temporary IDs for a key.
fn union_index(env: &Env, live_key: &crate::DataKey, archive_key: &IndexArchiveKey) -> soroban_sdk::Vec<u64> {
    let live: soroban_sdk::Vec<u64> = env
        .storage()
        .persistent()
        .get(live_key)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));
    let archived: soroban_sdk::Vec<u64> = env
        .storage()
        .temporary()
        .get(archive_key)
        .unwrap_or_else(|| soroban_sdk::Vec::new(env));
    let mut out: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(env);
    for id in live.iter() {
        out.push_back(id);
    }
    for id in archived.iter() {
        if !out.contains(&id) {
            out.push_back(id);
        }
    }
    out
}

/// Returns all stream IDs for an employer, including archived ones.
pub fn get_employer_streams(env: &Env, employer: &Address) -> soroban_sdk::Vec<u64> {
    union_index(
        env,
        &crate::DataKey::EmployerStreams(employer.clone()),
        &IndexArchiveKey::Employer(employer.clone()),
    )
}

/// Returns all stream IDs for an employee, including archived ones.
pub fn get_employee_streams(env: &Env, employee: &Address) -> soroban_sdk::Vec<u64> {
    union_index(
        env,
        &crate::DataKey::EmployeeStreams(employee.clone()),
        &IndexArchiveKey::Employee(employee.clone()),
    )
}

/// Returns all stream IDs for a token, including archived ones.
pub fn get_token_streams(env: &Env, token: &Address) -> soroban_sdk::Vec<u64> {
    union_index(
        env,
        &crate::DataKey::TokenStreams(token.clone()),
        &IndexArchiveKey::Token(token.clone()),
    )
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
