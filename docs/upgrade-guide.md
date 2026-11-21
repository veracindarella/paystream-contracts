# Contract Upgrade Guide

PayStream uses Soroban's native WASM upgrade mechanism. The contract address never changes — only the executable bytecode is replaced. All persistent storage (streams, indices, admin) survives the upgrade untouched.

## How it works

1. **Build** the new contract version
2. **Upload** the new WASM to the ledger (produces a WASM hash)
3. **Call `upgrade`** on the deployed contract with the new WASM hash (admin only)
4. **Call `migrate`** if the new version requires data-model changes (admin only, no-op in base version)

> **Admin runbook:** for the concrete, copy-paste timelock flow
> (propose → wait → execute → migrate → verify), see
> [`docs/upgrade-runbook.md`](upgrade-runbook.md). That runbook is the
> authoritative procedure for production upgrades; the steps below are a
> condensed overview.

## Step-by-step

### 1. Build the new contract

```bash
stellar contract build
```

### 2. Upload the new WASM

```bash
stellar contract upload \
  --source-account <ADMIN_IDENTITY> \
  --wasm target/wasm32v1-none/release/paystream_stream.wasm \
  --network testnet
# outputs: <NEW_WASM_HASH>
```

### 3. Upgrade the deployed contract

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source-account <ADMIN_IDENTITY> \
  --network testnet \
  -- upgrade \
  --new_wasm_hash <NEW_WASM_HASH>
```

### 4. Run migration (if needed)

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source-account <ADMIN_IDENTITY> \
  --network testnet \
  -- migrate \
  --admin <ADMIN_ADDRESS>
```

### 5. Verify the deployment

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source-account <ADMIN_IDENTITY> \
  --network testnet \
  -- get_admin
# confirm the returned admin matches <ADMIN_ADDRESS>

stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source-account <ADMIN_IDENTITY> \
  --network testnet \
  -- get_version
# confirm the reported version matches the newly deployed build
```

See the verify-deploy step in [`docs/upgrade-runbook.md`](upgrade-runbook.md)
for the full post-upgrade verification checklist.

### Rollback

If an upgrade must be reverted, follow the rollback procedure in
[`docs/upgrade-runbook.md`](upgrade-runbook.md) (OPS-51). Do not attempt a
manual rollback outside that procedure.

## Storage compatibility

Soroban's upgrade replaces only the WASM bytecode. All `persistent` and `instance` storage entries remain intact. When adding new storage keys in an upgrade:

- New keys with `unwrap_or` defaults are safe — they return the default until explicitly set
- Renamed or removed keys require a `migrate` implementation to transform existing data
- The `DataKey` enum is `#[contracttype]` — adding new variants is backward-compatible

## Security

- Only the contract admin can call `upgrade` and `migrate`
- The admin address is stored in instance storage under `DataKey::Admin`
- Soroban emits a `SYSTEM` event on every upgrade containing the old and new WASM hashes, providing an on-chain audit trail

## Reentrancy Re-verification (SEC-16)

On each **major** Soroban SDK version bump, the reentrancy analysis must be re-verified.

See the checklist in [`docs/security/reentrancy-analysis.md`](security/reentrancy-analysis.md#re-verification-checklist) for the full procedure.

In summary:
1. Read the SDK release notes for changes to the cross-contract call model.
2. Re-inspect the `withdraw` and `cancel_stream` call graphs.
3. Confirm `token::transfer` is still a leaf call with no callback path.
4. Run `test_reentrant_withdraw_rejected`.
5. Update `docs/security/reentrancy-analysis.md` with the new SDK version and review date.

## Token Contract Upgrades (TOK-10)

The token contract supports the same in-place pattern. Balances, allowances,
total supply and the admin nonce live in contract storage and survive the upgrade.

```bash
cargo build -p paystream-token --target wasm32v1-none --release
stellar contract upload --wasm target/wasm32v1-none/release/paystream_token.wasm \
  --source admin --network testnet
# outputs: <NEW_WASM_HASH>

# Read the current nonce, then upgrade
stellar contract invoke --id <TOKEN_CONTRACT_ID> --source admin --network testnet -- admin_nonce
stellar contract invoke --id <TOKEN_CONTRACT_ID> --source admin --network testnet \
  -- upgrade --new_wasm_hash <NEW_WASM_HASH> --nonce <NONCE>

# Confirm the new WASM is operational (no-op hook, admin only)
stellar contract invoke --id <TOKEN_CONTRACT_ID> --source admin --network testnet \
  -- migrate --admin <ADMIN_ADDRESS>

# Verify the deployment: nonce advanced and admin unchanged
stellar contract invoke --id <TOKEN_CONTRACT_ID> --source admin --network testnet -- admin_nonce
stellar contract invoke --id <TOKEN_CONTRACT_ID> --source admin --network testnet -- get_admin
```

- `upgrade` requires the stored admin's auth and consumes the admin nonce shared with `mint`.
  Always read the nonce immediately before proposing/executing the upgrade and pass that
  exact value — a stale nonce causes the upgrade to fail.
- Never reorder or remove `TokenDataKey` variants; only append new ones.
- Verify with `cargo test -p paystream-token --features wasm-tests` (requires the release WASM build).
- The propose → wait → execute → migrate → verify flow and the verify-deploy checklist
  are the same as for the stream contract; see
  [`docs/upgrade-runbook.md`](upgrade-runbook.md). Rollback is covered there too (OPS-51).

## Soroban SDK Upgrades

Bumping the `soroban-sdk` dependency itself is covered in [soroban-sdk-upgrade.md](soroban-sdk-upgrade.md).
