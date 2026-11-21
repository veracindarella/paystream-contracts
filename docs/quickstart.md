# Quickstart: Your First Stream in 5 Minutes

Create a salary stream on Stellar Testnet, watch it accrue, and withdraw from it.
**Estimated time: 5–10 minutes.** No Rust toolchain or contract deployment needed.

## One-command quickstart

The entire setup below is automated in `scripts/quickstart.sh`. Run it to generate
accounts, fund them via Friendbot, deploy and initialise the contracts, and create
your first stream in a single command:

```bash
./scripts/quickstart.sh
```

The script prints the new **stream ID** and the **claimable amount** after ~5 seconds
of accrual. You can then withdraw with the manual steps in section 6.

You will play both roles: an **employer** who funds the stream and an **employee** who withdraws.
The stream uses testnet XLM (via its Stellar Asset Contract), so no token admin is required.

---

## 1. Install the Stellar CLI

```bash
cargo install --locked stellar-cli
# or, on macOS: brew install stellar-cli
stellar --version
```

Expected output (version may differ):

```
stellar 23.x.x
```

Add the testnet network (skip if already configured):

```bash
stellar network add testnet \
  --rpc-url https://soroban-testnet.stellar.org \
  --network-passphrase "Test SDF Network ; September 2015"
```

---

## 2. Create and fund two accounts

`--fund` requests 10,000 testnet XLM from Friendbot for each key.

```bash
stellar keys generate --global employer --network testnet --fund
stellar keys generate --global employee --network testnet --fund
stellar keys address employer
stellar keys address employee
```

Expected output: two public keys, e.g.

```
GABC...EMPLOYER
GXYZ...EMPLOYEE
```

---

## 3. Get test tokens

Friendbot already gave each account testnet XLM. Get the XLM token contract address
(Stellar Asset Contract) to use as the stream token:

```bash
stellar contract id asset --asset native --network testnet
```

Expected output:

```
CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC
```

Set the IDs used in the rest of this guide. `STREAM_ID` is the PayStream stream contract
from [testnet.md](testnet.md#current-contract-ids):

```bash
export STREAM_ID=<STREAM_CONTRACT_ID_FROM_TESTNET_MD>
export TOKEN_ID=$(stellar contract id asset --asset native --network testnet)
export EMPLOYER=$(stellar keys address employer)
export EMPLOYEE=$(stellar keys address employee)
```

---

## 4. Create a stream

Deposit 1 XLM (`10000000` stroops) streaming at `10000` stroops per second (~17 minutes to exhaust):

```bash
stellar contract invoke --id $STREAM_ID --source employer --network testnet \
  -- create_stream \
    --employer $EMPLOYER \
    --employee $EMPLOYEE \
    --token_address $TOKEN_ID \
    --deposit 10000000 \
    --rate_per_second 10000 \
    --stop_time 0
```

Expected output: the new stream ID, e.g.

```
42
```

```bash
export STREAM=<STREAM_ID_FROM_OUTPUT>
```

---

## 5. Check the claimable amount

Wait ~30 seconds, then:

```bash
stellar contract invoke --id $STREAM_ID --source employee --network testnet \
  -- claimable --stream_id $STREAM
```

Expected output: stroops earned so far (grows by ~10000 per second), e.g.

```
"300000"
```

Optionally check the status:

```bash
stellar contract invoke --id $STREAM_ID --source employee --network testnet \
  -- stream_status --stream_id $STREAM
```

Expected output:

```
"Active"
```

---

## 6. Withdraw

```bash
stellar contract invoke --id $STREAM_ID --source employee --network testnet \
  -- withdraw --employee $EMPLOYEE --stream_id $STREAM
```

Expected output: the amount transferred to the employee, e.g.

```
"450000"
```

Run `claimable` again — it restarts from near `0` and keeps growing.

---

## Switching networks

The quickstart above targets **testnet**, but the same stack (contracts, indexer,
API, dashboard) runs against **mainnet** without code changes. The network is
selected entirely through environment variables:

| Variable | Purpose | Testnet example |
| --- | --- | --- |
| `NETWORK` | Network name used by the indexer/API | `testnet` |
| `NETWORK_PASSPHRASE` | Stellar network passphrase | `Test SDF Network ; September 2015` |
| `RPC_URL` | Soroban RPC endpoint | `https://soroban-testnet.stellar.org` |
| `STREAM_CONTRACT_ID` | PayStream contract ID for that network | see [testnet.md](testnet.md#current-contract-ids) |

To switch to mainnet, set the same variables to the mainnet values (see
[mainnet.md](mainnet.md)) and restart the indexer and API:

```bash
export NETWORK=mainnet
export NETWORK_PASSPHRASE="Public Global Stellar Network ; September 2015"
export RPC_URL=https://soroban-mainnet.stellar.org
export STREAM_CONTRACT_ID=<MAINNET_STREAM_CONTRACT_ID>
```

Safety checks:

- **Passphrase mismatch** — the indexer and API verify that `NETWORK_PASSPHRASE`
  matches the passphrase reported by `RPC_URL` and **refuse to start** if they
  disagree, so you cannot accidentally index or serve the wrong network.
- **Contract ID validation** — `STREAM_CONTRACT_ID` (and any other configured
  contract IDs) are validated against the selected network; IDs that do not
  exist on that network cause startup to fail with a clear error.

Per-network values (passphrase, RPC URL, contract IDs) are documented in
[testnet.md](testnet.md) and [mainnet.md](mainnet.md).

---

## Next steps

- Pause, resume, top up, or cancel the stream: [API Reference](api-reference.md)
- Stream states and transitions: [Stream Status Lifecycle](api-reference.md#stream-status-lifecycle)
- Error codes you may hit: [Error Codes](api-reference.md#error-codes)
- JavaScript integration: [Client Guide](client-guide.md)

## Estimating payroll costs

Employers can model payroll costs before funding a stream using the off-chain
calculator `scripts/project-stream.sh` (or its on-chain complement,
`estimated_exhaustion_time`, view SC-40).

Given a desired `rate_per_second` and `deposit`, the script reports:

- **Time to exhaustion** — how long the deposit lasts at the given rate
- **Monthly cost** — tokens consumed per 30-day month
- **Recommended deposit for 6 months** — amount needed to run a stream half a year

### Plain text output

```bash
./scripts/project-stream.sh 10000 10000000
```

Example output:

```
PayStream payroll projection
============================
Rate per second:    10000 tokens
Deposit:            10000000 tokens
Time to exhaustion: 1000 seconds (~0.01 days)
Monthly cost:       25920000000 tokens/month
Recommended deposit for 6 months: 155520000000 tokens
```

### JSON output

```bash
./scripts/project-stream.sh 10000 10000000 --json
```

Example output:

```json
{
  "rate_per_second": 10000,
  "deposit": 10000000,
  "stop_time": 0,
  "time_to_exhaustion_seconds": 1000,
  "time_to_exhaustion_days": 0.01,
  "monthly_cost": 25920000000,
  "recommended_deposit_6_months": 155520000000
}
```

An optional `stop_time` (hard end timestamp) may be passed as the third
argument. The same calculation is available on-chain via
`estimated_exhaustion_time(stream_id)`, which returns the estimated ledger
timestamp at which a stream will be exhausted.

Run via Makefile:

```bash
make project RATE=10000 DEPOSIT=10000000
```
