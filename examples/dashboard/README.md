# PayStream Dashboard (reference implementation)

A minimal [Next.js](https://nextjs.org) + React dashboard that reads PayStream
salary streams from **Stellar** (testnet or mainnet, selected by configuration)
and lets an employee withdraw. It is a working integration example for the data
model in
[`docs/dashboard-data-model.md`](../../docs/dashboard-data-model.md) and the call
patterns in [`docs/client-guide.md`](../../docs/client-guide.md) — it is kept
separate from the `contracts/` workspace and is not part of the Rust build, but
it does have its own CI workflow (see [CI](#ci) below).

## Features

- **Freighter wallet** connection (`@stellar/freighter-api`).
- **Employer view** — stream list with status, claimable, and burn rate, plus
  overview cards (total claimable / remaining / Σ burn rate over Active streams).
- **Employee view** — stream list with a **Withdraw** button per stream and a
  **Withdraw all** action (`withdraw` / `withdraw_all`).
- Reads via `streams_by_employer` / `streams_by_employee` → `get_streams_batch`
  (chunks of 50) → `claimable_batch`, exactly the recommended read path.

All token amounts are shown for 7-decimal Stellar assets (e.g. XLM stroops).

## Prerequisites

1. A deployed PayStream stream contract on the network you intend to use. For
   testnet, from the repo root:

   ```bash
   ./scripts/deploy-testnet.sh   # note the printed stream contract id
   ./scripts/init-testnet.sh     # initialize the contract (admin)
   ```

   For mainnet, deploy and initialize the contract with your own tooling and
   record the resulting contract id.

2. The [Freighter browser extension](https://www.freighter.app/) installed and
   set to the same network as the dashboard.
3. A funded account on that network, and some streams created against it so the
   dashboard has something to show. On testnet you can fund an account with
   friendbot (`https://friendbot.stellar.org?addr=<PUBLIC_KEY>`). You can create
   streams with the snippets in [`docs/client-guide.md`](../../docs/client-guide.md).

## Setup

```bash
cd examples/dashboard
cp .env.example .env.local   # then set the network, RPC URL and contract id
npm install
npm run dev                  # http://localhost:3000
```

`.env.local`:

| Variable | Description |
|---|---|
| `NEXT_PUBLIC_NETWORK` | Network to target: `testnet` (default) or `mainnet`. |
| `NEXT_PUBLIC_STREAM_CONTRACT_ID` | Stream contract id deployed on the selected network. |
| `NEXT_PUBLIC_RPC_URL` | Soroban RPC endpoint for the selected network (defaults to the network's public RPC). |

### Switching networks

The dashboard is network-agnostic: set `NEXT_PUBLIC_NETWORK` to `testnet` or
`mainnet` and provide the matching RPC URL and stream contract id. No code
changes are required.

| Network | `NEXT_PUBLIC_NETWORK` | `NEXT_PUBLIC_RPC_URL` | `NEXT_PUBLIC_STREAM_CONTRACT_ID` |
|---|---|---|---|
| Testnet | `testnet` | `https://soroban-testnet.stellar.org` | id printed by `deploy-testnet.sh` |
| Mainnet | `mainnet` | `https://soroban-mainnet.stellar.org` | id of your mainnet deployment |

At startup the dashboard validates the configuration against the selected
network: the RPC endpoint's network passphrase must match `NEXT_PUBLIC_NETWORK`,
and the stream contract id must be a valid contract id for that network. On a
passphrase or contract-id mismatch the app refuses to start and reports the
error, so a testnet contract id can never be used against mainnet (or vice
versa). Make sure Freighter is also set to the same network.

### Contract ID validation

`NEXT_PUBLIC_STREAM_CONTRACT_ID` is validated so a misconfiguration cannot go
unnoticed:

- **Build time** — `npm run build` (i.e. `next build`) **fails** in production
  builds when `NEXT_PUBLIC_STREAM_CONTRACT_ID` is missing or is not a valid
  Stellar contract address. The error message names the required env var.
- **Startup** — when the app starts, the contract ID format is checked again
  (valid Stellar contract address: `C...`, 56 characters). An invalid value
  produces a clear error that names `NEXT_PUBLIC_STREAM_CONTRACT_ID`.

## Usage

1. Open the app and click **Connect Freighter**, selecting the account you funded.
2. The **Employer** tab lists streams where the connected account is the employer;
   the **Employee** tab lists streams where it is the employee. Use the same
   account for both if you created a stream between two of your own test accounts,
   or switch accounts in Freighter to see each side.
3. In the **Employee** tab, click **Withdraw** on a stream (or **Withdraw all**) to
   claim accrued tokens. The transaction is simulated, signed by Freighter, and
   submitted to the configured network; the list refreshes on success.

## Scripts

| Command | Description |
|---|---|
| `npm run dev` | Start the dev server. |
| `npm run build` | Production build (`next build`). |
| `npm run start` | Serve the production build. |
| `npm run lint` | ESLint check. |
| `npm run typecheck` | TypeScript check (`tsc --noEmit`). |
| `npm run test:e2e` | End-to-end suite against the testnet sandbox. |

## CI

The dashboard has its own GitHub Actions workflow,
[`dashboard-ci.yml`](../../.github/workflows/dashboard-ci.yml), which runs on
pull requests and pushes that touch `examples/dashboard`. It has two jobs:

- **build** — installs dependencies, then runs `npm run typecheck`,
  `npm run lint`, and `npm run build` (`next build`). Type or build errors fail
  the PR.
- **e2e** — reuses the same testnet sandbox setup as the contract e2e tests
  (deploy + init scripts) and runs `npm run test:e2e` against it.

Both jobs cache npm dependencies (keyed on `examples/dashboard/package-lock.json`),
so repeat runs skip the full install. Run the same commands locally before
opening a PR:

```bash
cd examples/dashboard
npm ci
npm run typecheck
npm run lint
npm run build
npm run test:e2e   # requires the testnet sandbox (deploy-testnet.sh / init-testnet.sh)
```

## Notes

- This is a **reference/example**, not a production frontend: no error/retry
  hardening, no token-metadata lookup (amounts are raw base units), and no
  time-series/indexer features (which require off-chain event ingestion per the
  data model).
- Network selection is driven entirely by environment variables; the same build
  runs against testnet or mainnet. See [Switching networks](#switching-networks).
- The transaction flow (`prepareTransaction` → Freighter `signTransaction` →
  `sendTransaction`) mirrors the "Wallets (browser)" section of the client guide.
