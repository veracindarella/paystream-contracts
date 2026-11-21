# SEP-41 compliance — `paystream-token` (TOK-20)

Checklist of the PayStream token contract (`contracts/token`) against the
[SEP-41 Token Interface](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0041.md),
including the CAP-67 revision (Protocol 23) that makes the `transfer`
recipient a `MuxedAddress`.

**Spec version checked:** SEP-41 as published in `stellar-protocol` `master`
with the CAP-67 muxed-address amendment (`transfer(from: Address, to:
MuxedAddress, amount: i128)`), against soroban-sdk 28.0.0.

**Status legend:** ✅ implemented · ⚠️ partial / intentional deviation · ⏳ deferred

---

## Interface functions

| SEP-41 function | Status | Notes |
|---|---|---|
| `allowance(from, spender) -> i128` | ✅ | Returns 0 once `expiration_ledger` has passed. Previously returned `(i128, u32)`, which **broke every SEP-41 caller** (e.g. `soroban_sdk::token::Client`) at decode time. The tuple is still available as the non-standard `allowance_with_expiration`. |
| `approve(from, spender, amount, expiration_ledger)` | ✅ | Requires `from` auth. Rejects negative `amount`; rejects `expiration_ledger < current ledger` when `amount > 0`. Extends the temporary allowance entry's TTL to its expiration ledger. Emits `approve`. |
| `balance(id) -> i128` | ✅ | 0 for unknown addresses. |
| `transfer(from, to: MuxedAddress, amount)` | ⚠️ | Requires `from` auth. Per CAP-67, `to` is a `MuxedAddress`: balances are keyed by the **underlying address** returned by `MuxedAddress::address()`, so a muxed recipient shares the balance of its base account. The muxed id is surfaced **only** in the `transfer` event data as `to_muxed_id` (see *Events*). **Deviation:** `amount == 0` is rejected (`"amount must be positive"`) where SEP-41 only forbids negative amounts — see *Deviations*. |
| `transfer_from(spender, from, to, amount)` | ⚠️ | Requires `spender` auth; checks expiry, allowance and balance; decrements the allowance; emits `transfer`. Same zero-amount deviation. **Fixed:** negative amounts were previously accepted and moved tokens *to* `from` while increasing the allowance. |
| `burn(from, amount)` | ⚠️ | Requires `from` auth; reduces balance and total supply; emits `burn`. Same zero-amount deviation. |
| `burn_from(spender, from, amount)` | ⚠️ | Allowance-checked burn; emits `burn`. Same zero-amount deviation. |
| `decimals() -> u32` | ✅ | Defaults to 7 (as the SAC) until set once by the admin via `set_metadata`. |
| `name() -> String` | ✅ | Default `"PayStream Token"`; set-once via `set_metadata`. |
| `symbol() -> String` | ✅ | Default `"PST"`; set-once via `set_metadata`. |

## Events

| SEP-41 event | Topics | Data | Status |
|---|---|---|---|
| `transfer` | `["transfer", from: Address, to: Address]` | `amount: i128`, or `{amount: i128, to_muxed_id: u64}` when `to` is muxed | ✅ (emitted by `transfer` and `transfer_from`) |

### CAP-67 muxed addresses

`transfer` accepts a `MuxedAddress` for `to` (CAP-67, Protocol 23). The
handling is:

- **Balances key on the underlying address.** The contract stores and updates
  balances under `to.address()` — the base `Address` of the muxed address — so
  a muxed recipient and its base account are the same balance holder. Muxed
  ids never appear in storage keys.
- **Muxed id is event-only.** When `to` is muxed, the `transfer` event's `to`
  topic is the underlying `Address` and the event data is
  `{amount: i128, to_muxed_id: u64}`; for a non-muxed `to` the data is the
  bare `amount: i128`. The muxed id is not persisted anywhere else.
| `approve` | `["approve", from: Address, spender: Address]` | `[amount: i128, expiration_ledger: u32]` | ✅ |
| `burn` | `["burn", from: Address]` | `amount: i128` | ✅ (emitted by `burn` and `burn_from`) |
| `mint` (admin interface, not required by SEP-41) | `["mint", to: Address]` | `amount: i128` | ✅ |

Before this change the token emitted **no events at all**, so wallets and
indexers following SEP-41 could not observe PayStream token movements.

## Admin / non-SEP-41 functions

These are PayStream extensions and are outside SEP-41: `initialize`, `mint`
(with replay-protection nonce), `admin_nonce`, `set_metadata`,
`allowance_with_expiration`, `upgrade`, `migrate`, `propose_admin`,
`accept_admin`, `total_supply`.

## Deviations and deferred items

| Item | Status | Rationale |
|---|---|---|
| Zero-amount `transfer` / `transfer_from` / `burn` / `burn_from` rejected | ⚠️ deferred | SEP-41 permits `amount == 0` (only negatives must fail). The stream contract never sends 0-amount transfers (it guards every call with `> 0`), and rejecting 0 has caught integration bugs. Relaxing it changes existing error behaviour, so it is left for a maintainer decision. To be fully strict-compliant, change the `amount > 0` checks to `amount >= 0` and treat 0 as a no-op that still emits the event. |
| Clawback / `set_authorized` / `authorized` | ⏳ out of scope | Part of the Stellar Asset Contract admin interface, not SEP-41. Not planned for PayStream token. |
| Events via `#[contractevent]` | ⏳ deferred | soroban-sdk 28 deprecates `Events::publish`; the topic layout above is produced with `publish` so it matches SEP-41 exactly. Migrating to `#[contractevent]` types is a follow-up refactor with no wire change. |

Follow-up issues for the deferred rows should be filed by a maintainer before
mainnet (this PR does not open issues on the upstream tracker).

## How compliance is verified

- `contracts/token/src/test.rs` → `mod sep41`: metadata defaults and
  set-once, expired allowance → 0, negative approve / `transfer_from`
  rejected, the exact topics/data of `transfer`, muxed `transfer`, `approve`,
  `burn` and `mint` events, `test_transfer_to_muxed_recipient` (a transfer to
  a muxed recipient credits the underlying address and emits `to_muxed_id`),
  and a round-trip through
  `soroban_sdk::token::TokenClient` (the generic SEP-41 client) to prove the
  ABI decodes under the standard interface.
- `contracts/token/fuzz` (TOK-19) fuzzes transfer, allowance and mint/burn
  invariants.
