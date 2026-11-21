// Shared helpers for talking to the PayStream stream contract from the browser
// with @stellar/stellar-sdk and signing through the Freighter wallet.
"use client";

import {
  Contract,
  Networks,
  TransactionBuilder,
  nativeToScVal,
  rpc,
  scValToNative,
  xdr,
  Address,
} from "@stellar/stellar-sdk";
import {
  getAddress,
  isConnected,
  requestAccess,
  signTransaction,
} from "@stellar/freighter-api";

const RPC_URL = process.env.NEXT_PUBLIC_RPC_URL || "https://soroban-testnet.stellar.org";
export const NETWORK_PASSPHRASE = Networks.TESTNET;

const CONTRACT_ID_ENV = "NEXT_PUBLIC_STREAM_CONTRACT_ID";

// StrKey contract addresses are base32 (RFC 4648, no padding) and start with
// "C"; the version byte + 32-byte payload + CRC16 checksum encode to 56 chars.
const CONTRACT_ID_RE = /^C[A-Z2-7]{55}$/;

/**
 * Validate a Stellar contract address (StrKey "C..." form).
 *
 * Throws a clear error naming NEXT_PUBLIC_STREAM_CONTRACT_ID when the value is
 * missing or malformed, so misconfiguration fails fast at startup/build time
 * instead of surfacing as an opaque runtime error.
 */
export function validateContractId(id: string | undefined): string {
  const value = (id ?? "").trim();
  if (!value) {
    throw new Error(
      `Missing required environment variable ${CONTRACT_ID_ENV}: set it to the deployed PayStream contract address (a Stellar contract ID starting with "C").`,
    );
  }
  if (!CONTRACT_ID_RE.test(value)) {
    throw new Error(
      `Invalid ${CONTRACT_ID_ENV} "${value}": expected a Stellar contract address (56 characters, starting with "C", base32).`,
    );
  }
  return value;
}

/**
 * Startup check: ensure the contract ID is configured and well-formed.
 * Call this once when the app boots so a bad env var fails loudly.
 */
export function assertContractIdConfigured(): string {
  return validateContractId(process.env.NEXT_PUBLIC_STREAM_CONTRACT_ID);
}

// Derived via the validator so an invalid value fails fast. The env var is
// inlined at build time by Next.js; during static prerender it may be unset,
// in which case we fall back to "" and defer the error to `assertContractIdConfigured`.
export const STREAM_CONTRACT_ID = process.env.NEXT_PUBLIC_STREAM_CONTRACT_ID
  ? validateContractId(process.env.NEXT_PUBLIC_STREAM_CONTRACT_ID)
  : "";

export const server = new rpc.Server(RPC_URL);
// Built lazily: `new Contract("")` throws, and this module is also evaluated
// during static prerender where the env var may be unset.
let _stream: Contract | null = null;
const stream = () => (_stream ??= new Contract(assertContractIdConfigured()));

// Argument helpers (mirrors docs/client-guide.md)
const addr = (a: string) => new Address(a).toScVal();
const u64 = (n: bigint) => nativeToScVal(n, { type: "u64" });
const scVec = (values: xdr.ScVal[]) => xdr.ScVal.scvVec(values);

/** Lifecycle state of a stream, as decoded from the contract's StreamStatus enum. */
export type StreamStatus = "Active" | "Paused" | "Cancelled" | "Exhausted";

/** A stream as returned by get_stream / get_streams_batch (token amounts in base units). */
export interface Stream {
  id: bigint;
  employer: string;
  employee: string;
  token: string;
  deposit: bigint;
  withdrawn: bigint;
  rate_per_second: bigint;
  start_time: bigint;
  stop_time: bigint;
  last_withdraw_time: bigint;
  status: string;
  pending_accrual: bigint;
  locked: boolean;
  low_balance_threshold: bigint;
}

/** A stream enriched with client-side values for the dashboard tables. */
export interface StreamRow extends Stream {
  status_label: StreamStatus;
  claimable: bigint;
  remaining: bigint;
}

// The ScVal for a unit enum variant decodes to its name (e.g. "Active"); fall
// back to a naive match so display never breaks on an unexpected encoding.
function normalizeStatus(raw: unknown): StreamStatus {
  const name = String(typeof raw === "object" && raw !== null ? Object.keys(raw)[0] : raw);
  const match = ["Active", "Paused", "Cancelled", "Exhausted"].find(
    (s) => s.toLowerCase() === name.toLowerCase(),
  );
  return (match as StreamStatus) || "Active";
}

/** Read-only contract call, simulated with the connected account as source. */
async function read(publicKey: string, method: string, ...args: xdr.ScVal[]) {
  const account = await server.getAccount(publicKey);
  const tx = new TransactionBuilder(account, {
    fee: "100",
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(stream().call(method, ...args))
    .setTimeout(30)
    .build();
  const sim = await server.simulateTransaction(tx);
  if (rpc.Api.isSimulationError(sim)) throw new Error(sim.error);
  return scValToNative(sim.result!.retval);
}

/**
 * Sign and submit a state-changing call through Freighter. Builds the
 * transaction, simulates it to attach the Soroban auth footprint, asks
 * Freighter to sign, then submits the signed envelope and waits for success.
 */
async function invoke(method: string, ...args: xdr.ScVal[]) {
  const { address } = await getAddress();
  const account = await server.getAccount(address);
  const tx = new TransactionBuilder(account, {
    fee: "100",
    networkPassphrase: NETWORK_PASSPHRASE,
  })
    .addOperation(stream().call(method, ...args))
    .setTimeout(30)
    .build();

  const prepared = await server.prepareTransaction(tx);
  const { signedTxXdr } = await signTransaction(prepared.toXDR(), {
    networkPassphrase: NETWORK_PASSPHRASE,
  });
  const signed = TransactionBuilder.fromXDR(signedTxXdr, NETWORK_PASSPHRASE);

  const sent = await server.sendTransaction(signed);
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

/** Fetch every stream owned by `employer`, chunked 50 at a time, with claimable amounts. */
export async function loadEmployerStreams(employer: string): Promise<StreamRow[]> {
  const ids: bigint[] = ((await read(employer, "streams_by_employer", addr(employer))) as bigint[]) || [];
  return hydrate(employer, ids);
}

/** Fetch every stream paying `employee`, chunked 50 at a time, with claimable amounts. */
export async function loadEmployeeStreams(employee: string): Promise<StreamRow[]> {
  const ids: bigint[] = ((await read(employee, "streams_by_employee", addr(employee))) as bigint[]) || [];
  return hydrate(employee, ids);
}

// Shared read path: get_streams_batch (chunks of 50) + claimable_batch, then
// derive remaining = deposit − withdrawn − claimable per the data model.
async function hydrate(source: string, ids: bigint[]): Promise<StreamRow[]> {
  const rows: StreamRow[] = [];
  for (let i = 0; i < ids.length; i += 50) {
    const chunk = ids.slice(i, i + 50);
    const streams = (await read(
      source,
      "get_streams_batch",
      scVec(chunk.map((id) => u64(id))),
    )) as Stream[];
    const claimables = (await read(
      source,
      "claimable_batch",
      scVec(chunk.map((id) => u64(id))),
    )) as bigint[];
    streams.forEach((s, j) => {
      const status_label = normalizeStatus(s.status);
      const claimable = status_label === "Active" ? claimables[j] : 0n;
      rows.push({ ...s, status_label, claimable, remaining: s.deposit - s.withdrawn - claimable });
    });
  }
  return rows;
}

/** Σ rate_per_second over the employer's Active streams (base units / second). */
export function burnRate(streams: StreamRow[]): bigint {
  return streams
    .filter((s) => s.status_label === "Active")
    .reduce((sum, s) => sum + s.rate_per_second, 0n);
}

/** Withdraw a single stream's claimable amount (employee signs). */
export const withdraw = (employee: string, streamId: bigint) =>
  invoke("withdraw", addr(employee), u64(streamId));

/** Withdraw every eligible stream in one transaction (employee signs). */
export const withdrawAll = (employee: string) =>
  invoke("withdraw_all", addr(employee)) as Promise<[bigint, bigint][]>;

// --- Wallet + formatting helpers ---

export async function ensureConnected(): Promise<string> {
  if (!(await isConnected()).isConnected) {
    const res = await requestAccess();
    if (!res.address) throw new Error("No account selected in Freighter");
  }
  const { address } = await getAddress();
  return address;
}

/** Format a base-unit amount for a 7-decimal Stellar asset. */
export function formatAmount(base: bigint, decimals = 7): string {
  const neg = base < 0n;
  const abs = (neg ? -base : base).toString().padStart(decimals + 1, "0");
  const int = abs.slice(0, -decimals) || "0";
  const frac = abs.slice(-decimals).replace(/0+$/, "");
  return `${neg ? "-" : ""}${int}${frac ? "." + frac : ""}`;
}

export const shortAddr = (a: string) => `${a.slice(0, 6)}…${a.slice(-4)}`;
