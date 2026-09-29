# Glossary

Soroban and PayStream terms used throughout this repository.

## PayStream terms

| Term | Definition |
|---|---|
| **Stream** | A salary stream from an employer to an employee, stored on-chain as a `Stream` record with a unique `u64` id. Funds are escrowed by the stream contract and released to the employee per second. |
| **Deposit** | The total amount of tokens the employer locks in the stream contract when creating a stream (plus any later `top_up`). It is the upper bound on what the employee can ever withdraw. |
| **Rate** | The number of tokens released to the employee per second (`rate_per_second`). Accrual stops while the stream is paused. |
| **Claimable** | Tokens earned by the employee but not yet withdrawn: `min((now - last_withdraw_time) * rate_per_second, deposit - withdrawn)`, with time capped at the stop time. Query it with `claimable(stream_id)`. |
| **Stop time** | An optional hard end timestamp (`stop_time`) after which the stream accrues nothing more. A value of `0` means the stream runs until the deposit is exhausted or it is cancelled. |
| **Exhausted** | Terminal stream status reached when the full deposit has been paid out, or the stop time has passed and the remainder is settled (via `withdraw` or `settle_stream`). No further withdrawals are possible. |

## Stellar / Soroban terms

| Term | Definition |
|---|---|
| **SEP-41** | The Stellar token interface standard (analogous to ERC-20) defining functions such as `transfer`, `balance`, and `approve`. PayStream accepts any SEP-41 compliant token. |
| **SAC** | Stellar Asset Contract — a built-in contract that exposes a classic Stellar asset (including native XLM) through the SEP-41 interface, so Soroban contracts can hold and transfer it. |
| **Stroop** | The smallest unit of a Stellar asset: 1 stroop = 0.0000001 (10⁻⁷) of a unit. Token amounts in contract calls (deposits, rates) are expressed in these base units. |
| **Ledger** | A block in the Stellar network, closed roughly every 5 seconds. Contracts read the current ledger timestamp (`env.ledger().timestamp()`) to compute accrued salary. |
| **TTL** | Time To Live — the number of ledgers a Soroban storage entry remains live before it is archived. Contracts extend TTLs (`extend_ttl`) so stream and admin data stays accessible. |
| **Nonce** | A monotonically increasing number that must be supplied with admin operations (`propose_admin`, `pause_contract`, `upgrade`, …). Each nonce is consumed once, preventing replay of a signed admin call. Query the next value with `admin_nonce()`. |
| **WASM hash** | The SHA-256 hash of an uploaded contract WebAssembly binary. Contract upgrades reference the new code by this hash (`upgrade(new_wasm_hash, nonce)`). |
