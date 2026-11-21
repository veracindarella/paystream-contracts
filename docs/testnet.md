# Testnet Deployment

This document lists the current PayStream testnet contract addresses, explains how to get test tokens, and describes how to interact with the contracts on Stellar Testnet.

> **Note:** Contract IDs change after every redeployment. This file is updated as part of the release process. If the IDs below are stale, redeploy using the scripts in `scripts/` and update this file.

---

## Current Contract IDs

| Contract | ID |
|---|---|
| PayStream Token | `CDZQHVQHQMHIGJGSQIJVXGPGNZJQDQV4BBKMG7MSIDTJTTBBQYAEUPB` |
| PayStream Stream | `CBXKDJUQYQDQKSQT6AKZRQWXDXTQHVXHBQQJQ3WKDXJHPNMRGYAQMRS` |

Network: **Stellar Testnet** (`https://horizon-testnet.stellar.org`)
RPC URL: `https://soroban-testnet.stellar.org`
Network passphrase: `Test SDF Network ; September 2015`
Last deployed: **2026-09-25**

### Selecting testnet in the indexer and dashboard

The indexer and dashboard are network-agnostic and pick their network from the environment:

```bash
# Indexer
NETWORK=testnet

# Dashboard
NEXT_PUBLIC_NETWORK=testnet
```

With `NETWORK=testnet` / `NEXT_PUBLIC_NETWORK=testnet`, both services use the testnet passphrase (`Test SDF Network ; September 2015`), the testnet RPC URL above, and the contract IDs listed in this document.

At startup, the indexer and API validate their configuration and **refuse to start** if:

- the configured passphrase does not match the selected network, or
- the configured contract IDs are not valid for the selected network (e.g. mainnet IDs while `NETWORK=testnet`).

See [networks.md](./networks.md) for the full list of per-network settings and how to switch networks.

---

## Prerequisites

- [Rust](https://rustup.rs/) (latest stable) with `wasm32-unknown-unknown` target
- [Stellar CLI](https://developers.stellar.org/docs/tools/developer-tools/cli/stellar-cli)

```bash
rustup target add wasm32-unknown-unknown
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"
```

---

## Getting a Testnet Account and Test XLM

1. Generate a keypair:
   ```bash
   stellar keys generate --global my-testnet-key --network testnet
   stellar keys address my-testnet-key
   ```

2. Fund it via the Friendbot faucet:
   ```bash
   # Replace <YOUR_PUBLIC_KEY> with the address from the previous step
   curl "https://friendbot.stellar.org?addr=<YOUR_PUBLIC_KEY>"
   ```
   Or use the web faucet: <https://laboratory.stellar.org/#account-creator?network=test>

3. Verify the balance:
   ```bash
   stellar account show <YOUR_PUBLIC_KEY> --network testnet
   ```

---

## Getting Test Tokens (PayStream Token)

The PayStream Token contract is pre-initialised with an admin-controlled supply. To mint test tokens to your account, ask the admin or run:

```bash
stellar contract invoke \
  --id <TOKEN_CONTRACT_ID> \
  --source <ADMIN_KEY> \
  --network testnet \
  -- mint --admin <ADMIN_ADDRESS> --to <YOUR_ADDRESS> --amount 1000000000
```

Check your balance:
```bash
stellar contract invoke \
  --id <TOKEN_CONTRACT_ID> \
  --source <YOUR_KEY> \
  --network testnet \
  -- balance --owner <YOUR_ADDRESS>
```

---

## Deploying Contracts (maintainers)

```bash
./scripts/build.sh
./scripts/deploy-testnet.sh
```

The script prints the new contract IDs. Export them and initialise:

```bash
export STELLAR_ADMIN_ADDRESS=<YOUR_PUBLIC_KEY>
export TOKEN_CONTRACT_ID=<FROM_DEPLOY>
export STREAM_CONTRACT_ID=<FROM_DEPLOY>
./scripts/init-testnet.sh
```

Update the **Current Contract IDs** table above and commit the change.

---

## Quick Interaction Examples

### Create a stream

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <EMPLOYER_KEY> \
  --network testnet \
  -- create_stream \
    --employer <EMPLOYER_ADDRESS> \
    --employee <EMPLOYEE_ADDRESS> \
    --token_address <TOKEN_CONTRACT_ID> \
    --deposit 1000000 \
    --rate_per_second 100 \
    --stop_time 0
```

### Check claimable amount

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <ANY_KEY> \
  --network testnet \
  -- claimable --stream_id 1
```

### Withdraw earnings

```bash
stellar contract invoke \
  --id <STREAM_CONTRACT_ID> \
  --source <EMPLOYEE_KEY> \
  --network testnet \
  -- withdraw --employee <EMPLOYEE_ADDRESS> --stream_id 1
```

---

## End-to-End Lifecycle Test

`scripts/e2e-test.sh` exercises the full payroll lifecycle against a real testnet:

1. Builds both contracts
2. Deploys token and stream contracts
3. Initialises both contracts
4. Generates employer and employee keypairs, funds via Friendbot
5. Mints tokens to the employer
6. Creates a stream (`rate_per_second=10`, `deposit=100000`)
7. Waits 30 seconds for salary to accrue
8. Withdraws claimable earnings and verifies the employee's balance increased
9. Cancels the remaining stream

The script exits non-zero on any failure.

```bash
export STELLAR_SOURCE_ACCOUNT=my-testnet-key
export STELLAR_ADMIN_ADDRESS=<YOUR_PUBLIC_KEY>
./scripts/e2e-test.sh
```

---

## Native XLM Streams (SAC)

PayStream accepts any SEP-41 token address. Stellar's native asset (XLM) is exposed as a SEP-41 token through its built-in **Stellar Asset Contract (SAC)**, so XLM streams need no custom token deployment — pass the XLM SAC address as `token_address`.

| Network | XLM SAC address |
|---|---|
| Testnet | `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC` |

Verify (or derive for another network) with:

```bash
stellar contract id asset --asset native --network testnet
```

Create an XLM stream (amounts are in **stroops**; 1 XLM = 10,000,000 stroops):

```bash
stellar contract invoke --id <STREAM_ID> --source <EMPLOYER_KEY> --network testnet \
  -- create_stream \
    --employer <EMPLOYER_ADDRESS> \
    --employee <EMPLOYEE_ADDRESS> \
    --token_address CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC \
    --deposit 100000000 \
    --rate_per_second 100 \
    --stop_time 0
```

An end-to-end testnet check is provided in `scripts/xlm-sac-test.sh` (create an XLM stream → wait → withdraw → verify balance → cancel).

### Edge cases

- **Fees are separate from the stream.** Transaction fees are always paid in XLM by the transaction source account and are never deducted from the stream deposit. An employer streaming XLM must hold `deposit + fees` in spendable XLM; an employee needs a small XLM balance to pay for `withdraw` calls.
- **SAC transfers are fee-free.** The native SAC does not charge a transfer fee, so the amount the employee receives equals the amount withdrawn.
- **Minimum balance (base reserve).** Classic accounts must keep a minimum XLM balance (currently 1 XLM base reserve plus 0.5 XLM per subentry). The SAC cannot transfer XLM that would drop the employer below this reserve, so `create_stream` / `top_up` fail if `deposit` exceeds the employer's *spendable* balance, not their total balance.
- **Recipient accounts must exist.** The native SAC cannot credit an unfunded classic account. The employee's account must be created (funded) before the first `withdraw`, and the employer's account must exist to receive refunds on `cancel_stream`.
- **No trustline needed.** Unlike issued assets, native XLM does not require a trustline on either account.
- **Contract-held XLM** sits in the PayStream contract's SAC balance, which is not subject to the account reserve.

---

## Useful Links

- Stellar Testnet Horizon: <https://horizon-testnet.stellar.org>
- Stellar Laboratory: <https://laboratory.stellar.org>
- Friendbot faucet: <https://friendbot.stellar.org>
- Soroban Testnet RPC: <https://soroban-testnet.stellar.org>
