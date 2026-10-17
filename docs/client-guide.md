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
import { Buffer } from "node:buffer";

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
const u32 = (n: number) => nativeToScVal(n, { type: "u32" });
const scVec = (values: xdr.ScVal[]) => xdr.ScVal.scvVec(values);
const scStruct = (fields: Record<string, xdr.ScVal>) =>
  xdr.ScVal.scvMap(
    Object.entries(fields).map(
      ([key, val]) =>
        new xdr.ScMapEntry({ key: xdr.ScVal.scvSymbol(key), val }),
    ),
  );
```

Fund a testnet account with Friendbot (`https://friendbot.stellar.org?addr=<PUBLIC_KEY>`) before sending transactions.

## Helpers

### Invoke (state-changing call)

Builds the transaction, simulates it to attach Soroban resources and auth, signs, submits, and waits for the result.

```ts
async function invokeContract(
  signer: Keypair,
  target: Contract,
  method: string,
  ...args: xdr.ScVal[]
) {
  const account = await server.getAccount(signer.publicKey());
  const tx = new TransactionBuilder(account, {
    fee: BASE_FEE,
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(target.call(method, ...args))
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

async function invoke(signer: Keypair, method: string, ...args: xdr.ScVal[]) {
  return invokeContract(signer, stream, method, ...args);
}

async function approveToken(
  signer: Keypair,
  tokenContractId: string,
  amount: bigint,
  expirationLedger: number,
) {
  const token = new Contract(tokenContractId);
  return invokeContract(
    signer,
    token,
    "approve",
    addr(signer.publicKey()),
    addr(STREAM_CONTRACT_ID),
    i128(amount),
    u32(expirationLedger),
  );
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

### create_streams_batch

`StreamParams` is a Soroban struct, encoded here as a map with symbol keys. The call is atomic and accepts 1–50 streams. Ensure the employer has approved each token contract for the total deposit before calling.

```ts
const batchParams = scVec([
  scStruct({
    employee: addr(process.env.EMPLOYEE_1_ADDRESS!),
    token: addr(TOKEN_CONTRACT_ID),
    deposit: i128(500_0000000n),
    rate_per_second: i128(5_000n),
    stop_time: u64(0n),
  }),
  scStruct({
    employee: addr(process.env.EMPLOYEE_2_ADDRESS!),
    token: addr(TOKEN_CONTRACT_ID),
    deposit: i128(750_0000000n),
    rate_per_second: i128(7_500n),
    stop_time: u64(0n),
  }),
]);

try {
  const latestLedger = await server.getLatestLedger();
  await approveToken(
    employer,
    TOKEN_CONTRACT_ID,
    1_250_0000000n,
    latestLedger.sequence + 100_000,
  );
  const ids = await invoke(
    employer,
    "create_streams_batch",
    addr(employer.publicKey()),
    batchParams,
  ) as bigint[];
  console.log("Created stream IDs:", ids);
} catch (error) {
  console.error("create_streams_batch failed:", error);
  throw error;
}
```

### cancel_streams_batch

Cancels every listed stream atomically. The employer must own all of them.

```ts
const streamIds = [12n, 13n];

try {
  await invoke(
    employer,
    "cancel_streams_batch",
    addr(employer.publicKey()),
    scVec(streamIds.map(u64)),
  );
} catch (error) {
  console.error("cancel_streams_batch failed:", error);
  throw error;
}
```

### withdraw_all

Withdraws nonzero claimable amounts across the employee's eligible streams in one transaction. The return value is a list of `[streamId, amount]` pairs.

```ts
try {
  const withdrawals = await invoke(
    employee,
    "withdraw_all",
    addr(employee.publicKey()),
  ) as [bigint, bigint][];
  console.log("Withdrawn by stream:", withdrawals);
} catch (error) {
  console.error("withdraw_all failed:", error);
  throw error;
}
```

### update_rate

Only the employer can change the rate. Accrued earnings at the old rate remain claimable.

```ts
try {
  await invoke(
    employer,
    "update_rate",
    addr(employer.publicKey()),
    u64(streamId),
    i128(20_000n),
  );
} catch (error) {
  console.error("update_rate failed:", error);
  throw error;
}
```

### set_auto_topup

Approve the stream contract as token spender before configuring auto top-up. The allowance should cover the configured `max_total`; its expiration is expressed as a ledger sequence.

```ts
const maxTotal = 100_0000000n;

try {
  const latestLedger = await server.getLatestLedger();
  await approveToken(
    employer,
    TOKEN_CONTRACT_ID,
    maxTotal,
    latestLedger.sequence + 100_000,
  );
} catch (error) {
  console.error("Token allowance approval failed:", error);
  throw error;
}

try {
  await invoke(
    employer,
    "set_auto_topup",
    addr(employer.publicKey()),
    u64(streamId),
    i128(20_0000000n), // top up when remaining deposit falls below 20 tokens
    i128(10_0000000n), // transfer 10 tokens per top-up
    i128(maxTotal),
  );
} catch (error) {
  console.error("set_auto_topup failed:", error);
  throw error;
}
```

### propose_admin / accept_admin

The current admin proposes the transfer with its current nonce. The new admin must accept using the same nonce and sign the second transaction.

```ts
const currentAdmin = Keypair.fromSecret(process.env.ADMIN_SECRET!);
const nextAdmin = Keypair.fromSecret(process.env.NEXT_ADMIN_SECRET!);

try {
  const nonce = await read(currentAdmin.publicKey(), "admin_nonce") as bigint;
  await invoke(currentAdmin, "propose_admin", addr(nextAdmin.publicKey()), u64(nonce));
  await invoke(nextAdmin, "accept_admin", addr(nextAdmin.publicKey()), u64(nonce));
} catch (error) {
  console.error("Admin transfer failed:", error);
  throw error;
}
```

### Upgrade the contract

Build and upload the new WASM first; `stellar contract upload` prints the 32-byte WASM hash. The contract's time-locked flow requires 48 hours between proposal and execution. Do not run this against production as a test.

```bash
stellar contract build
stellar contract upload \
  --source-account <ADMIN_IDENTITY> \
  --wasm target/wasm32v1-none/release/paystream_stream.wasm \
  --network testnet
# Save the printed hash as NEW_WASM_HASH.
```

```ts
const admin = Keypair.fromSecret(process.env.ADMIN_SECRET!);
const wasmHashBytes = Buffer.from(
  process.env.NEW_WASM_HASH!.replace(/^0x/, ""),
  "hex",
);
if (wasmHashBytes.length !== 32) throw new Error("NEW_WASM_HASH must be 32 bytes");
const wasmHash = nativeToScVal(wasmHashBytes, { type: "bytes" });

try {
  const nonce = await read(admin.publicKey(), "admin_nonce") as bigint;
  await invoke(admin, "propose_upgrade", wasmHash, u64(nonce));
} catch (error) {
  console.error("propose_upgrade failed:", error);
  throw error;
}

// Wait until the 48-hour time-lock has elapsed before executing.
try {
  const nonce = await read(admin.publicKey(), "admin_nonce") as bigint;
  await invoke(admin, "execute_upgrade", u64(nonce));
} catch (error) {
  console.error("execute_upgrade failed:", error);
  throw error;
}

try {
  await invoke(admin, "migrate", addr(admin.publicKey()));
  const version = await read(admin.publicKey(), "version") as number;
  console.log("Upgraded contract version:", version);
} catch (error) {
  console.error("Post-upgrade migration or verification failed:", error);
  throw error;
}
```

The transaction helper simulates before submission and throws on failed submission or execution. Check the error code/message in the thrown error and compare it with [api-reference.md](api-reference.md). For build, upload, and migration details, see [upgrade-guide.md](upgrade-guide.md).

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
