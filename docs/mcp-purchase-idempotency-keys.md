# Purchase idempotency keys

`mindvault_buy` sends an `Idempotency-Key` header with the payment request. The key
is derived deterministically from the network, the payer public key and the
resource id (`buy-<32 hex chars>`), so a retry after a transport reconnect or a
lost response reuses the original key instead of generating a new one. The server
can then resolve the retry to the original receipt rather than settling a second
payment.

An explicit key (1-128 characters of letters, digits, `.`, `_`, `:`, `-`) passed to
the internal `buy` helper takes precedence over the derived key.

If a response is lost, confirm the outcome with `mindvault_purchase_history` or
`mindvault_tx_status` before retrying. See `docs/mcp-x402-retry-classification.md`.

## On-chain receipt idempotency

On the Soroban `vault-registry` contract, strict `record_payment` rejects any
reused `receipt_id` with `ReceiptAlreadyExists`. For retry-safe settlement
recording (timed-out RPC submissions, facilitator restarts, or backup-settler
failover), use `record_payment_idempotent(settler, receipt_id, resource_id, payer, amount, tx_hash)`:

- Identical retries for an existing `receipt_id` return the stored
  `PaymentReceipt` unchanged with no storage write and no duplicate `payment`
  event, even after settlement, refunds, price updates, or post-sale resource
  state changes.
- Reusing a `receipt_id` with any differing `(resource_id, payer, amount, tx_hash)`
  field still fails with `ReceiptAlreadyExists`, and reusing a `tx_hash` under a
  different `receipt_id` still fails with `DuplicateTxHash`.
