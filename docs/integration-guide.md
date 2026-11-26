# Integration Partner Onboarding Guide

This guide is for third-party integrators building on top of the streaming payments contract. It covers how to read events, call read-only queries, submit transactions safely, and handle errors. By the end you should be able to build a working integration that observes streams, queries their state, and submits transactions with correct auth and fees.

> **Status:** This guide must be reviewed by a partner before issue #538 (DOC-39) is closed.

## Table of contents

1. [Prerequisites and related docs](#1-prerequisites-and-related-docs)
2. [Read patterns](#2-read-patterns)
   - [Polling events](#21-polling-events)
   - [Read-only queries via `simulateTransaction`](#22-read-only-queries-via-simulatetransaction)
3. [Auth](#3-auth)
4. [Fee estimation](#4-fee-estimation)
5. [Error handling](#5-error-handling)
6. [Example app](#6-example-app)

---

## 1. Prerequisites and related docs

Before you start, read these companion documents:

- **[SDK (BE-03)](../sdk/README.md)** — the official client SDK. It wraps the RPC calls, auth assembly, and fee logic described here. Prefer the SDK over hand-rolling RPC calls.
- **[Events schema](./events-schema.md)** — the v1 topic layout, event payloads, and indexer compatibility notes. Essential for [polling events](#21-polling-events).
- **[API reference](./api-reference.md)** — contract entrypoints, argument types, and the **E-code table** used in [error handling](#5-error-handling).
- **[Client guide](./client-guide.md)** — end-user-facing flows (creating streams, claiming, cancelling).
- **[Quickstart](./quickstart.md)** — get a local environment and a deployed contract running quickly.

You will need:

- A Soroban RPC endpoint (testnet or mainnet).
- The contract ID of the deployed streaming payments contract.
- A funded Stellar account (keypair or wallet) for submitting transactions.
- The SDK installed (see BE-03) or a Stellar SDK capable of building Soroban transactions.

---

## 2. Read patterns

Integrations read from the contract in two ways:

1. **Events** — a historical/streaming log of state changes (stream created, withdrawn, cancelled, etc.). Use these to keep an off-chain index in sync.
2. **Read-only queries** — point-in-time state (a stream's balance, an employer's streams, claimable amounts). Use these to render current state and to validate before submitting transactions.

### 2.1 Polling events

Events are retrieved with the Soroban RPC `getEvents` method. You filter by the contract ID and by **topics**. The v1 topic layout is defined in **[docs/events-schema.md](./events-schema.md)** — read it first, because topic ordering and symbol names are part of the compatibility contract for indexers.

A typical poll loop:

1. Track a `startLedger` cursor (the last ledger you successfully processed).
2. Call `getEvents` with `startLedger`, `filters` (contract ID + topic filters), and a `limit`.
3. Process returned events in order.
4. Advance `startLedger` to the latest processed ledger and repeat on an interval.

```ts
import { SorobanRpc, scValToNative, xdr } from "@stellar/stellar-sdk";

const server = new SorobanRpc.Server(RPC_URL);

// Topic filter: match the contract and the event's first topic (the event name).
// See docs/events-schema.md for the full v1 topic layout.
const filters = [
  {
    type: "contract",
    contractIds: [CONTRACT_ID],
    topics: [
      // e.g. match any event whose first topic is the "stream" symbol.
      // Adjust to the exact topic layout documented in events-schema.md.
      [xdr.ScVal.scvSymbol("stream").toXDR("base64") + "*"],
    ],
  },
];

async function pollEvents(startLedger: number) {
  const res = await server.getEvents({
    startLedger,
    filters,
    limit: 100,
  });

  for (const event of res.events) {
    const topics = event.topic.map((t) => scValToNative(t));
    const value = scValToNative(event.value);
    handleEvent(topics, value, event.ledger);
  }

  const latest = res.latestLedger;
  return latest;
}
```

**Indexer compatibility notes** (see [events-schema.md](./events-schema.md) for details):

- Treat the **first topic** as the event name/type and the remaining topics as indexed keys (e.g. stream ID, employer, employee). Do not assume a fixed number of topics beyond what the schema documents.
- Event payloads are versioned. Record the schema version you built against so you can migrate when the layout changes.
- Events are only retained for a bounded ledger window on RPC nodes. For long-lived indexes, persist your cursor and backfill from a history archive or your own event store if you fall behind.
- Process events **idempotently** (key by transaction hash + event index) so re-polling after a restart does not double-count.

### 2.2 Read-only queries via `simulateTransaction`

Read-only contract functions do not require a submitted transaction. Build the invocation, then call `simulateTransaction` and decode the return value from the simulation result. This is free (no fee, no ledger write) and is the correct way to read current state.

The read-only entrypoints are:

| Function | Purpose |
| --- | --- |
| `get_stream` | Fetch a single stream by ID. |
| `get_streams_batch` | Fetch multiple streams by ID in one call. |
| `claimable` | Compute the currently claimable amount for a stream (or recipient). |
| `claimable_batch` | Compute claimable amounts for multiple streams in one call. |
| `streams_by_employer` | List stream IDs created by an employer. |
| `streams_by_employee` | List stream IDs paying an employee. |

```ts
import { Contract, TransactionBuilder, SorobanRpc, scValToNative, xdr, Account } from "@stellar/stellar-sdk";

const server = new SorobanRpc.Server(RPC_URL);
const contract = new Contract(CONTRACT_ID);

async function readStream(streamId: bigint) {
  // A dummy source account is fine for simulation; it is never charged.
  const source = new Account(DUMMY_PUBLIC_KEY, "0");

  const tx = new TransactionBuilder(source, {
    fee: "100", // BASE_FEE; irrelevant for simulation but required by the builder
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(contract.call("get_stream", xdr.ScVal.scvU64(streamId)))
    .setTimeout(30)
    .build();

  const sim = await server.simulateTransaction(tx);
  if (SorobanRpc.Api.isSimulationError(sim)) {
    throw new Error(`simulation failed: ${sim.error}`);
  }

  // The return value is the last entry in the results.
  const retval = sim.result!.retval;
  return scValToNative(retval);
}
```

**Batch reads.** Prefer `get_streams_batch` and `claimable_batch` over N individual calls when rendering a list. One simulation returns all results, which reduces round-trips and RPC load. See [fee estimation](#4-fee-estimation) for how batching also affects submitted-transaction fees.

**Indexing reads.** `streams_by_employer` and `streams_by_employee` return stream IDs. Combine them with `get_streams_batch` to hydrate full records, or maintain your own index from [events](#21-polling-events) and use these queries to reconcile.

---

## 3. Auth

Soroban uses **authorization entries** attached to a transaction to prove that an address approved a contract invocation. The streaming contract enforces role-based access:

- **Employer** — creates streams, funds them, and can cancel (subject to contract rules).
- **Employee** — withdraws/claims accrued funds.
- **Admin** — contract-level administration (e.g. configuration), where applicable.

### How auth entries are attached

You generally do **not** hand-assemble auth entries. Build the invocation, then call `prepareTransaction`, which simulates the transaction, discovers the required auth entries, and attaches them:

```ts
import { SorobanRpc, TransactionBuilder, Contract, xdr } from "@stellar/stellar-sdk";

const server = new SorobanRpc.Server(RPC_URL);
const contract = new Contract(CONTRACT_ID);

async function buildWithdraw(sourceAccount: Account, streamId: bigint) {
  const tx = new TransactionBuilder(sourceAccount, {
    fee: "100",
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(contract.call("withdraw", xdr.ScVal.scvU64(streamId)))
    .setTimeout(30)
    .build();

  // Simulates, sets the resource fee, and attaches required auth entries.
  return await server.prepareTransaction(tx);
}
```

`prepareTransaction` returns a transaction whose auth entries are populated but **unsigned**. The entries reference the address that must sign (the employee for `withdraw`, the employer for create/cancel).

### Wallet signing

After `prepareTransaction`, sign with the wallet that controls the required role:

- **Local keypair:** `tx.sign(keypair)`.
- **Browser wallet:** pass the prepared transaction's XDR to the wallet for signing, then rebuild from the signed XDR.

```ts
const prepared = await buildWithdraw(sourceAccount, streamId);
prepared.sign(employeeKeypair); // employee authorizes the withdrawal
const result = await server.sendTransaction(prepared);
```

**Rules of thumb:**

- The signer must match the address in the auth entry, or the transaction fails at submission.
- Never sign a transaction you did not build and simulate yourself; verify the contract ID and operation before signing.
- For multi-party flows (e.g. an employer creating a stream on behalf of an employee), ensure every required auth entry is signed by the correct party.

---

## 4. Fee estimation

Soroban transaction fees have two parts:

1. **Inclusion fee** — `BASE_FEE` per operation (currently `100` stroops per operation in the examples above). This is the classic Stellar fee.
2. **Resource fee** — the cost of CPU instructions, ledger reads/writes, and bandwidth consumed by the invocation. This is **derived from simulation**.

### Getting the resource fee

`simulateTransaction` returns the resource fee in its result. `prepareTransaction` applies it to the transaction's fee automatically. If you build transactions manually, read `sim.minResourceFee` from the simulation and set the fee to `BASE_FEE * opCount + minResourceFee` (plus a small buffer).

```ts
const sim = await server.simulateTransaction(tx);
if (SorobanRpc.Api.isSimulationError(sim)) throw new Error(sim.error);

const inclusionFee = 100n * BigInt(tx.operations.length);
const totalFee = inclusionFee + BigInt(sim.minResourceFee);
// Apply totalFee (with a buffer) when rebuilding the transaction.
```

### Batch vs. individual fee comparison

Batching reduces **per-item** cost because fixed overheads (base inclusion fee, some ledger reads) are amortized:

| Approach | Inclusion fee | Resource fee | Notes |
| --- | --- | --- | --- |
| N individual transactions | `N × BASE_FEE` | Sum of N simulations | Highest total; N round-trips and N signatures. |
| One batch transaction | `1 × BASE_FEE` (per op) | Single simulation covering all items | Lower total; one signature; watch the per-transaction size/instruction limits. |

Guidance:

- Use `get_streams_batch` / `claimable_batch` for **reads** — always cheaper than N simulations.
- For **writes**, batch where the contract exposes a batch entrypoint and the batch fits within transaction resource limits. If a batch would exceed limits, split it.
- Always simulate the exact batch you intend to submit; resource fees are not linear and must be measured, not guessed.

---

## 5. Error handling

Errors surface at two stages: **simulation** and **submission**. Handle both.

### Simulation errors

`simulateTransaction` returns an error when the invocation would fail (bad arguments, failed auth, contract panic, insufficient balance, etc.). Check with `SorobanRpc.Api.isSimulationError(sim)` and inspect `sim.error`. Do not submit a transaction that failed simulation.

### Submission errors

`sendTransaction` can fail for reasons unrelated to contract logic: stale sequence numbers, insufficient fee, expired time bounds, or a transaction that was valid at simulation but failed on-chain. Inspect the response `status`:

- `PENDING` / `DUPLICATE` — accepted; poll `getTransaction` for the final result.
- `ERROR` — rejected; inspect `errorResult` (an `xdr.TransactionResult`) for the failure reason.
- `TRY_AGAIN_LATER` — transient; retry with backoff.

After submission, poll `getTransaction(hash)` until `status` is `SUCCESS` or `FAILED`. On `FAILED`, decode `resultMeta`/`resultXdr` to find the contract error.

### Contract error codes (E-codes)

When the contract itself rejects a call, it returns a numeric error code. The authoritative list is the **E-code table in [docs/api-reference.md](./api-reference.md)**. Map codes to user-facing messages there rather than hard-coding strings in your integration, so you stay in sync as codes are added.

```ts
function describeContractError(code: number): string {
  // Look up `code` in the E-code table in docs/api-reference.md.
  return ERROR_TABLE[code] ?? `Unknown contract error (code ${code})`;
}
```

**Best practices:**

- Validate inputs client-side (e.g. stream exists, sufficient balance) before simulating, to avoid predictable failures.
- Retry transient submission errors with exponential backoff; never retry deterministic contract errors.
- Log the transaction hash, simulation error, and decoded result for support and debugging.

---

## 6. Example app

A complete, working integration lives in **[`examples/dashboard`](../examples/dashboard)**. It demonstrates:

- Polling events with topic filters and maintaining an index cursor.
- Reading streams via `simulateTransaction` (`get_stream`, `get_streams_batch`, `claimable`).
- Building, preparing (auth + fees), signing, and submitting transactions.
- Mapping contract E-codes to user-facing messages.

Use it as the reference implementation when wiring up your own integration, and cross-check behavior against the [SDK (BE-03)](../sdk/README.md).

---

## Review checklist (before closing #538)

- [ ] Guide reviewed by a partner integrator.
- [ ] Links to SDK (BE-03), [events schema](./events-schema.md), [API reference](./api-reference.md), [client guide](./client-guide.md), and [quickstart](./quickstart.md) verified.
- [ ] Example app in `examples/dashboard` runs end-to-end against testnet.