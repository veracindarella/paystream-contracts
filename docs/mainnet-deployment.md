# Mainnet Deployment Runbook

This document covers everything required to deploy PayStream contracts to Stellar Mainnet safely.

> **Dependency:** SC-01 (LOW-02 re-initialization guard fix) must be merged and included in the WASM before running a mainnet deployment.

---

## Pre-Flight Checklist

Complete every item before executing `deploy-mainnet.sh`.

### Code Quality
- [ ] All CI checks pass on the commit being deployed (`cargo test`, `cargo clippy`, `cargo fmt --check`)
- [ ] LOW-02 fix (re-initialization guard in `initialize`) is confirmed present in the WASM being deployed
- [ ] Contract version number in `lib.rs` (`CONTRACT_VERSION`) is up to date
- [ ] No open Critical or High security issues in the repository

### Key Management
- [ ] Deployer key is a funded mainnet account (minimum 10 XLM recommended for fees)
- [ ] Admin key is separate from the deployer key
- [ ] Admin key will be transferred to a multisig wallet after initialization (see [Admin Multisig Setup](#admin-multisig-setup))
- [ ] Deployer key is stored securely and not committed to the repository

### Environment
- [ ] Stellar CLI v22.0.0 or later installed (`stellar --version`)
- [ ] `STELLAR_SOURCE_ACCOUNT` environment variable set to the deployer account name
- [ ] Mainnet RPC endpoint is reachable
- [ ] Sufficient XLM balance for deployment fees

### Audit & Review
- [ ] Contract WASM has been reviewed or audited
- [ ] Deployment has been reviewed by at least one additional team member
- [ ] Post-deploy validation steps are understood

---

## Initial Parameter Values

Use these values when calling `initialize` and `set_min_deposit` after deployment:

| Parameter | Recommended Value | Rationale |
|---|---|---|
| `min_deposit` | `10_000_000` (10 XLM in stroops) | Prevents dust streams; adjustable post-launch |
| `rate_per_second` max | `1_000_000_000` (enforced by contract) | Contract-level cap |

---

## Deployment Steps

### 1. Build and verify locally

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
stellar contract build
```

### 2. Run the deployment script

```bash
export STELLAR_SOURCE_ACCOUNT=<your-mainnet-account-name>
./scripts/deploy-mainnet.sh --confirm
```

The `--confirm` flag is required to prevent accidental execution.

Save the output — you will need the contract IDs for initialization.

### 3. Initialize the stream contract

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <ADMIN_ACCOUNT> \
  --network mainnet \
  -- initialize \
  --admin <ADMIN_ADDRESS>
```

### 4. Set minimum deposit

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <ADMIN_ACCOUNT> \
  --network mainnet \
  -- set_min_deposit \
  --admin <ADMIN_ADDRESS> \
  --nonce 0 \
  --amount 10000000
```

### 5. Initialize the token contract

```bash
stellar contract invoke \
  --id <TOKEN_CONTRACT_ID> \
  --source <ADMIN_ACCOUNT> \
  --network mainnet \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --supply <INITIAL_SUPPLY>
```

### 6. Verify LOW-02 fix

Confirm the re-initialization guard is active by attempting to call `initialize` a second time — it must fail:

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <ADMIN_ACCOUNT> \
  --network mainnet \
  -- initialize \
  --admin <ANY_ADDRESS>
# Expected: transaction fails with "already initialized"
```

---

## Admin Multisig Setup

After deployment and initialization, transfer admin control from the deployer key to a multisig wallet.

### Proposed admin

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <CURRENT_ADMIN> \
  --network mainnet \
  -- propose_admin \
  --new_admin <MULTISIG_ADDRESS> \
  --nonce <CURRENT_NONCE>
```

### Accept admin (from multisig)

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <MULTISIG_ACCOUNT> \
  --network mainnet \
  -- accept_admin \
  --new_admin <MULTISIG_ADDRESS> \
  --nonce <PROPOSAL_NONCE>
```

### Verify the transfer

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --network mainnet \
  -- get_pending_admin
# Expected: null (transfer complete, no pending admin)
```

---

## Post-Deploy Validation

Run these checks after every deployment:

```bash
# Confirm stream_count starts at 0
stellar contract invoke --id <STREAM_ID> --network mainnet -- stream_count
# Expected: 0

# Confirm contract is not paused
stellar contract invoke --id <STREAM_ID> --network mainnet -- is_paused
# Expected: false

# Confirm version is set
stellar contract invoke --id <STREAM_ID> --network mainnet -- version
# Expected: 1 (after migrate is called)
```

### Call migrate after upgrade

After any WASM upgrade, call `migrate` to stamp the new version:

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <ADMIN_ACCOUNT> \
  --network mainnet \
  -- migrate \
  --admin <ADMIN_ADDRESS>
```

---

## Rollback

Soroban contracts cannot be deleted. In the event of a critical issue post-deploy:

1. **Pause the contract** immediately to block new streams and withdrawals:

   ```bash
   stellar contract invoke \
     --id <STREAM_ID> \
     --source <ADMIN_ACCOUNT> \
     --network mainnet \
     -- pause_contract \
     --nonce <CURRENT_NONCE>
   ```

2. Allow existing employees to claim earned tokens via `cancel_stream` (employer action).
3. Deploy a patched contract at a new address.
4. Communicate the new contract address to all integrators.

---

## Environment Variables Reference

| Variable | Required | Description |
|---|---|---|
| `STELLAR_SOURCE_ACCOUNT` | Yes | Stellar CLI account name for the deployer key |
| `STELLAR_RPC_URL` | No | Override default mainnet RPC endpoint |
| `STELLAR_NETWORK_PASSPHRASE` | No | Override default mainnet network passphrase |

---

## Useful Commands

```bash
# Check account balance
stellar account info --account <ADDRESS> --network mainnet

# Query admin nonce before any admin operation
stellar contract invoke --id <STREAM_ID> --network mainnet -- admin_nonce

# Query contract version
stellar contract invoke --id <STREAM_ID> --network mainnet -- version
```
