# Client Guide (JavaScript / TypeScript)

How to call the PayStream stream contract from a frontend or Node.js script with [`@stellar/stellar-sdk`](https://github.com/stellar/js-stellar-sdk). Full parameter and error documentation lives in [api-reference.md](api-reference.md).

## Setup

```bash
npm install @stellar/stellar-sdk
```

```ts
import {
  Address,
  BASE_FEE,
  Contract,
  Keypair,
  Networks,
  TransactionBuilder,
  nativeToScVal,
  rpc,
  scValToNative,
  xdr,
} from "@stellar/stellar-sdk";

const RPC_URL = "https://soroban-testnet.stellar.org";
const NETWORK_PASSPHRASE = Networks.TESTNET;
const STREAM_CONTRACT_ID = process.env.STREAM_CONTRACT_ID!; // from ./scripts/deploy-testnet.sh
const TOKEN_CONTRACT_ID = process.env.TOKEN_CONTRACT_ID!;   // any SEP-41 token / SAC

const server = new rpc.Server(RPC_URL);
const stream = new Contract(STREAM_CONTRACT_ID);

// Argument helpers
const addr = (a: string) => new Address(a).toScVal();
const i128 = (n: bigint) => nativeToScVal(n, { type: "i128" });
const u64 = (n: bigint) => nativeToScVal(n, { type: "u64" });
```

Fund a testnet account with Friendbot (`https://friendbot.stellar.org?addr=<PUBLIC_KEY>`) before sending transactions.

## Helpers

### Invoke (state-changing call)

Builds the transaction, simulates it to attach Soroban resources and auth, signs, submits, and waits for the result.

```ts
async function invoke(signer: Keypair, method: string, ...args: xdr.ScVal[]) {
  const account = await server.getAccount(signer.publicKey());
  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(stream.call(method, ...args))
    .setTimeout(30)
    .build();

  const prepared = await server.prepareTransaction(tx); // simulate + attach footprint/auth
  prepared.sign(signer);

  const sent = await server.sendTransaction(prepared);
  if (sent.status === "ERROR") {
    throw new Error(`Submission failed: ${JSON.stringify(sent.errorResult)}`);
  }

  let res = await server.getTransaction(sent.hash);
  while (res.status === rpc.Api.GetTransactionStatus.NOT_FOUND) {
    await new Promise((r) => setTimeout(r, 1000));
    res = await server.getTransaction(sent.hash);
  }
  if (res.status !== rpc.Api.GetTransactionStatus.SUCCESS) {
    throw new Error(`Transaction failed: ${sent.hash}`);
  }
  return res.returnValue ? scValToNative(res.returnValue) : undefined;
}
```

### Read (simulation only, no fee)

```ts
async function read(sourcePublicKey: string, method: string, ...args: xdr.ScVal[]) {
  const account = await server.getAccount(sourcePublicKey);
  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(stream.call(method, ...args))
    .setTimeout(30)
    .build();

  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
  return scValToNative(sim.result!.retval);
}
```

## Operations

Amounts are `i128` values in the token's base units (7 decimals for Stellar assets: `1 unit = 10_000_000n` stroops).

### initialize

One-time setup; the admin must sign.

```ts
const admin = Keypair.fromSecret(process.env.ADMIN_SECRET!);
await invoke(admin, "initialize", addr(admin.publicKey()));
```

### create_stream

The employer signs and the deposit is transferred into the contract. Returns the new stream id (`u64`, decoded as `bigint`).

```ts
const employer = Keypair.fromSecret(process.env.EMPLOYER_SECRET!);
const employeeAddress = "G...";

const streamId: bigint = await invoke(
  employer,
  "create_stream",
  addr(employer.publicKey()),  // employer
  addr(employeeAddress),       // employee
  addr(TOKEN_CONTRACT_ID),     // token
  i128(1_000_0000000n),        // deposit: 1,000 tokens
  i128(10_000n),               // rate_per_second
  u64(0n),                     // stop_time: 0 = no hard stop
);
```

### claimable

Read-only; anyone can query.

```ts
const amount: bigint = await read(employer.publicKey(), "claimable", u64(streamId));
```

### withdraw

The employee signs. Returns the amount withdrawn.

```ts
const employee = Keypair.fromSecret(process.env.EMPLOYEE_SECRET!);
const withdrawn: bigint = await invoke(
  employee,
  "withdraw",
  addr(employee.publicKey()),
  u64(streamId),
);
```

### cancel_stream

The employer signs. The employee receives the earned share and the remainder is refunded to the employer.

```ts
await invoke(employer, "cancel_stream", addr(employer.publicKey()), u64(streamId));
```

### get_stream

```ts
const s = await read(employer.publicKey(), "get_stream", u64(streamId));
// { id, employer, employee, token, deposit, withdrawn, rate_per_second,
//   start_time, stop_time, last_withdraw_time, status, locked }
```

## Error handling

Contract panics surface as a failed simulation (`prepareTransaction` / `simulateTransaction` throws) whose message contains the contract error code, e.g. `E008` for a rate above the maximum. See the error table in [api-reference.md](api-reference.md). Common causes:

- missing signature — the address passed as `employer` / `employee` must be the signer
- insufficient token balance or allowance for the deposit
- contract paused by the admin
- withdrawing from a cancelled or exhausted stream

## Wallets (browser)

In a dApp, replace `prepared.sign(signer)` with your wallet's signing call (e.g. Freighter's `signTransaction(prepared.toXDR(), { networkPassphrase })`) and rebuild the transaction with `TransactionBuilder.fromXDR(signedXdr, NETWORK_PASSPHRASE)` before submitting.
