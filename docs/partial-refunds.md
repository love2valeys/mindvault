# Partial Refund Accounting and Remaining Refundable Balance

This document details the on-chain partial refund accounting mechanism on the `vault-registry` contract (#36).

## Overview

When payments are settled via `settle_payment`, buyers can receive refunds up to the original payment amount ceiling within the configured refund deadline (`refund_window`).

Prior to this feature, a payment could only be refunded once or refunds lacked cumulative balance tracking in events. This enhancement adds:

1. **Multiple Partial Refunds**: A settled payment can receive multiple partial refunds until cumulative refunds reach the original payment amount.
2. **Ceiling Enforcement**: The contract strictly enforces that `amount <= payment.amount - cumulative_refunded_amount`. Any attempt to exceed the total payment errors with `PaymentAmountMismatch`.
3. **Idempotent Refund Recording**: Calling `record_creator_refund` or `record_admin_refund` with identical arguments (same `refund_id`, `payment_receipt_id`, `recipient`, `amount`, `tx_hash`, `initiated_by`, `admin_override`) returns `Ok(())` idempotently without double-counting the refund or altering balances. Duplicate refund IDs with conflicting parameters fail consistently with `ReceiptAlreadyExists`.
4. **Remaining Balance in Events**: Every `refund` event carries the updated `RefundReceipt` which includes `remaining_balance` (the remaining refundable balance after the refund).
5. **Read-only Getter**: `get_remaining_refundable(payment_receipt_id)` returns the exact remaining refundable balance in stroops without mutating state.

## Schema Changes

### `RefundReceipt`

```rust
pub struct RefundReceipt {
    pub refund_id: String,
    pub payment_receipt_id: String,
    pub resource_id: String,
    pub recipient: Address,
    pub amount: i128,
    pub tx_hash: String,
    pub initiated_by: Address,
    pub recorded_at: u32,
    pub admin_override: bool,
    pub remaining_balance: i128,
}
```

### Event: `refund`

- Topic: `(symbol_short!("refund"), refund_id)`
- Data: `RefundReceipt` (including `remaining_balance`)

## Public Interface

| Function                                                                                    | Auth      | Return                         | Description                                                                              |
| ------------------------------------------------------------------------------------------- | --------- | ------------------------------ | ---------------------------------------------------------------------------------------- |
| `record_creator_refund(creator, refund_id, payment_receipt_id, recipient, amount, tx_hash)` | `creator` | `Result<(), Error>`            | Record partial or full refund within refund window. Idempotent on identical retries.     |
| `record_admin_refund(admin, refund_id, payment_receipt_id, recipient, amount, tx_hash)`     | `admin`   | `Result<(), Error>`            | Record partial or full refund overriding refund window. Idempotent on identical retries. |
| `get_remaining_refundable(payment_receipt_id)`                                              | —         | `Result<i128, Error>`          | Read the remaining refundable balance for a payment receipt.                             |
| `get_refunded_amount(payment_receipt_id)`                                                   | —         | `Result<i128, Error>`          | Read the total cumulative amount refunded so far.                                        |
| `get_refund(refund_id)`                                                                     | —         | `Result<RefundReceipt, Error>` | Fetch refund receipt by id, including `remaining_balance`.                               |
