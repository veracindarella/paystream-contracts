# PayStream Dashboard (reference implementation)

A minimal [Next.js](https://nextjs.org) + React dashboard that reads PayStream
salary streams from **Stellar testnet** and lets an employee withdraw. It is a
working integration example for the data model in
[`docs/dashboard-data-model.md`](../../docs/dashboard-data-model.md) and the call
patterns in [`docs/client-guide.md`](../../docs/client-guide.md) — it is kept
separate from the `contracts/` workspace and is not part of the Rust build or CI.

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

1. A deployed PayStream stream contract on testnet. From the repo root:

   ```bash
   ./scripts/deploy-testnet.sh   # note the printed stream contract id
   ./scripts/init-testnet.sh     # initialize the contract (admin)
   ```

2. The [Freighter browser extension](https://www.freighter.app/) installed.
3. A testnet account funded with XLM (friendbot:
   `https://friendbot.stellar.org?addr=<PUBLIC_KEY>`), and some streams created
   against that account so the dashboard has something to show. You can create
   streams with the snippets in [`docs/client-guide.md`](../../docs/client-guide.md).

## Setup

```bash
cd examples/dashboard
cp .env.example .env.local   # then edit NEXT_PUBLIC_STREAM_CONTRACT_ID
npm install
npm run dev                  # http://localhost:3000
```

`.env.local`:

| Variable | Description |
|---|---|
| `NEXT_PUBLIC_STREAM_CONTRACT_ID` | Stream contract id from `deploy-testnet.sh`. |
| `NEXT_PUBLIC_RPC_URL` | Soroban RPC endpoint. This also **selects the network**: `https://soroban-testnet.stellar.org` for testnet (default) or `https://soroban.stellar.org` for mainnet. It must match the network passphrase configured in `src/lib/stellar.ts` — a mismatch will cause signature/verification failures. |

## Deployment

For step-by-step hosting instructions (Vercel and self-hosted Node), the full
environment variable reference, and how to switch between testnet and mainnet,
see [`docs/dashboard-deployment.md`](../../docs/dashboard-deployment.md).

## Usage

1. Open the app and click **Connect Freighter**, selecting the account you funded.
2. The **Employer** tab lists streams where the connected account is the employer;
   the **Employee** tab lists streams where it is the employee. Use the same
   account for both if you created a stream between two of your own test accounts,
   or switch accounts in Freighter to see each side.
3. In the **Employee** tab, click **Withdraw** on a stream (or **Withdraw all**) to
   claim accrued tokens. The transaction is simulated, signed by Freighter, and
   submitted to testnet; the list refreshes on success.

## Scripts

| Command | Description |
|---|---|
| `npm run dev` | Start the dev server. |
| `npm run build` | Production build. |
| `npm run start` | Serve the production build. |
| `npm run typecheck` | TypeScript check (`tsc --noEmit`). |

## Notes

- This is a **reference/example**, not a production frontend: no error/retry
  hardening, no token-metadata lookup (amounts are raw base units), and no
  time-series/indexer features (which require off-chain event ingestion per the
  data model).
- The transaction flow (`prepareTransaction` → Freighter `signTransaction` →
  `sendTransaction`) mirrors the "Wallets (browser)" section of the client guide.
