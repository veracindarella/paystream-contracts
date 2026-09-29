<!-- SPDX-License-Identifier: Apache-2.0 -->

# Admin Keypair Rotation Runbook

This document describes how to rotate the PayStream contract admin keypair.
Follow this procedure whenever you want to move admin authority to a new key,
or whenever the existing admin key is suspected to be compromised.

---

## Overview

PayStream uses a **two-step admin transfer** pattern to prevent accidental or
malicious key lockout:

1. **`propose_admin`** — current admin nominates a new admin and binds a nonce
   to the proposal.
2. **`accept_admin`** — the nominated address confirms acceptance using the same
   nonce.

Both steps require authentication from their respective parties, and the nonce
binds the proposal to a specific on-chain state, preventing front-running or
replay attacks.

---

## Prerequisites

- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli) installed
- Current admin keypair available and accessible
- Replacement admin keypair generated and secured
- Contract ID and network parameters on hand

```bash
# Environment setup
export NETWORK=testnet          # or mainnet
export CONTRACT_ID=<STREAM_CONTRACT_ID>
export OLD_ADMIN_KEY=<PATH_OR_ALIAS_TO_OLD_ADMIN_KEY>
export NEW_ADMIN_KEY=<PATH_OR_ALIAS_TO_NEW_ADMIN_KEY>
export OLD_ADMIN_ADDRESS=<OLD_ADMIN_STELLAR_ADDRESS>
export NEW_ADMIN_ADDRESS=<NEW_ADMIN_STELLAR_ADDRESS>
```

---

## Step 1 — Query the current admin nonce

All admin operations consume a monotonically-increasing nonce for replay
protection. Always read the current nonce before calling any admin function.

```bash
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  -- admin_nonce
```

Record the returned value as `CURRENT_NONCE`.

```bash
export CURRENT_NONCE=<VALUE_RETURNED_ABOVE>
```

---

## Step 2 — Propose the new admin

The current admin nominates the replacement address. This call consumes
`CURRENT_NONCE` and stores it alongside the pending admin address so
`accept_admin` can verify it.

```bash
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  --source "$OLD_ADMIN_KEY" \
  -- propose_admin \
  --new_admin "$NEW_ADMIN_ADDRESS" \
  --nonce "$CURRENT_NONCE"
```

**Verify the proposal was recorded:**

```bash
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  -- get_pending_admin
```

The output must show `$NEW_ADMIN_ADDRESS`. If the output is empty the call did
not succeed — check transaction status before continuing.

---

## Step 3 — Accept with the new admin key

The nominated admin confirms acceptance. `nonce` must match the value passed
to `propose_admin` in Step 2.

```bash
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  --source "$NEW_ADMIN_KEY" \
  -- accept_admin \
  --new_admin "$NEW_ADMIN_ADDRESS" \
  --nonce "$CURRENT_NONCE"
```

---

## Step 4 — Verify the transfer

```bash
# 1. Pending admin must now be cleared
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  -- get_pending_admin
# Expected: None / empty

# 2. Admin nonce must have incremented to CURRENT_NONCE + 1
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  -- admin_nonce
# Expected: CURRENT_NONCE + 1

# 3. Smoke-test: new admin can perform an admin operation
stellar contract invoke \
  --network "$NETWORK" \
  --id "$CONTRACT_ID" \
  --source "$NEW_ADMIN_KEY" \
  -- set_min_deposit \
  --admin "$NEW_ADMIN_ADDRESS" \
  --nonce "$((CURRENT_NONCE + 1))" \
  --amount <CURRENT_MIN_DEPOSIT_VALUE>
# Expected: success (no error)
```

If step 3 succeeds, the rotation is complete. Revoke / destroy the old admin
key material according to your key-management policy.

---

## Emergency Procedure — Old key compromised before transfer completes

If you suspect the **old admin key is compromised** and have not yet completed
Step 3:

### Scenario A — `propose_admin` has NOT been called yet

The attacker has the old key but there is no pending proposal on-chain.
You must race them:

1. Immediately call `propose_admin` from the old key to the new key (Step 2
   above).
2. Call `accept_admin` from the new key (Step 3 above) as fast as possible.
3. After `accept_admin` succeeds the old key loses all authority.
4. Optionally pause the contract while the rotation is in flight to block any
   attacker actions:

   ```bash
   stellar contract invoke \
     --network "$NETWORK" \
     --id "$CONTRACT_ID" \
     --source "$OLD_ADMIN_KEY" \
     -- pause_contract \
     --nonce "$CURRENT_NONCE"
   ```

   Then unpause after the rotation completes (using the **new** admin key and
   the incremented nonce).

### Scenario B — `propose_admin` has been called, `accept_admin` NOT yet called

The pending admin address is already set on-chain. Only the nominated address
can call `accept_admin`, so the attacker cannot redirect the transfer to a
different address.

1. Call `accept_admin` from the new key immediately (Step 3 above).
2. The old key loses authority the moment `accept_admin` succeeds.

### Scenario C — New key is also suspected compromised

Contact `security@paystream.example` immediately. Do not attempt further
on-chain actions until you have a secure key pair in hand.

---

## Key Verification Checklist

| Step | Command | Expected result |
|------|---------|-----------------|
| Pre-rotation | `admin_nonce` | Current nonce N |
| After `propose_admin` | `get_pending_admin` | New admin address |
| After `accept_admin` | `get_pending_admin` | None / empty |
| After `accept_admin` | `admin_nonce` | N + 1 |
| Smoke-test | admin-only call with new key | Success |
| Smoke-test | admin-only call with old key | Auth failure |

---

## Related Documents

- [SECURITY.md](../../SECURITY.md) — vulnerability reporting and overall
  security policy
- [docs/security/admin-multisig.md](admin-multisig.md) — using a multisig
  account as the admin keypair
- [docs/security/emergency-drain.md](emergency-drain.md) — emergency fund
  recovery procedure
- [docs/api-reference.md](../api-reference.md) — full `propose_admin`,
  `accept_admin`, and `admin_nonce` API documentation
