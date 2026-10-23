# Read-Only Fee Split Preview (`preview_fee_split`)

This document describes the read-only fee split preview feature exposed by the `vault-registry` contract (#35).

## Overview

MindVault records on-chain payment receipts for resource purchases and leases. To provide callers, payment facilitators, and AI agents with complete transparency before initiating or signing an x402 payment, the contract provides `preview_fee_split(amount)`.

This read-only query returns the exact integer breakdown in USDC stroops of:

1. `platform_fee`: The total platform cut, partitioned between the fee recipient and optional destination.
2. `destination_fee`: The portion of the platform fee directed to a special destination (such as `Burn` or `Charity(address)`).
3. `recipient_fee`: The net platform fee remaining for the configured `fee_recipient`.
4. `royalty_fee`: The creator royalty cut in stroops.
5. `total_fee`: The sum of all deducted fees (`platform_fee + royalty_fee`).
6. `creator_remainder`: The net amount payable to the resource creator (`amount - total_fee`).

## Integer Precision and Rounding Rules

All calculations use integer division truncating towards zero (floor division for positive numbers):

- `raw_platform_fee = amount * platform_fee_bps / 10_000`
- `royalty_fee = amount * royalty_bps / 10_000`
- `destination_fee = raw_platform_fee * destination.bps / 10_000`
- `recipient_fee = raw_platform_fee - destination_fee` (if `fee_recipient` is configured)
- `platform_fee = destination_fee + recipient_fee`
- `total_fee = platform_fee + royalty_fee`
- `creator_remainder = amount - total_fee`

### Exact Conservation Invariants

- `destination_fee + recipient_fee == platform_fee`
- `platform_fee + royalty_fee == total_fee`
- `creator_remainder + total_fee == amount`

Every stroop is accounted for with zero rounding leakage.

## Method Interface

```rust
pub fn preview_fee_split(env: Env, amount: i128) -> Result<FeeSplitPreview, Error>;
```

### Amount Bound Validation

- `amount <= 0` returns `Error::InvalidPrice`.
- `amount > MAX_PRICE` returns `Error::PriceExceedsMax`.
