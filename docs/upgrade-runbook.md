# Upgrade Runbook: Contract WASM Upgrades

Operational runbook for the time-locked `propose_upgrade` → `execute_upgrade` flow of the **stream** and **token** contracts. For Soroban SDK version bumps, see [upgrade-guide.md](upgrade-guide.md) and [soroban-sdk-upgrade.md](soroban-sdk-upgrade.md).

**Key constants**

| Constant | Value | Ledgers (≈5 s each) |
|---|---|---|
| `TIMELOCK_DELAY` | 172 800 s (48 h) | ≈ 34 560 |

Every admin call consumes the current admin nonce. Read it with `admin_nonce` before each step. A wrong nonce fails with E009.

The runbook applies to both contracts. Pick the target with `CONTRACT` and the matching WASM artifact:

| `CONTRACT` | `ID` | WASM artifact |
|---|---|---|
| `stream` | `<STREAM_CONTRACT_ID>` | `paystream_stream.wasm` |
| `token` | `<TOKEN_CONTRACT_ID>` | `paystream_token.wasm` |

```bash
export NET=testnet CONTRACT=stream
export ID=<STREAM_CONTRACT_ID> ADMIN=<admin-identity>
export WASM=target/wasm32v1-none/release/paystream_stream.wasm
# For the token contract instead:
#   export CONTRACT=token ID=<TOKEN_CONTRACT_ID> WASM=target/wasm32v1-none/release/paystream_token.wasm

nonce() { stellar contract invoke --id $ID --source $ADMIN --network $NET -- admin_nonce; }
```

> All commands below were validated on a sandbox (local `stellar network container` + testnet) before being added here. Re-run them on testnet before mainnet.

## Normal upgrade

| # | Step | Duration |
|---|---|---|
| 1 | Build and upload new WASM | ~5 min (1–2 ledgers to upload) |
| 2 | `propose_upgrade` | 1 ledger |
| 3 | Wait for the timelock | 48 h (≈34 560 ledgers) |
| 4 | `execute_upgrade` | 1 ledger |
| 5 | `migrate` + verify `version` | 1–2 ledgers |
| 6 | Smoke tests | ~15 min |

1. **Build and upload.** Bump `CONTRACT_VERSION` first.
   ```bash
   cargo test && stellar contract build
   HASH=$(stellar contract upload --wasm $WASM --source $ADMIN --network $NET)
   echo "uploaded $CONTRACT wasm: $HASH"
   ```
   Record `HASH` and the git commit in the release notes.
2. **Propose.** Record the nonce you use and the `unlock_time` from the `upgrade_proposed` event.
   ```bash
   export PROPOSE_NONCE=$(nonce)
   stellar contract invoke --id $ID --source $ADMIN --network $NET -- \
     propose_upgrade --new_wasm_hash $HASH --nonce $PROPOSE_NONCE
   ```
   Announce the upgrade, the hash, and the unlock time to integrators.
3. **Wait** until ledger timestamp ≥ `unlock_time`. Calling `execute_upgrade` earlier fails with E026. Check the pending upgrade at any time:
   ```bash
   stellar contract invoke --id $ID --network $NET -- pending_upgrade
   ```
4. **Execute.**
   ```bash
   stellar contract invoke --id $ID --source $ADMIN --network $NET -- execute_upgrade --nonce $(nonce)
   ```
   Confirm the `upgrade_executed` event carries the expected hash.
5. **Migrate and verify.**
   ```bash
   stellar contract invoke --id $ID --source $ADMIN --network $NET -- migrate --admin <ADMIN_ADDRESS>
   ```
   Then run the verify-deploy step below.
6. **Smoke tests** (testnet first, then mainnet): `get_stream` on an existing stream, `claimable`, `stream_count`, `is_paused`. Then on testnet run a full create → withdraw → cancel cycle.

## Verify deploy

Run after `migrate` and again after any rollback. All three checks must pass before unpausing or announcing completion.

```bash
# 1. Version must equal the new CONTRACT_VERSION in the release notes.
stellar contract invoke --id $ID --network $NET -- version

# 2. No upgrade may remain pending (empty/None after execute).
stellar contract invoke --id $ID --network $NET -- pending_upgrade

# 3. Admin nonce advanced by the number of admin calls made.
nonce
```

Smoke tests (stream contract):

```bash
stellar contract invoke --id $ID --network $NET -- stream_count
stellar contract invoke --id $ID --network $NET -- is_paused
stellar contract invoke --id $ID --network $NET -- get_stream --stream_id <EXISTING_STREAM_ID>
stellar contract invoke --id $ID --network $NET -- claimable --stream_id <EXISTING_STREAM_ID>
```

Smoke tests (token contract): `name`, `symbol`, `decimals`, `total_supply`, and a `balance` read for a known holder. On testnet, run a full create → withdraw → cancel cycle (stream) and a transfer round-trip (token) before promoting to mainnet.

## Rollback

> Full rollback procedure, decision tree, and incident comms: see **OPS-51** (rollback runbook). This section is the short form for the upgrade flow only.

- **Before execution:** cancel. This clears `PendingUpgrade` and emits `upgrade_cancelled`.
  ```bash
  stellar contract invoke --id $ID --source $ADMIN --network $NET -- cancel_upgrade --nonce $(nonce)
  ```
  Confirm with `pending_upgrade` (must be empty) and re-check `nonce`.
- **After execution:** there is no instant revert. Propose the **previous WASM hash** (it is still uploaded on-chain) and follow the normal flow, including the 48 h timelock. Pause the contract while you wait if the new version is harmful. Storage is preserved across upgrades. If the new version wrote incompatible storage, the rollback WASM must be able to read it.
  ```bash
  export ROLLBACK_HASH=<PREVIOUS_WASM_HASH>
  stellar contract invoke --id $ID --source $ADMIN --network $NET -- \
    propose_upgrade --new_wasm_hash $ROLLBACK_HASH --nonce $(nonce)
  # after the timelock:
  stellar contract invoke --id $ID --source $ADMIN --network $NET -- execute_upgrade --nonce $(nonce)
  stellar contract invoke --id $ID --source $ADMIN --network $NET -- migrate --admin <ADMIN_ADDRESS>
  ```
  Then re-run the verify-deploy step.

## Emergency upgrade

The timelock cannot be bypassed. To limit damage:

1. **Pause first:** `pause_contract --nonce $(nonce)`. This blocks state-changing entry points within 1 ledger.
2. Upload the fixed WASM and `propose_upgrade` right away.
3. Communicate the incident and the unlock time.
4. After 48 h: `execute_upgrade` → `migrate` → verify `version` → smoke tests.
5. `unpause_contract --nonce $(nonce)` once verified.

See also: [operations-runbook.md](operations-runbook.md), [security/key-rotation.md](security/key-rotation.md), OPS-51 (rollback runbook).
