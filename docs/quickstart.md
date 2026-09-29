# Quickstart: Your First Stream in 5 Minutes

Create a salary stream on Stellar Testnet, watch it accrue, and withdraw from it.
**Estimated time: 5–10 minutes.** No Rust toolchain or contract deployment needed.

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

## Next steps

- Pause, resume, top up, or cancel the stream: [API Reference](api-reference.md)
- Stream states and transitions: [Stream Status Lifecycle](api-reference.md#stream-status-lifecycle)
- Error codes you may hit: [Error Codes](api-reference.md#error-codes)
- JavaScript integration: [Client Guide](client-guide.md)
