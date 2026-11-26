# Architecture Decision Records

| ADR | Title |
|---|---|
| [ADR-001](ADR-001-i128-amounts.md) | Use `i128` for token amounts |
| [ADR-002](ADR-002-per-second-accrual.md) | Per-second accrual instead of per-ledger |
| [ADR-003](ADR-003-native-receipts.md) | Native receipts instead of a separate NFT contract |
| [ADR-004](ADR-004-persistent-stream-storage.md) | Persistent storage for streams |
| [ADR-005](ADR-005-notional-interest.md) | Notional interest / yield-bearing streams (out of scope for v0.3) |

New ADRs: copy [template.md](template.md), take the next number, and open a PR.

## Roadmap

### v0.3

- **Notional interest / yield-bearing streams (#466, SC-98):** Researched and recorded in
  [ADR-005](ADR-005-notional-interest.md). Decision: **out of scope for v0.3**. Salary streams
  remain linear. The design, the oracle dependency risk, and the conditions for revisiting are
  documented in the ADR. If the decision is reversed, a follow-up design issue will be created
  and linked from ADR-005.