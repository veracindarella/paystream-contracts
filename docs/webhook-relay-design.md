# Webhook Relay Service Design (PROD-24)

**Issue:** #313
**Status:** Design
**Priority:** Medium

---

## Problem

[dashboard-data-model.md](dashboard-data-model.md) marks several dashboard data points as
"Indexed" (refund totals, low-balance alerts, event history) and assumes an off-chain consumer
of contract events, but employers currently have no push-based way to learn about stream events.
Polling `streams_by_employer` + `claimable` per employer is expensive for payroll back-ends and
too slow for alerting (e.g. `low_bal`).

The **webhook relay** is a standalone off-chain service that subscribes to PayStream contract
events on-chain, filters and transforms them, and pushes HTTPS webhooks to employer-registered
endpoints. This document is the design prerequisite for implementation; no contract changes are
required — the relay consumes the events defined in [events-schema.md](events-schema.md) (v1).

---

## Architecture

```text
Soroban RPC / Horizon ──► Ingestor ──► Filter ──► Transformer ──► Queue ──► Dispatcher ──► Employer webhook
                            (cursor)   (topics)   (payload v1)   (per-url)  (retry/DLQ)
```

### 1. Event subscription (Ingestor)

- Subscribe to `CONTRACT`-type diagnostics/events for the PayStream contract ID via
  `soroban_rpc:GET /events` (Horizon's `events?type=contract` is the fallback), paginated by
  ledger cursor; persist the last processed cursor so restarts resume without gaps.
- Poll cadence: one request per new ledger (~5–7 s on mainnet), tracked by `paging_token`.
- Deduplicate on `(ledger, tx_hash, event_index)` — the relay's canonical **event ID**.

### 2. Filtering

- Keep only topics whose first two components match `["v1", <known event>]` from
  [events-schema.md](events-schema.md); ignore everything else (schema upgrades then degrade to
  "unknown event" logs instead of malformed deliveries).
- Route events to subscribers by matching `employer` (extracted from the event data or, for
  `withdraw`/`status` on the employee side, resolved via the stream's indexed `employer` from a
  one-time `get_stream` read cached per stream ID).

### 3. Transformation

Decode raw SCMJSON topics/data into a stable, documented webhook payload (below). Amounts stay
`i128` but are serialized as **strings** to survive JSON float limits.

### 4. Delivery (Dispatcher)

One worker per webhook URL, FIFO with per-URL concurrency 1 so events for a stream arrive in
order. Requests time out after 10 s.

---

## Event payload schema

`POST <webhook_url>` with `Content-Type: application/json`:

```json
{
  "id": "0001234567890-2-0",
  "type": "stream.low_balance",
  "created_at": 1762000000,
  "payload": {
    "stream_id": "42",
    "employer": "GABC...",
    "token": "GDEF...",
    "remaining": "1500000",
    "threshold": "2000000"
  }
}
```

| `type` | Source event | Payload fields |
|---|---|---|
| `stream.created` | `stream_created` | `stream_id`, `employer`, `employee`, `rate` |
| `stream.withdrawn` | `withdrawn` | `stream_id`, `employee`, `amount` |
| `stream.topped_up` | `topped_up` | `stream_id`, `employer`, `amount` |
| `stream.status_changed` | `stream_status_changed` | `stream_id`, `old_status`, `new_status` |
| `stream.cancelled` | `stream_cancelled` | `stream_id`, `claimable_amount`, `refund_amount` |
| `stream.rate_updated` | `rate_updated` | `stream_id`, `old_rate`, `new_rate` |
| `stream.low_balance` | `low_bal` | `stream_id`, `employer`, `remaining`, `threshold` |
| `contract.paused` / `contract.unpaused` | `contract_paused` | `paused` |

`id` is the canonical event ID (cursor + index) and is stable across redeliveries.

---

## Retry logic and idempotency

- **Retry:** exponential backoff with full jitter — base delays 1 min, 5 min, 25 min, 2 h, 8 h
  (5 attempts total, i.e. 1 initial delivery + 4 retries). The actual delay for attempt `n` is
  `random(0, base[n])` (full jitter). HTTP `410` or repeated DNS failure ⇒ retire the webhook to
  `disabled` state.
- **Dead letter:** after final failure the event is parked in the `webhook_dead_letters` table
  for 30 days, replayable by the owner via `POST /v1/webhooks/{id}/dead-letters/{dlq_id}/replay`.
- **Non-retryable:** HTTP `4xx` other than `408`/`429` ⇒ drop after one attempt, log, count
  against the webhook's failure budget (10 consecutive drops ⇒ auto-disable + notification email).
- **Idempotency:** at-least-once delivery. Consumers deduplicate on the `Idempotency-Key`
  header (equal to the payload `id`). The relay itself never double-enqueues: the ingest cursor
  advances only after the event is durably queued.
- **Ordering:** best-effort per stream (single dispatcher worker per URL); across URLs no
  ordering guarantee. Documented as such.
- **Observability:** every attempt is recorded in `webhook_delivery_attempts`
  (`subscriber_id`, `event_id`, `attempt`, `status_code`, `response_body` (truncated),
  `error`, `created_at`), exposed via `GET /v1/webhooks/{id}/deliveries`.
=======

---

## Security

- **TLS only.** Registration and delivery URLs must be `https://`; plain HTTP rejected with a
  dedicated error. Certificates validated strictly (no self-signed pinning option in v1).
- **HMAC signing:** every delivery carries
  `X-PayStream-Signature: t=<unix_ts>,v1=<hex>` where
  `v1 = HMAC-SHA256(secret, "<t>." + raw_body)`. Secret is generated at registration, shown
  once, stored encrypted (KMS/at-rest envelope encryption). Verifiers must reject timestamps
  older than 5 minutes (replay window). `low_bal`-class payloads are also replay-safe because
  consumers key on the event `id`.
- **SSRF protection:** resolve the webhook host before registering; refuse private, loopback,
  link-local, and metadata ranges; re-resolve at delivery time.
- **Secrets hygiene:** signing secret and full Authorization headers never logged.

---

## API: webhook registration

Small authenticated REST surface (employer authenticates with a Stellar wallet signature,
SEP-43-style, nonce-based like the contract admin nonce pattern):

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/v1/webhooks` | Register `{url, events[], secret?}` → returns webhook `id` + secret |
| `GET` | `/v1/webhooks` | List caller's webhooks (no secrets) |
| `PATCH` | `/v1/webhooks/{id}` | Update `url` / `events` / `active` |
| `DELETE` | `/v1/webhooks/{id}` | Remove webhook and pending deliveries |
| `POST` | `/v1/webhooks/{id}/test` | Send a signed `ping` test event |
| `GET` | `/v1/webhooks/{id}/deliveries` | Per-attempt delivery status (status code, response, timestamp) |
| `GET` | `/v1/webhooks/{id}/dead-letters` | List dead-lettered events |
| `POST` | `/v1/webhooks/{id}/dead-letters/{dlq_id}/replay` | Re-enqueue a dead-lettered event |

Validation: max 10 webhooks per employer, max 100 stream subscriptions per webhook, URL length
≤ 2 KB. Registration triggers the SSRF check and a mandatory `ping` handshake (endpoint must
return `2xx` within 5 s) before activation.

---

## Implementation complexity estimate

| Component | Effort | Notes |
|---|---|---|
| Ingestor (RPC subscribe, cursor, dedupe) | 1 wk | Go or Rust; well-trodden, main risk is RPC pagination edge cases |
| Decoder + transformer (SCMJSON → payload) | 1 wk | Contract-event decoding and schema-version guards |
| Registration API + auth + secret storage | 1.5 wk | Includes wallet-signature auth and KMS integration |
| Dispatcher (retries, DLQ, per-URL workers) | 1 wk | Queue backend: Postgres `FOR UPDATE SKIP LOCKED` polling in v1; no new infra |
| Ops (deploy, metrics, runbook) | 0.5 wk | Lag, delivery-success, DLQ-depth dashboards |
| **Total** | **~5 engineer-weeks** | Medium. No contract changes; v1 scope is employer-facing events only |

Deferred: employee-facing events, HMAC key rotation endpoint, mTLS, event replay UI, multi-relay
horizontal sharding (single instance is sufficient at current volume).

---

## Implementation notes (issue #403)

The v1 relay lives in `backend/src/webhooks/`:

| Module | Responsibility |
|---|---|
| `subscribers.ts` | Subscriber registry: `url`, `secret`, `events[]` filter, `active` flag |
| `signature.ts` | `signPayload(secret, timestamp, rawBody)` → `t=<ts>,v1=<hex>` HMAC-SHA256 |
| `backoff.ts` | `backoffDelay(attempt)` — full-jitter exponential backoff over the base schedule |
| `dispatcher.ts` | HTTP delivery, retry loop, dead-lettering, per-attempt recording |
| `router.ts` | Registration + admin/status endpoints |

Storage (Postgres): `webhook_subscribers`, `webhook_delivery_attempts`,
`webhook_dead_letters`. Event routing resolves `employer` from the event data when present and
otherwise from a cached `get_stream` lookup keyed by stream ID. `i128` amounts are serialized as
decimal strings in the payload, matching the schema above.

---

## Acceptance criteria mapping (issue #313)

- [x] Architecture: subscription, filtering, transformation, delivery — §Architecture
- [x] Retry logic and idempotency — §Retry logic and idempotency
- [x] Security: HMAC signing, TLS only — §Security
- [x] API: registration endpoint, payload schema — §API, §Event payload schema
- [x] Complexity estimate — §Implementation complexity estimate

## Acceptance criteria mapping (issue #413, BE-12)

- [x] Subscribes to `low_bal` and `stream_near_expiry` — §Notification service §1
- [x] Per-recipient preferences and channels (email/push/in-app) — §Notification service §2
- [x] Throttle to avoid duplicate alerts per stream — §Notification service §3
- [x] Delivery and opt-out auditable — §Notification service §4
