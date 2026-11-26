# PayStream Mainnet Prototype

This document tracks the mainnet prototype effort (PROD-34 / issue #323). All
development and testing to date has been on Stellar Testnet. This prototype
deploys PayStream to Stellar Mainnet with a small real-money stream to validate
end-to-end operation with real XLM, confirm fee levels, and surface any
testnet-vs-mainnet differences in the Soroban host.

> **Status:** Prototype in progress. Contract IDs are placeholders until the
> mainnet deployment is complete.

---

## 1. Mainnet deployment prerequisites checklist

The following security findings / checks must be completed (or acknowledged as
accepted risk) before mainnet deployment:

| ID | Finding / check | Status | Notes |
|----|-----------------|--------|-------|
| SEC-22 | Admin multisig setup (2-of-3 or stronger) for the mainnet admin | ☐ | See [docs/security/admin-multisig.md](security/admin-multisig.md). Mainnet admin must not be a single-signature account. |
| SEC-30 | Re-initialization guard on `initialize` (LOW-02 fix) | ☐ | `initialize` panics if already initialised; verify guard is in place before mainnet deploy. |
| LOW-02 | Re-initialization guard must be resolved before mainnet | ☐ | Referenced in [SECURITY.md](SECURITY.md#mainnet-readiness). |

Additional pre-flight items:

- [ ] Rust toolchain + `wasm32-unknown-unknown` target installed
- [ ] Stellar CLI configured for mainnet (`stellar network add mainnet ...`)
- [ ] Mainnet deployer account funded with real XLM
- [ ] Contract WASMs built in release mode (`stellar contract build`)
- [ ] Mainnet RPC endpoint configured (e.g. https://soroban-mainnet.stellar.org)

---

## 2. Test stream on mainnet

A small real-money stream is created on mainnet to validate end-to-end
operation:

- **Amount:** 1 XLM (10,000,000 stroops)
- **Duration:** 1 hour
- **Rate:** ~2.78 stroops/second
- **Token:** native XLM (Stellar Asset Contract)
- **Roles:** employer funds, employee withdraws

### Steps

1. Deploy the token and stream contracts to mainnet:
   ```bash
   stellar contract deploy \
     --wasm target/wasm32-unknown-unknown/release/paystream_token.wasm \
     --source <deployer> --network mainnet
   stellar contract deploy \
     --wasm target/wasm32-unknown-unknown/release/paystream_stream.wasm \
     --source <deployer> --network mainnet
   ```

2. Initialise both contracts with the mainnet admin (multisig) address.

3. Create the stream:
   ```bash
   stellar contract invoke --id <STREAM_ID> --source <employer> --network mainnet \
     -- create_stream \
       --employer <EMPLOYER> \
       --employee <EMPLOYEE> \
       --token_address <NATIVE_XLM_SAC_ID> \
       --deposit 10000000 \
       --rate_per_second 3 \
       --stop_time <now + 3600>
   ```

4. After the stream runs (or is stopped), the employee calls `withdraw` to claim
   earned tokens.

---

## 3. Withdrawal executed and verified on-chain

After the test stream is created and has accrued:

- Employee calls `withdraw --employee <EMPLOYEE> --stream_id <STREAM_ID>` on mainnet.
- Transaction hash and result are recorded below.
- The employee's XLM balance is verified to have increased by the claimable amount.

| Field | Value |
|-------|-------|
| Stream ID | `<pending>` |
| Employer | `<pending>` |
| Employee | `<pending>` |
| Withdrawal tx hash | `<pending>` |
| Amount withdrawn | `<pending>` stroops |
| Block explorer link | `<pending>` |

---

## 4. Measured transaction fees

Actual mainnet transaction fees are measured during the prototype and documented
here. See [benchmarks/](benchmarks/) for the fee data.

| Operation | Fee (stroops) | Ledger | Tx hash |
|-----------|---------------|--------|---------|
| Contract deploy (token) | `<pending>` | `<pending>` | `<pending>` |
| Contract deploy (stream) | `<pending>` | `<pending>` | `<pending>` |
| initialize × 2 | `<pending>` | `<pending>` | `<pending>` |
| create_stream | `<pending>` | `<pending>` | `<pending>` |
| withdraw | `<pending>` | `<pending>` | `<pending>` |

Update [benchmarks/baseline.json](benchmarks/baseline.json) with measured CPU
instruction counts and fee observations.

---

## 5. Contract IDs (mainnet)

Contract IDs are published here once deployed to mainnet. These are
placeholders until the launch.

| Contract | Mainnet ID | Deployed at |
|----------|------------|-------------|
| PayStream Token | `<pending>` | `<pending>` |
| PayStream Stream | `<pending>` | `<pending>` |

Deep links to [Stellar Expert](https://stellar.expert) for each contract are
added in [docs/api-reference.md](api-reference.md) once the IDs are known.

### Network configuration

The dashboard and indexer select the network via environment variables — no
code changes are required to switch between testnet and mainnet. See
[docs/networks.md](networks.md) for the full switching guide.

| Setting | Mainnet value |
|---------|---------------|
| Network passphrase | `Public Global Stellar Network ; September 2015` |
| Soroban RPC URL | `https://soroban-mainnet.stellar.org` |
| Token contract ID | `<pending>` (set `NEXT_PUBLIC_TOKEN_CONTRACT_ID` / `TOKEN_CONTRACT_ID`) |
| Stream contract ID | `<pending>` (set `NEXT_PUBLIC_STREAM_CONTRACT_ID` / `STREAM_CONTRACT_ID`) |

To run the stack against mainnet:

```bash
# Dashboard
NEXT_PUBLIC_NETWORK=mainnet \
NEXT_PUBLIC_SOROBAN_RPC_URL=https://soroban-mainnet.stellar.org \
NEXT_PUBLIC_TOKEN_CONTRACT_ID=<mainnet token id> \
NEXT_PUBLIC_STREAM_CONTRACT_ID=<mainnet stream id> \
npm run dev

# Indexer / API
NETWORK=mainnet \
SOROBAN_RPC_URL=https://soroban-mainnet.stellar.org \
TOKEN_CONTRACT_ID=<mainnet token id> \
STREAM_CONTRACT_ID=<mainnet stream id> \
npm run start
```

On startup the indexer and API validate the configured passphrase against the
selected network and refuse to start on a mismatch. Contract IDs are also
validated against the network (format and expected network) and startup fails
if they do not match.

---

## 6. Testnet vs. mainnet differences observed

Any differences between the Soroban host on testnet and mainnet surfaced during
the prototype are recorded here:

- `<pending>`

---

## Next steps

- Complete the prerequisites checklist above.
- Deploy to mainnet and publish the contract IDs.
- Run the 1 XLM / 1 hour test stream.
- Execute and verify the withdrawal.
- Measure and document fees in benchmarks/.
