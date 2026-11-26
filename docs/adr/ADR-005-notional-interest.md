# ADR-005: Notional / Compounding Interest for Salary Streams

- **Status:** Accepted
- **Date:** 2025-01-01
- **Issue:** #466 (SC-98)
- **Deciders:** Smart Contract Team
- **Related:** docs/adr/template.md

## Context

The current salary streaming implementation produces **linear** streams: a fixed
total amount is unlocked proportionally to elapsed time between a start and end
timestamp. Recipients accrue a constant amount per unit time and can withdraw
whatever has unlocked so far.

A request was raised (SC-98, #466) to research whether a **notional interest** or
**yield-bearing** stream is in scope for v0.3+. The motivation is that salary
paid out over time loses value relative to being paid upfront and invested, and
that some employers may wish to offer employees a yield-bearing compensation
product (e.g., a stream that accrues interest on the unwithdrawn balance, or a
stream denominated in a yield-bearing asset).

This ADR records the research findings and the decision on whether to pursue
notional/compounding interest streams in v0.3+.

Key considerations:

- **Accrual model.** Linear streams are simple, deterministic, and cheap to
  compute on-chain. Compounding interest requires either periodic accrual
  (stateful, gas-heavy) or a closed-form exponential accrual (requires fixed-point
  math and careful rounding).
- **Oracle dependency.** A *notional* interest rate that tracks a real-world
  benchmark (e.g., a money-market rate) requires an on-chain price/rate feed,
  introducing an external trust and manipulation surface.
- **Yield-bearing assets.** An alternative is to denominate the stream in an
  already yield-bearing token (e.g., a rebasing or exchange-rate-appreciating
  vault share) and let the asset itself carry the yield, keeping the streaming
  contract linear.
- **Regulatory.** Interest-bearing compensation can have tax and securities-law
  implications depending on jurisdiction; this is outside the contract's control
  but affects product scope.

## Decision

**Notional/compounding interest streams are OUT OF SCOPE for v0.3 and are
closed for the foreseeable roadmap.** The streaming primitive will remain
**linear-only**.

Rationale:

1. **Complexity vs. value.** On-chain compounding accrual adds significant
   state, math, and rounding complexity to a core primitive that is currently
   simple and auditable. The marginal product value does not justify the risk at
   this stage.
2. **Oracle risk.** A notional rate tied to a real benchmark requires an oracle.
   Introducing a price/rate feed into the salary-streaming path creates a
   manipulation and liveness dependency that is disproportionate to the feature.
3. **Cleaner alternative exists.** Users who want yield-bearing compensation can
   stream a **yield-bearing asset** (an external vault share or rebasing token)
   using the existing linear primitive. The yield is then a property of the
   asset, not of the streaming contract, keeping the contract oracle-free and
   linear.
4. **Regulatory ambiguity.** Interest-bearing salary has unresolved tax and
   securities implications that are better handled at the product/asset layer
   than baked into the core contract.

This decision **closes** the research item. No follow-up design issue is created
because the feature is not in scope. If demand materializes, the recommended path
is the "yield-bearing via external vault" alternative below, which can be
supported without changing the streaming contract.

## Alternatives Considered

### 1. Linear-only (chosen)

Keep the current linear accrual. Simple, deterministic, oracle-free, cheap to
compute, easy to audit. Yield, if desired, is achieved by choosing a
yield-bearing stream asset.

- **Pros:** Minimal risk, no oracle, no new math, preserves auditability.
- **Cons:** No built-in interest; users must opt into a yield-bearing asset
  themselves.

### 2. Notional interest with oracle

Accrue interest on the unwithdrawn balance at a rate sourced from an on-chain
oracle (e.g., a benchmark lending rate), compounded periodically or via a
closed-form exponential.

- **Pros:** Explicit, transparent interest on the stream balance.
- **Cons:** Oracle dependency (manipulation, staleness, liveness); compounding
  math and rounding complexity; higher gas; larger audit surface; regulatory
  ambiguity. **Rejected** for v0.3+.

### 3. Yield-bearing via external vault

Denominate the stream in a yield-bearing asset (vault share / rebasing token).
The streaming contract stays linear; the asset's exchange rate or rebase carries
the yield.

- **Pros:** No oracle in the streaming contract; reuses the linear primitive;
  yield logic lives in the (separately audited) asset.
- **Cons:** Depends on the external asset's security and liquidity; rebasing
  tokens require care in accounting. **Preferred path if demand arises**, but not
  implemented as part of this ADR.

## Consequences and Risks

- **Positive:** Core streaming primitive remains simple, oracle-free, and
  auditable. No new attack surface added to the salary path.
- **Negative:** No native interest feature; users wanting yield must select a
  yield-bearing asset themselves.
- **Oracle dependency (if alternative 2 were pursued):** A rate oracle would
  introduce a trusted external dependency. A compromised or stale feed could
  misprice accrual, and a manipulable feed could be exploited to inflate
  withdrawals. This risk is the primary reason alternative 2 is rejected.
- **Price-feed manipulation:** Any oracle-based accrual is exposed to
  manipulation (spot vs. TWAP, low-liquidity feeds). Avoided by not adopting an
  oracle.
- **Accrual complexity:** Compounding requires fixed-point exponential math and
  careful rounding; errors can cause insolvency or dust accumulation. Avoided by
  staying linear.
- **Regulatory implications:** Interest-bearing compensation may carry tax and
  securities obligations that vary by jurisdiction. Keeping the contract
  interest-free avoids embedding regulated behavior in the primitive; any
  yield-bearing asset choice is the integrator's responsibility.

## Follow-ups

- **None for v0.3.** This research item is **closed**; notional/compounding
  interest is not in scope.
- If product demand emerges, open a new design issue to evaluate the
  **yield-bearing via external vault** alternative (alternative 3), which does
  not require changes to the linear streaming contract.
- Document in user-facing docs that yield-bearing compensation is achieved by
  streaming a yield-bearing asset, not by native interest accrual.