# Auth Audit — `paystream-stream` Contract

**Date:** 2026-10-18  
**Auditor:** Internal (SEC-25 / issue #263)  
**Scope:** Every public function in `contracts/stream/src/lib.rs`  
**Method:** Manual review of each entry point for correct `require_auth()` placement  
**Status:** ✅ No gaps found

---

## Summary

All state-mutating public functions call `require_auth()` on the correct party before
performing any state read or write. Read-only functions that expose no privileged data
require no auth — this is intentional and consistent with Soroban best practice.

---

## Function-by-Function Review

### Admin functions

| Function | Required Signer | Auth Call | Verification |
|---|---|---|---|
| `initialize(admin)` | `admin` | `admin.require_auth()` | ✅ Called before `set_admin`; panics if admin is already set so replaying is impossible |
| `propose_admin(new_admin, nonce)` | current admin (via `get_admin`) | `current.require_auth()` | ✅ Current admin fetched from storage, then `.require_auth()` before any mutation |
| `accept_admin(new_admin, nonce)` | `new_admin` (incoming admin) | `new_admin.require_auth()` | ✅ Called on the nominee before validating pending admin state |
| `pause_contract(nonce)` | current admin (via `get_admin`) | `admin.require_auth()` | ✅ Admin fetched, authed, nonce consumed, then paused |
| `unpause_contract(nonce)` | current admin (via `get_admin`) | `admin.require_auth()` | ✅ Same pattern as `pause_contract` |
| `set_min_deposit(admin, nonce, amount)` | `admin` | `admin.require_auth()` | ✅ Auth + equality check against stored admin |
| `expire_streams(admin, stream_ids)` | `admin` | `admin.require_auth()` | ✅ Auth + equality check against stored admin before iterating |
| `upgrade(new_wasm_hash, nonce)` | current admin (via instance storage) | `admin.require_auth()` | ✅ Admin loaded from storage, authed, nonce consumed before WASM swap |
| `propose_upgrade(new_wasm_hash, nonce)` | current admin | `admin.require_auth()` | ✅ Same as `upgrade` |
| `execute_upgrade(nonce)` | current admin | `admin.require_auth()` | ✅ Auth before checking timelock and executing |
| `cancel_upgrade(nonce)` | current admin | `admin.require_auth()` | ✅ Auth before clearing pending upgrade |
| `migrate(admin)` | `admin` | `admin.require_auth()` | ✅ Auth + equality check against stored admin |
| `set_max_creates_per_ledger(admin, nonce, max)` | `admin` | `admin.require_auth()` | ✅ Auth + equality check against stored admin; nonce consumed |

### Employer functions

| Function | Required Signer | Auth Call | Verification |
|---|---|---|---|
| `create_stream(employer, ...)` | `employer` | `employer.require_auth()` | ✅ First line of function body; checked before any token transfer |
| `create_streams_batch(employer, params)` | `employer` | `employer.require_auth()` | ✅ First line; batch validation runs after auth |
| `create_stream_from_template(employer, employee, template_id)` | `employer` (delegated to `create_stream`) | Called inside `create_stream` | ✅ Template load is read-only; auth is enforced by the delegated `create_stream` call |
| `top_up(employer, stream_id, amount, ...)` | `employer` | `employer.require_auth()` | ✅ First call; employer equality against stored stream checked after |
| `set_low_balance_threshold(employer, stream_id, threshold)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `set_auto_topup(employer, stream_id, ...)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `cancel_auto_topup(employer, stream_id)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `pause_stream(employer, stream_id)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `pause_stream_batch(employer, stream_ids)` | `employer` | `employer.require_auth()` | ✅ Auth before batch loop |
| `resume_stream(employer, stream_id)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `resume_stream_batch(employer, stream_ids)` | `employer` | `employer.require_auth()` | ✅ Auth before batch loop |
| `update_rate(employer, stream_id, new_rate)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `cancel_stream(employer, stream_id)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality check |
| `cancel_streams_batch(employer, stream_ids)` | `employer` | `employer.require_auth()` | ✅ Auth before batch loop |
| `transfer_stream(employer, stream_id, new_employer)` | `employer` | `employer.require_auth()` | ✅ Auth then employer equality + new_employer ≠ employee check |
| `save_template(employer, template_id, params)` | `employer` | `employer.require_auth()` | ✅ Auth before template write |

### Employee functions

| Function | Required Signer | Auth Call | Verification |
|---|---|---|---|
| `withdraw(employee, stream_id)` | `employee` | `employee.require_auth()` | ✅ Auth before stream load; receipt owner also verified against `employee` |
| `withdraw_all(employee)` | `employee` | `employee.require_auth()` | ✅ Auth once before iterating all employee streams; receipt owner re-checked per stream |
| `transfer_receipt(from, to, stream_id)` | `from` (current receipt holder) | `from.require_auth()` | ✅ Auth then receipt owner equality check before transferring |

### Public / permissionless functions (no auth required)

These functions are intentionally callable by anyone; they either return read-only data
or perform permissionless settlement that benefits the stream's parties.

| Function | Rationale |
|---|---|
| `get_stream(stream_id)` | Read-only; no state mutation |
| `get_streams_batch(stream_ids)` | Read-only |
| `stream_status(stream_id)` | Read-only |
| `claimable(stream_id)` | Read-only |
| `claimable_batch(stream_ids)` | Read-only |
| `claimable_at(stream_id, timestamp)` | Read-only projection |
| `receipt_owner(stream_id)` | Read-only |
| `get_auto_topup_config(stream_id)` | Read-only |
| `get_auto_topup(stream_id)` | Read-only alias |
| `get_pending_admin()` | Read-only |
| `admin_nonce()` | Read-only |
| `streams_by_employer(employer)` | Read-only index query |
| `streams_by_employee(employee)` | Read-only index query |
| `streams_by_employee_paginated(employee, offset, limit)` | Read-only paginated query |
| `streams_in_range(from_ts, to_ts)` | Read-only time-range query |
| `stream_count()` | Read-only counter |
| `stream_count_by_employer(employer)` | Read-only counter |
| `stream_count_by_employee(employee)` | Read-only counter |
| `stream_count_by_token(token)` | Read-only counter |
| `is_paused()` | Read-only |
| `admin()` | Read-only; admin address is not secret |
| `version()` | Read-only |
| `list_templates(employer)` | Read-only |
| `settle_stream(stream_id)` | Permissionless keeper; settles an expired stream in favour of its parties; no party can extract funds they are not owed |

---

## Auth Placement Pattern

The codebase follows a consistent pattern:

1. Call `require_auth()` as the **first statement** in every state-mutating function.
2. Load stream state **after** auth to avoid paying load costs for unauthenticated calls.
3. Re-assert caller identity against stored stream fields (`assert_eq!(stream.employer, employer)`)
   as a secondary check — ensures the authed address is also the *correct* party for that specific stream.
4. Consume admin nonces atomically with the auth check to prevent replay attacks.

This two-layer model (auth + field equality) means a valid Stellar keypair that is not
the stream's employer cannot affect that stream, even if it can authenticate.

---

## Gaps Found

**None.** No missing or misplaced `require_auth()` calls were identified during this audit.

---

## CodeQL Coverage

The custom CodeQL query `.github/codeql/queries/MissingRequireAuth.ql` enforces this
at the CI level. It flags any `impl` function that:
- mutates storage (calls `set_*`, `save_*`, `remove_*`, or similar helpers), and
- does not call `require_auth` or a recognized auth-checking helper.

The query fires on each push via `.github/workflows/codeql.yml`. Any regression
(new function missing auth) will block the PR.

A dedicated `StorageMutationWithoutAuth.ql` query was also added (see
`.github/codeql/queries/StorageMutationWithoutAuth.ql`) that focuses specifically
on direct `env.storage().*.set()` and `.remove()` calls without a preceding auth
check in the same function scope.

---

## API Authentication & Audit Model (BE-7)

The public HTTP API in front of the contract has its own auth layer, separate from
Soroban `require_auth()`. This section documents the API-key lifecycle, which
endpoints require a key, anonymous throttling, and the audit log for key usage.

### API key lifecycle

| Stage | Behaviour |
|---|---|
| **Issue** | Keys are minted by an admin-only endpoint (`POST /v1/api-keys`). The server generates a random secret (≥ 32 bytes, CSPRNG), returns the plaintext **once**, and stores only a hash (SHA-256) plus metadata: `key_id`, `label`, `scopes`, `created_at`, `created_by`, `expires_at` (optional), `revoked_at` (null while active). |
| **Use** | Clients send `Authorization: Bearer <key>`. The server hashes the presented secret, looks up the active record by hash, and rejects unknown, expired, or revoked keys with `401 Unauthorized`. |
| **Rotate** | Issue a new key, migrate the client, then revoke the old one. Overlapping validity is allowed; keys are independent. |
| **Revoke** | `DELETE /v1/api-keys/{key_id}` (admin-only) sets `revoked_at`. Revocation is effective immediately — the lookup rejects revoked keys on the next request. Revocation is idempotent and never deletes the audit trail. |
| **Expire** | Keys with `expires_at` in the past are treated as revoked; no background job is required for enforcement. |

Scopes are coarse-grained: `read`, `write`, `subscribe`. A key may hold any subset;
a request requiring a scope the key lacks is rejected with `403 Forbidden`.

### Endpoint auth requirements

| Endpoint class | Examples | Auth | Throttle |
|---|---|---|---|
| **Write** | `POST /v1/streams`, `POST /v1/streams/{id}/top-up`, `POST /v1/streams/{id}/cancel`, `POST /v1/templates` | API key with `write` scope | Per-key token bucket |
| **Subscribe** | `POST /v1/subscriptions`, `DELETE /v1/subscriptions/{id}`, webhook registration | API key with `subscribe` scope | Per-key token bucket |
| **Key management** | `POST /v1/api-keys`, `DELETE /v1/api-keys/{id}` | Admin credential (not a normal API key) | Per-IP token bucket |
| **Anonymous read** | `GET /v1/streams/{id}`, `GET /v1/streams?employer=…`, `GET /v1/streams/{id}/claimable` | None | Per-IP token bucket (stricter) |

Anonymous reads are allowed so public dashboards and explorers keep working, but they
are throttled more aggressively than keyed reads. Keyed reads (a key with `read`
scope) get the per-key bucket.

### Rate limiting

Token-bucket limiter, one bucket per API key (keyed by `key_id`) and one per client IP
for anonymous traffic. Bucket parameters are configurable per deployment; defaults:

| Class | Capacity | Refill |
|---|---|---|
| Keyed write/subscribe | 60 requests | 1 req/s |
| Keyed read | 300 requests | 5 req/s |
| Anonymous read (per IP) | 30 requests | 0.5 req/s |

When a bucket is empty the API returns:

```
HTTP/1.1 429 Too Many Requests
Retry-After: <seconds until next token>
X-RateLimit-Limit: <capacity>
X-RateLimit-Remaining: 0
X-RateLimit-Reset: <unix seconds>
```

`Retry-After` is computed from the bucket refill rate (ceil of the time to the next
token) and is always ≥ 1. Rate-limit state is in-memory per instance; multi-instance
deployments must use a shared store (e.g. Redis) so limits are enforced globally.

### Audit log for key usage

Every authenticated request (and every rejected auth attempt) appends one record to
the append-only audit log. Records are written before the response is sent; a failed
audit write fails the request closed (`503`) rather than allowing unaudited access.

Schema (one JSON object per line, or one row per record in the audit table):

| Field | Type | Description |
|---|---|---|
| `ts` | RFC 3339 timestamp | When the request was processed |
| `request_id` | string (UUID) | Correlates with response header `X-Request-Id` |
| `key_id` | string \| null | Key identifier; `null` for anonymous requests |
| `key_label` | string \| null | Human label at time of use (denormalized) |
| `outcome` | enum | `allowed`, `denied_scope`, `denied_revoked`, `denied_expired`, `denied_unknown`, `rate_limited` |
| `method` | string | HTTP method |
| `path` | string | Request path (query string redacted of secrets) |
| `status` | integer | HTTP status returned |
| `ip` | string | Client IP (from trusted proxy header) |
| `user_agent` | string \| null | Truncated to 256 chars |
| `latency_ms` | integer | Server processing time |

The plaintext key secret is **never** logged — only `key_id`. Request bodies are not
logged.

**Retention:** audit records are retained for **90 days** by default (configurable via
`AUDIT_RETENTION_DAYS`), then purged by a scheduled job. Records are immutable for
their retention window; revocation of a key does not delete its history. Aggregated
counters (requests per key per day) may be retained longer for abuse analysis.

### Relationship to contract auth

API-key auth gates *who may submit* a request to the backend. It does not replace
Soroban auth: the backend still builds transactions that the end user's Stellar
keypair must sign, and the contract's `require_auth()` checks remain the source of
truth for on-chain authorization. A compromised API key can submit requests but
cannot move funds without the corresponding Stellar signatures.

---

## References

- Issue: #263 (SEC-25)
- Issue: #408 (BE-7 — API authentication and rate limiting)
- Related: `.github/codeql/queries/MissingRequireAuth.ql`
- Related: `.github/codeql/queries/StorageMutationWithoutAuth.ql`
- Soroban auth model: https://developers.stellar.org/docs/build/smart-contracts/example-contracts/auth
