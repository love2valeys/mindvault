# Admin recovery target rules

`recover_admin(new_admin)` replaces the admin immediately, so the replacement must not already hold an active governance position. The call is rejected with `SameAdmin` (the error enum is full, so no new code is added) when `new_admin` is:

| Target | Result |
| --- | --- |
| the current admin | `SameAdmin` |
| the recovery admin | `SameAdmin` |
| the pending admin, nomination still live | `SameAdmin` |
| the pending admin, nomination expired | accepted |
| any other address | accepted; admin set, pending nomination cleared |

Rejected requests change nothing: admin, recovery admin and the pending nomination are left as they were. Validation lives in `recovery_target_check.rs` and runs before any storage write.

## 2-of-2 dual-key bootstrap and key separation

Deployments that want built-in recovery for a lost or misconfigured primary admin key can bootstrap with `bootstrap_dual_admin(admin, recovery_admin)`:

- Both `admin` and `recovery_admin` must authorize the bootstrap call (2-of-2) and must be distinct addresses (`SameAdmin` otherwise).
- Only valid before any admin exists (`AdminAlreadySet` otherwise); `nominate_new_admin` remains the backward-compatible single-key bootstrap path.
- Role separation is preserved after bootstrap: `nominate_new_admin` rejects the active `recovery_admin` with `SameAdmin`, and `set_recovery_admin` rejects any live `pending_admin` with `SameAdmin`.
- When `recover_admin(new_admin)` transfers admin authority, it clears all pending nomination state (`PendingAdmin`, `PendingAdminExpiry`, and `PendingAdminNominatedAt`) so an old nominee can never accept afterward. Recovery remains callable even while the registry is paused.
