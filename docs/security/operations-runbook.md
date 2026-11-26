# Operations Runbook

This runbook covers routine and emergency operations for the streaming contract. All admin actions on mainnet require multi-sig authorization (see `docs/security/admin-multisig.md`).

## Admin Multi-Sig Requirement (Mainnet)

On mainnet the admin address MUST be a 2-of-3 multi-sig (Stellar native multisig or a deployed multisig contract). No single key may hold admin authority. Every admin call below must be signed by at least two of the three designated signers.

### Signing steps for every admin call

1. **Prepare** — the operator builds the admin transaction (e.g. `set_admin`, `pause`, `unpause`, `upgrade`, parameter changes) and records the XDR.
2. **Distribute** — share the unsigned XDR with the other signers out-of-band (encrypted channel).
3. **Collect signatures** — each signer independently verifies the transaction contents (contract ID, function, arguments, network passphrase) and signs. At least **2 of 3** signatures are required.
4. **Submit** — a signer submits the fully-signed transaction to the network.
5. **Verify on-chain** — confirm the transaction succeeded and the resulting state change (e.g. new admin, paused flag) via a ledger explorer or `stellar contract invoke --id <contract> -- get_admin`.
6. **Record** — log the transaction hash, signers, and timestamp in the operations log.

> A single-signer admin transaction MUST be rejected during review. If only one signature is available, do not submit.

## Routine Operations

### Pause / Unpause

- Build the `pause` / `unpause` transaction.
- Follow the multi-sig signing steps above (2-of-3).
- Verify the paused state on-chain before announcing.

### Parameter Updates

- Build the parameter-change transaction.
- Follow the multi-sig signing steps above (2-of-3).
- Verify the new parameter values on-chain.

## Emergency Operations

For emergency drains and incident response, follow `docs/security/emergency-drain.md`. Emergency admin calls are still subject to the 2-of-3 multi-sig requirement; pre-stage signer availability so emergency actions are not blocked.

## Admin Transfer

To rotate or transfer admin authority:

1. Create the new 2-of-3 multi-sig account (see onboarding checklist in `docs/security/admin-multisig.md`).
2. Build the `set_admin` transaction targeting the new multi-sig address.
3. Collect 2-of-3 signatures from the **current** admin signers and submit.
4. Verify the new admin on-chain (`get_admin` returns the new multi-sig address).
5. Confirm the old signers can no longer authorize admin calls.

## Observability and Health Checks

The indexer/API exposes the following endpoints for monitoring:

- **`/healthz`** — liveness probe. Returns `200` while the process is running.
- **`/readyz`** — readiness probe. Returns `200` only when the indexer is within the acceptable lag behind the chain head **and** the database is reachable; otherwise returns `503` with a reason. Use this endpoint for load-balancer and orchestrator readiness checks.
- **`/metrics`** — Prometheus metrics endpoint. Exports indexer lag behind chain, event throughput, and error rate.

### Lag Alert

A Prometheus alert fires when indexer lag exceeds the configured threshold (see the alert rules in the OPS monitoring configuration). The alert is wired to OPS monitoring and pages the on-call operator. On firing:

1. Check `/readyz` to confirm whether the indexer is lagging or the DB is unreachable.
2. Inspect `/metrics` for lag, event throughput, and error-rate trends.
3. Follow the incident response steps in `docs/security/emergency-drain.md` if the lag is caused by a stalled or failing indexer.

## Admin Audit Trail

All admin and governance actions are indexed for audit: `propose_admin`/`accept_admin`, `pause`/`unpause`, `set_min_deposit`, `set_protocol_fee`, and `upgrade` steps. The admin nonce sequence is reconstructed from these events.

- **Query by admin address** — the audit trail is queryable by admin address so governance tools can review all activity for a given admin.
- **Unexpected admin action alerts** — alerts fire on unexpected admin actions (e.g. admin changes, pause/unpause, or parameter changes not recorded in the operations log). On firing, cross-check the transaction against the operations log and the multi-sig signing steps above.

## Verification Checklist

- [ ] Admin address is a 2-of-3 multi-sig (verified on-chain)
- [ ] All admin transactions carry at least 2 signatures
- [ ] Transaction hashes and signers logged
- [ ] State changes verified on-chain after each admin call
- [ ] Procedure rehearsed on testnet before mainnet deployment
- [ ] `/healthz` and `/readyz` return healthy responses
- [ ] Lag alert configured and routed to OPS monitoring
- [ ] Admin audit trail queryable by admin address; unexpected-admin-action alerts verified
