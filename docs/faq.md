# Frequently Asked Questions

Answers for integrators building on the PayStream stream contract. See the [API reference](api-reference.md) for full function details.

## Stream lifecycle

**1. What happens if the employee doesn't withdraw for a year?**
Nothing is lost. Tokens keep accruing at `rate_per_second` until the deposit is fully earned (or `stop_time` is reached). The employee can withdraw the full accrued amount at any time, capped at `deposit - withdrawn`. See [`claimable`](api-reference.md#claimable) and [`withdraw`](api-reference.md#withdraw).

**2. Can an employer cancel before any time passes?**
Yes. `cancel_stream` works on any stream that is not already Cancelled or Exhausted. If no time has elapsed the employee receives 0 and the employer is refunded the full deposit. See [`cancel_stream`](api-reference.md#cancel_stream).

**3. What is the minimum stream duration?**
There is no minimum duration. The only constraints are that `stop_time` (when non-zero) must be in the future, `rate_per_second` must be between 1 and 1,000,000,000, and `deposit` must meet the admin-configured minimum. Effective duration is `deposit / rate_per_second` seconds, or until `stop_time`. See [`create_stream`](api-reference.md#create_stream).

**4. What happens to accrued tokens when a stream is paused?**
`resume_stream` resets `last_withdraw_time` to the resume timestamp, so the paused interval is excluded from accrual and any unwithdrawn pre-pause balance is not carried over. Employees should withdraw before a pause. See [`resume_stream`](api-reference.md#resume_stream) and the [claimable worked examples](api-reference.md#worked-examples).

**5. Can the rate be changed mid-stream?**
Yes. `update_rate` works on Active or Paused streams. Elapsed time is settled at the old rate and new accrual uses the new rate. See [`update_rate`](api-reference.md#update_rate).

**6. Can I add funds to a running stream?**
Yes, with `top_up`. Cancelled and Exhausted streams cannot be topped up (E005 / E006).

## Token requirements

**7. Which tokens are supported?**
Any SEP-41 compliant token contract, passed as `token_address` to [`create_stream`](api-reference.md#create_stream).

**8. Why did `create_stream` fail on the token transfer?**
The employer must hold at least `deposit` tokens and, where required by the token, have approved the stream contract to pull them. See the token [`approve`](api-reference.md#approve) function.

## Admin operations

**9. What does pausing the contract do?**
`pause_contract` blocks state-changing user operations such as `create_stream` and `withdraw` (E013) until `unpause_contract` is called. Read-only queries keep working. See [`pause_contract`](api-reference.md#pause_contract).

**10. What is the admin nonce and why did my call fail with E009?**
Every admin call takes the current nonce for replay protection and increments it on success. Read it with [`admin_nonce`](api-reference.md#admin_nonce) immediately before each admin call. Step-by-step commands are in the [operations runbook](operations-runbook.md).

**11. How is admin ownership transferred?**
In two steps: the current admin calls `propose_admin` with the current nonce, then the new admin calls `accept_admin` with that same nonce. See [`propose_admin`](api-reference.md#propose_admin) and [`accept_admin`](api-reference.md#accept_admin).

## Upgrade process

**12. How is the contract upgraded?**
The admin proposes a new WASM hash with `propose_upgrade`; after a 48-hour timelock it is applied with `execute_upgrade`. Stream state is preserved. See [`upgrade`](api-reference.md#upgrade) and the [upgrade guide](upgrade-guide.md).

## Error codes

**13. Where can I find the meaning of an error code like `E017`?**
All error codes are listed in [Error Codes](api-reference.md#error-codes). Panics carry the code as a message prefix, e.g. `E017: stream is not active`.

**14. Why does `withdraw` fail with E017?**
Withdrawals are only allowed on Active (or Exhausted) streams. Paused and Cancelled streams reject withdrawals.
