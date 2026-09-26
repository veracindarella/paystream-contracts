# Event Streaming

Real-time dashboards can subscribe to PayStream contract events instead of polling contract state.

## Endpoint

Soroban contract events are served by the Stellar RPC `getEvents` method; Horizon exposes the underlying operations over Server-Sent Events (SSE).

| Network | Horizon (SSE) | Stellar RPC (`getEvents`) |
|---|---|---|
| Testnet | `https://horizon-testnet.stellar.org` | `https://soroban-testnet.stellar.org` |
| Mainnet | `https://horizon.stellar.org` | your RPC provider |

- **Horizon SSE** — stream every operation touching the contract; send `Accept: text/event-stream`:
  `GET /accounts/{account}/operations?cursor=now` or `GET /operations?cursor=now&include_failed=false`
- **RPC `getEvents`** — the canonical way to read decoded contract events, filterable by contract ID and topic. Poll it with the returned `cursor` to get a live stream.

## Filtering

- **By contract ID** — `filters[].contractIds: ["<STREAM_CONTRACT_ID>"]`
- **By event type** — `filters[].topics`: the first topic is the event symbol (e.g. `withdraw`), the second is the stream ID. Use `"*"` as a wildcard.

Topics are base64-encoded XDR `ScVal`s. Encode a symbol with `nativeToScVal("withdraw", { type: "symbol" }).toXDR("base64")` from `@stellar/stellar-sdk`.

## curl

```bash
# Live operations via Horizon SSE
curl -N -H "Accept: text/event-stream" \
  "https://horizon-testnet.stellar.org/operations?cursor=now"

# Decoded contract events via RPC, filtered to PayStream withdraw events
curl -s https://soroban-testnet.stellar.org -H 'Content-Type: application/json' -d '{
  "jsonrpc": "2.0", "id": 1, "method": "getEvents",
  "params": {
    "startLedger": 1000000,
    "filters": [{
      "type": "contract",
      "contractIds": ["<STREAM_CONTRACT_ID>"],
      "topics": [["<base64 ScVal symbol \"withdraw\">", "*"]]
    }],
    "pagination": { "limit": 100 }
  }
}'
```

## Node.js

```js
import { rpc, scValToNative, nativeToScVal } from "@stellar/stellar-sdk";

const server = new rpc.Server("https://soroban-testnet.stellar.org");
const CONTRACT_ID = process.env.STREAM_CONTRACT_ID;
const TOPICS = ["created", "withdraw", "status", "cancelled", "topup", "rate_upd", "milestone"];

let cursor;
let startLedger = (await server.getLatestLedger()).sequence;

async function poll() {
  const res = await server.getEvents({
    ...(cursor ? { cursor } : { startLedger }),
    filters: [{
      type: "contract",
      contractIds: [CONTRACT_ID],
      topics: TOPICS.map((t) => [nativeToScVal(t, { type: "symbol" }).toXDR("base64"), "*"]),
    }],
    limit: 100,
  });
  for (const e of res.events) {
    const [name, streamId] = e.topic.map(scValToNative);
    console.log(e.ledger, name, streamId, scValToNative(e.value));
  }
  cursor = res.cursor;
}

setInterval(poll, 5000);
```

For raw operation streaming via Horizon SSE:

```js
import { Horizon } from "@stellar/stellar-sdk";

new Horizon.Server("https://horizon-testnet.stellar.org")
  .operations()
  .cursor("now")
  .stream({ onmessage: (op) => op.type === "invoke_host_function" && console.log(op) });
```

## Event payloads

Topics are `(symbol, stream_id)` unless noted. Values shown decoded.

| Event | Topics | Value |
|---|---|---|
| Stream created | `("created", id)` | `(employer, employee, rate_per_second)` |
| Withdrawal | `("withdraw", id)` | `(employee, amount)` |
| Status change | `("status", id)` | `StreamStatus` (e.g. `["Exhausted"]`) |
| Cancellation | `("cancelled", id)` | `(claimable_amount, refund_amount)` |
| Top-up | `("topup", id)` | `(employer, amount)` |
| Rate update | `("rate_upd", id)` | `(old_rate, new_rate)` |
| Milestone unlock | `("milestone", id)` | `(employer, amount)` |
| Contract paused | `("paused",)` | `bool` |

Example decoded `withdraw` event:

```json
{
  "type": "contract",
  "ledger": 1234567,
  "contractId": "CA...STREAM",
  "topic": ["withdraw", 1],
  "value": ["GB...EMPLOYEE", "3600"]
}
```
