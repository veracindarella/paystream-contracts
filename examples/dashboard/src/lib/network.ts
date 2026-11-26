import { Networks } from '@stellar/stellar-sdk';

/**
 * Shared, env-driven network configuration for the dashboard.
 *
 * The active network is selected via `NEXT_PUBLIC_NETWORK` (defaults to
 * `testnet`). Per-network defaults can be overridden via environment
 * variables. At module load we validate that the configured passphrase
 * matches the selected network and that the stream contract ID is a valid
 * Stellar contract address for that network. On mismatch we throw a clear
 * error so the app refuses to start.
 */

export type NetworkName = 'testnet' | 'mainnet';

export interface NetworkConfig {
  /** Stellar network passphrase. */
  passphrase: string;
  /** Default Soroban RPC URL for the network. */
  rpcUrl: string;
  /** Expected stream contract ID for the network (may be empty if unset). */
  streamContractId: string;
}

export const NETWORKS: Record<NetworkName, NetworkConfig> = {
  testnet: {
    passphrase: Networks.TESTNET,
    rpcUrl: 'https://soroban-testnet.stellar.org',
    streamContractId: '',
  },
  mainnet: {
    passphrase: Networks.PUBLIC,
    rpcUrl: 'https://soroban-mainnet.stellar.org',
    streamContractId: '',
  },
};

const VALID_NETWORKS: NetworkName[] = ['testnet', 'mainnet'];

function resolveNetworkName(raw: string | undefined): NetworkName {
  const value = (raw ?? '').trim().toLowerCase();
  if (!value) {
    return 'testnet';
  }
  if (!VALID_NETWORKS.includes(value as NetworkName)) {
    throw new Error(
      `[network] Invalid NEXT_PUBLIC_NETWORK "${raw}". Expected one of: ${VALID_NETWORKS.join(
        ', ',
      )}.`,
    );
  }
  return value as NetworkName;
}

/**
 * Validate that a value looks like a Stellar contract (strkey) address:
 * starts with `C` and is 56 characters long.
 */
export function isValidContractId(value: string): boolean {
  return /^C[A-Z2-7]{55}$/.test(value);
}

const networkName: NetworkName = resolveNetworkName(
  process.env.NEXT_PUBLIC_NETWORK,
);

const networkConfig: NetworkConfig = NETWORKS[networkName];

const configuredPassphrase =
  process.env.NEXT_PUBLIC_NETWORK_PASSPHRASE?.trim() || networkConfig.passphrase;

if (configuredPassphrase !== networkConfig.passphrase) {
  throw new Error(
    `[network] Passphrase mismatch for network "${networkName}". ` +
      `Expected "${networkConfig.passphrase}" but got "${configuredPassphrase}". ` +
      `Check NEXT_PUBLIC_NETWORK_PASSPHRASE.`,
  );
}

const configuredRpcUrl =
  process.env.NEXT_PUBLIC_RPC_URL?.trim() || networkConfig.rpcUrl;

const configuredStreamContractId =
  process.env.NEXT_PUBLIC_STREAM_CONTRACT_ID?.trim() ||
  networkConfig.streamContractId;

if (configuredStreamContractId && !isValidContractId(configuredStreamContractId)) {
  throw new Error(
    `[network] Invalid stream contract ID "${configuredStreamContractId}" for network "${networkName}". ` +
      `Expected a 56-character Stellar contract address starting with "C". ` +
      `Check NEXT_PUBLIC_STREAM_CONTRACT_ID.`,
  );
}

/** Resolved active network name. */
export const NETWORK: NetworkName = networkName;

/** Resolved network passphrase for the active network. */
export const NETWORK_PASSPHRASE: string = configuredPassphrase;

/** Resolved Soroban RPC URL for the active network. */
export const RPC_URL: string = configuredRpcUrl;

/** Resolved stream contract ID for the active network. */
export const STREAM_CONTRACT_ID: string = configuredStreamContractId;