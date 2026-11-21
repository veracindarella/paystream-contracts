# Dashboard Deployment Guide

This guide explains how to deploy the StreamPay dashboard located in [`examples/dashboard`](../examples/dashboard). It covers deploying to **Vercel** and **self-hosting on a Node.js server**, along with environment configuration and network selection.

> For local development and an overview of the dashboard, see [`examples/dashboard/README.md`](../examples/dashboard/README.md). For details on the client SDK used by the dashboard, see [`docs/client-guide.md`](./client-guide.md).

---

## Table of Contents

- [Prerequisites](#prerequisites)
- [Build Steps](#build-steps)
- [Environment Variables](#environment-variables)
- [Network Selection](#network-selection)
- [Deploying to Vercel](#deploying-to-vercel)
- [Self-Hosted Node Deployment](#self-hosted-node-deployment)
- [Troubleshooting](#troubleshooting)

---

## Prerequisites

- **Node.js** 18.x or later (20.x recommended)
- **npm** 9.x or later
- A Stellar account and a deployed StreamPay contract (see the main [README](../README.md))
- Access to a Stellar RPC endpoint (e.g. `https://soroban-testnet.stellar.org` for testnet)

---

## Build Steps

From the repository root:

```bash
cd examples/dashboard

# Install dependencies
npm install

# Build the production bundle
npm run build

# Start the production server
npm run start
```

The dashboard is a Next.js application. `npm run build` produces an optimized production build, and `npm run start` serves it (default port `3000`, override with `PORT`).

---

## Environment Variables

The dashboard reads configuration from environment variables. Copy the example file to get started:

```bash
cp examples/dashboard/.env.example examples/dashboard/.env.local
```

### Variable Reference

| Variable | Required | Description | Example |
| --- | --- | --- | --- |
| `NEXT_PUBLIC_STREAM_CONTRACT_ID` | Yes | The contract ID of the deployed StreamPay contract the dashboard interacts with. | `CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM` |
| `NEXT_PUBLIC_RPC_URL` | Yes | The Stellar RPC (Soroban RPC) endpoint used to submit and query transactions. | `https://soroban-testnet.stellar.org` |
| `NEXT_PUBLIC_NETWORK_PASSPHRASE` | No | Overrides the network passphrase. Defaults are derived from the RPC URL in `src/lib/stellar.ts`. | `Test SDF Network ; September 2015` |
| `PORT` | No | Port the production server listens on (self-hosted only). Defaults to `3000`. | `8080` |

> **Note:** Variables prefixed with `NEXT_PUBLIC_` are embedded into the client bundle at **build time**. If you change them, you must rebuild the application. Never place secrets in `NEXT_PUBLIC_` variables.

### `.env.local` example

```dotenv
# examples/dashboard/.env.local
NEXT_PUBLIC_STREAM_CONTRACT_ID=CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM
NEXT_PUBLIC_RPC_URL=https://soroban-testnet.stellar.org
```

---

## Network Selection

The dashboard supports both **testnet** and **mainnet**. Network selection is driven by the RPC URL and the corresponding network passphrase.

### How it works

- `src/lib/stellar.ts` derives the network passphrase from the configured RPC URL. The `NETWORK_PASSPHRASE` constant is set based on whether the RPC URL points at testnet or mainnet.
- `next.config.mjs` includes a webpack fallback configuration so that Node-only modules used by the Stellar SDK resolve correctly in the browser bundle. No changes are required for network switching — the same build works for either network as long as the env vars are set correctly.

### Testnet configuration

```dotenv
NEXT_PUBLIC_RPC_URL=https://soroban-testnet.stellar.org
# Passphrase: "Test SDF Network ; September 2015"
```

### Mainnet configuration

```dotenv
NEXT_PUBLIC_RPC_URL=https://soroban-mainnet.stellar.org
# Passphrase: "Public Global Stellar Network ; September 2015"
```

> **Important:** The `NEXT_PUBLIC_STREAM_CONTRACT_ID` differs between networks. Make sure the contract ID matches the network selected by `NEXT_PUBLIC_RPC_URL`. A testnet contract ID will not resolve on mainnet.

### Selecting a network at deploy time

Because these values are baked in at build time, choose your network **before** building:

```bash
# Testnet build
NEXT_PUBLIC_RPC_URL=https://soroban-testnet.stellar.org \
NEXT_PUBLIC_STREAM_CONTRACT_ID=C...TESTNET \
npm run build

# Mainnet build
NEXT_PUBLIC_RPC_URL=https://soroban-mainnet.stellar.org \
NEXT_PUBLIC_STREAM_CONTRACT_ID=C...MAINNET \
npm run build
```

---

## Deploying to Vercel

Vercel is the simplest way to host the dashboard.

### 1. Import the repository

1. Log in to [Vercel](https://vercel.com) and click **Add New → Project**.
2. Import your fork of the StreamPay repository from GitHub.

### 2. Configure the project

On the import screen, set:

- **Framework Preset:** Next.js (auto-detected)
- **Root Directory:** `examples/dashboard`
- **Build Command:** `npm run build` (default)
- **Output Directory:** `.next` (default)

![Vercel project settings — Root Directory set to examples/dashboard](images/vercel-root-directory.png)

> _Screenshot placeholder: Vercel "Configure Project" screen with Root Directory = `examples/dashboard`._

### 3. Add environment variables

Under **Settings → Environment Variables**, add the variables from the [table above](#variable-reference). Apply them to the **Production** (and optionally **Preview**) environments.

| Name | Value |
| --- | --- |
| `NEXT_PUBLIC_STREAM_CONTRACT_ID` | `C...` |
| `NEXT_PUBLIC_RPC_URL` | `https://soroban-testnet.stellar.org` |

![Vercel environment variables](images/vercel-env-vars.png)

> _Screenshot placeholder: Vercel Environment Variables panel showing the two `NEXT_PUBLIC_` variables._

### 4. Deploy

Click **Deploy**. Vercel installs dependencies, runs `npm run build`, and publishes the app. Subsequent pushes to your connected branch trigger automatic redeployments.

> **Tip:** To switch networks on Vercel, update the environment variables and trigger a **Redeploy** (env vars are baked in at build time).

---

## Self-Hosted Node Deployment

You can run the dashboard on any server with Node.js installed.

### 1. Build the application

```bash
cd examples/dashboard
npm ci
npm run build
```

### 2. Run the production server

```bash
# Reads .env.local (or exported env vars) and serves on PORT (default 3000)
npm run start
```

Put a reverse proxy (Nginx, Caddy, etc.) in front of it to terminate TLS and forward traffic to the Node process.

#### Sample Nginx reverse proxy

```nginx
server {
    listen 80;
    server_name dashboard.example.com;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

### 3. Run as a systemd service

Create `/etc/systemd/system/streampay-dashboard.service`:

```ini
[Unit]
Description=StreamPay Dashboard
After=network.target

[Service]
Type=simple
User=streampay
WorkingDirectory=/opt/streampay/examples/dashboard
Environment=NODE_ENV=production
Environment=PORT=3000
EnvironmentFile=/opt/streampay/examples/dashboard/.env.local
ExecStart=/usr/bin/npm run start
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Enable and start it:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now streampay-dashboard
sudo systemctl status streampay-dashboard
```

### 4. Alternative: Docker

```dockerfile
# Dockerfile (place at examples/dashboard/Dockerfile)
FROM node:20-alpine AS builder
WORKDIR /app
COPY package*.json ./
RUN npm ci
COPY . .
RUN npm run build

FROM node:20-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production
ENV PORT=3000
COPY --from=builder /app ./
EXPOSE 3000
CMD ["npm", "run", "start"]
```

Build and run:

```bash
cd examples/dashboard
docker build -t streampay-dashboard .
docker run -d --name streampay-dashboard \
  -p 3000:3000 \
  --env-file .env.local \
  streampay-dashboard
```

> **Reminder:** `NEXT_PUBLIC_` variables are compiled into the bundle during `docker build`. Pass them as build args (or bake them into `.env.local` before building) so the correct network and contract are used.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| Dashboard shows "contract not found" | Contract ID doesn't match the network | Verify `NEXT_PUBLIC_STREAM_CONTRACT_ID` matches the network in `NEXT_PUBLIC_RPC_URL` |
| Transactions fail with passphrase error | Wrong network passphrase | Ensure the RPC URL points to the intended network; check `NETWORK_PASSPHRASE` in `src/lib/stellar.ts` |
| Env changes have no effect | `NEXT_PUBLIC_` vars are build-time only | Rebuild / redeploy after changing variables |
| Build fails resolving Node modules | Webpack fallback misconfigured | Confirm `next.config.mjs` retains the webpack fallback config |

---

## Related Documentation

- [`examples/dashboard/README.md`](../examples/dashboard/README.md) — dashboard overview and local development
- [`docs/client-guide.md`](./client-guide.md) — client SDK usage
- [Main README](../README.md) — project overview and contract deployment