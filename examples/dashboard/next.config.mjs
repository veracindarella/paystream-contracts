/** @type {import('next').NextConfig} */
const CONTRACT_ID_ENV_VAR = 'NEXT_PUBLIC_STREAM_CONTRACT_ID';

// Stellar contract addresses are StrKey-encoded: a leading "C" followed by 55
// base32 characters (A-Z, 2-7), for a total length of 56.
const STELLAR_CONTRACT_ID_REGEX = /^C[A-Z2-7]{55}$/;

function isValidContractId(value) {
  return typeof value === 'string' && STELLAR_CONTRACT_ID_REGEX.test(value);
}

// Fail the production build loudly if the contract ID is missing or malformed,
// so misconfiguration cannot slip through to runtime.
if (process.env.NODE_ENV === 'production') {
  const contractId = process.env[CONTRACT_ID_ENV_VAR];

  if (!contractId) {
    throw new Error(
      `Missing required environment variable ${CONTRACT_ID_ENV_VAR}. ` +
        `Set it to a valid Stellar contract address (e.g. C... 56 characters) before running a production build.`,
    );
  }

  if (!isValidContractId(contractId)) {
    throw new Error(
      `Invalid ${CONTRACT_ID_ENV_VAR}: "${contractId}". ` +
        `Expected a valid Stellar contract address: a leading "C" followed by 55 base32 characters (A-Z, 2-7), 56 characters total.`,
    );
  }
}

const nextConfig = {
  reactStrictMode: true,
  // @stellar/stellar-sdk pulls in a few Node built-ins; disable them in the browser bundle.
  webpack: (config) => {
    config.resolve.fallback = { ...config.resolve.fallback, fs: false, net: false, tls: false };
    return config;
  },
};

export default nextConfig;