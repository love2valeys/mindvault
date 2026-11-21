# Anchor status

`get_anchor_status(resource_id, buyer)` returns `Option<AnchorStatus>` so a client
can tell a missing anchor (`None`) from an in-progress (`Pending`), written
(`Confirmed`) or failed (`Failed`) one.

The status is derived on read from records that already exist, so no stored
struct changed and every anchor written before this method existed reads as
`Confirmed`.

| Status | Condition |
| ------ | --------- |
| `None` | no anchor, no failure record, no rejected attempts |
| `Pending` | rejected `attempt_anchor_purchase_receipt` attempts are on record and fewer than `MAX_ANCHOR_ATTEMPTS`; retries are still allowed |
| `Failed` | an `AnchorFailure` record exists (`record_anchor_failure`) or the attempt cap was reached |
| `Confirmed` | the anchor is written (`get_purchase_receipt` succeeds) |

Transitions: `None -> Pending -> Failed`, `None/Pending -> Failed` (settler-reported
failure), and `None/Pending/Failed -> Confirmed` (retry succeeds). `Confirmed` is
terminal; anchors are immutable and writing one clears the failure and attempt
records. The `anchor` event is emitted exactly when a pair becomes `Confirmed`,
and `anchrfail` accompanies every failed anchor attempt or failure record, so
reads and events agree.

## Anchor failure event (`anchrfail`)

Every non-reverting anchor failure path (`attempt_anchor_purchase_receipt`,
`record_anchor_failure`, and `retry_anchor_purchase_receipt`) publishes the
same structured Soroban event:

- **Topics**: `(symbol_short!("anchrfail"), resource_id)`
- **Payload**: `AnchorFailure { resource_id, buyer, receipt_hash, reason, ledger }`

| `AnchorFailureReason` | Emitted by | Resulting `AnchorStatus` |
| --- | --- | --- |
| `ResourceNotFound` | `attempt_anchor_purchase_receipt` | `Pending` (or `Failed` at attempt cap) |
| `InvalidReceiptHash` | `attempt_anchor_purchase_receipt`, `retry_anchor_purchase_receipt` | `Pending` / `Failed` |
| `DuplicateReceipt` | `attempt_anchor_purchase_receipt`, `retry_anchor_purchase_receipt` | `Confirmed` (existing anchor preserved) |
| `RetryTooSoon` | `attempt_anchor_purchase_receipt` | `Pending` |
| `AttemptsExhausted` | `attempt_anchor_purchase_receipt` | `Failed` |
| `VerifierUnavailable` | `record_anchor_failure` | `Failed` |
| `NoPaymentRecorded` | `retry_anchor_purchase_receipt` | `Failed` |
| `ReceiptHashMismatch` | `retry_anchor_purchase_receipt` | `Failed` |

TypeScript bindings must be regenerated (`pnpm contract:bindings`) if exported signatures change.
