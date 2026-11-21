# Cancelling an admin nomination

`cancel_admin_nomination()` lets the current admin withdraw a pending nomination made with `nominate_new_admin` before the nominee calls `accept_admin`.

- Auth: the current admin only.
- No admin set: `AdminNotSet`. No pending nomination: `PendingAdminNotSet` (the error enum is full, so no new code was added).
- Effect: clears the pending nominee, its expiry and its nominated-at ledger. A cancelled nominee calling `accept_admin` gets `AdminNominationExpired`, and the admin can nominate again straight away.
- Event: `nomcancel` with the cancelled nominee address as data, so indexers can drop the pending entry.
- Cancellation takes effect in the ledger it is called in; it is not gated by `ADMIN_NOMINATION_MIN_GAP`, so it can be used at any point before acceptance.

## Monitoring Pending Admin Expiry

Off-chain monitors can poll `pending_admin_expiry()` alongside `pending_admin()` to inspect the active nomination deadline ledger sequence (`Option<u32>`) and alert before a nomination expires.
