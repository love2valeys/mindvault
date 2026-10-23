# Fee Preview and Recorded Payment Rounding Alignment

## Overview

MindVault provides price previews through `preview_fee_split(amount)` and records verified payment receipts via `record_payment` and `record_payment_idempotent`.
This document defines the rounding rules, invariants, and mathematical guarantees ensuring zero discrepancy between fee previews and recorded payment receipts, even for minimal prices (micro-payments/stroops) and boundary basis points.

## Exact Integer Invariants

All calculations operate entirely on integer USDC stroops (1 USDC = 10,000,000 stroops).
Fee calculations use basis points with denominator `FEE_BPS_DENOM = 10_000` (1 bps = 0.01%):

1. **Platform Fee**:
   $$\text{raw\_platform\_fee} = \lfloor \frac{\text{amount} \times \text{platform\_fee\_bps}}{10\,000} \rfloor$$

2. **Royalty Fee**:
   $$\text{royalty\_fee} = \lfloor \frac{\text{amount} \times \text{royalty\_bps}}{10\,000} \rfloor$$

3. **Destination Share (Burn / Charity)**:
   $$\text{destination\_fee} = \lfloor \frac{\text{raw\_platform\_fee} \times \text{destination\_bps}}{10\,000} \rfloor$$

4. **Fee Recipient Remainder**:
   $$\text{recipient\_fee} = \text{raw\_platform\_fee} - \text{destination\_fee}$$
   By deriving `recipient_fee` as the exact difference of `raw_platform_fee` and `destination_fee`, no rounding remainder is created or lost during destination routing:
   $$\text{destination\_fee} + \text{recipient\_fee} = \text{raw\_platform\_fee} = \text{platform\_fee}$$

5. **Creator Remainder**:
   $$\text{total\_fee} = \text{platform\_fee} + \text{royalty\_fee}$$
   $$\text{creator\_remainder} = \text{amount} - \text{total\_fee}$$
   Exact stroop conservation is preserved across all inputs:
   $$\text{creator\_remainder} + \text{total\_fee} = \text{amount}$$

## Small-Amount and Micro-Payment Behavior

For small prices where $\text{amount} \times \text{bps} < 10\,000$:

- Integer floor division produces `0` for the corresponding fee component.
- The creator remainder retains 100% of the stroops (`creator_remainder == amount`).
- No rounding errors or negative values occur.
- Total fee sum is always less than or equal to `amount`.

## Property Testing

The contract test suite verifies these guarantees across arbitrary boundaries using property tests (`fee_rounding_proptest.rs`):

- Random prices from 1 to 5,000,000 stroops.
- Varied fee configurations across valid `platform_fee_bps` (0–5,000), `royalty_bps` (0–5,000, bounded by `MAX_FEE_BPS`), and destination types (None, Burn, Charity).
- Verification that `preview_fee_split` exactly equals the payment amount in `record_payment` receipts.
- Verification that $\text{creator\_remainder} + \text{total\_fee} = \text{recorded.amount}$.
- Verification of non-negativity and exact partition invariants for all sub-components.
