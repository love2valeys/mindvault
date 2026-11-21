# Verifier Status Query Pagination Design

**Status:** Implemented in `contract/contracts/vault-registry/src/lib.rs` (see
`list_by_verification_status`, `get_verifier_status_history`, and
`list_role_members`).

## Context

Agents, verifiers, moderation dashboards, and off-chain indexers need bounded,
deterministic reads over three verification-related datasets in
`vault-registry`:

1. **Resources by verification status** — e.g. a review queue of `Pending`
   resources, an audited catalog of `Verified` resources, or compliance sweeps
   for `Rejected` and `Revoked` resources.
2. **Per-verifier status-change history** — a chronological audit trail of
   every verification decision (`set_verification_status` and
   `revoke_attestation`) recorded by a given verifier address.
3. **Active verifier roster** — the bounded set of addresses currently granted
   the `Verifier` role.

Soroban imposes per-transaction CPU, memory, and ledger-footprint limits, so
every listing query must be bounded, resumable, and resilient to concurrent
state updates between pages.

## Query Surfaces

### 1. `list_by_verification_status(status, cursor, limit) -> CatalogPage`

```rust
pub fn list_by_verification_status(
    env: Env,
    status: VerificationStatus,
    cursor: u32,
    limit: u32,
) -> CatalogPage
```

- **Filter domain**: all four `VerificationStatus` variants — `Pending`,
  `Verified`, `Rejected`, and `Revoked`.
- **Ordering**: deterministic global registration order (`DataKey::Index(0..Count)`).
- **Cursor domain**: 0-based global catalog slot (`u32`), matching `list_page`,
  `list_listed_page`, `list_by_dispute_status_page`, and `list_by_state`.
- **Page bound**: `limit` is clamped to `LIST_PAGE_CAP` (`20`).
- **Return shape**: `CatalogPage { items: Vec<Resource>, next_cursor: Option<u32> }`.
  `next_cursor` is `Some(next_slot)` whenever `next_slot < Count`, or `None` at
  end-of-list.
- **Implementation**: delegates to the shared `VaultRegistry::scan_catalog`
  helper, guaranteeing identical cursor, clamping, and TTL-refresh behavior
  across all catalog-backed filtered queries.

### 2. `get_verifier_status_history(verifier, cursor, limit) -> VerifierStatusPage`

```rust
pub fn get_verifier_status_history(
    env: Env,
    verifier: Address,
    cursor: u32,
    limit: u32,
) -> VerifierStatusPage
```

- **Backing storage**: append-only `DataKey::VerifierHistory(verifier)` storing
  `Vec<VerifierStatusEntry>` written by both `set_verification_status` and
  `revoke_attestation`.
- **Ordering**: chronological (oldest entry first; index `0` is the verifier's
  first recorded action).
- **Cursor domain**: 0-based index into `VerifierHistory(verifier)`.
- **Page bound**: `limit` is clamped to `LIST_PAGE_CAP` (`20`).
- **Return shape**: `VerifierStatusPage { items: Vec<VerifierStatusEntry>, next_cursor: Option<u32> }`.
- **Unknown / idle verifier**: returns an empty page with `next_cursor: None`
  and does not write or bump storage (not an error).

### 3. `list_role_members(RoleKind::Verifier, cursor, limit) -> RolePage`

- Pages through active verifier addresses in grant order, capped at
  `ROLE_PAGE_CAP` (`20`) and bounded by `MAX_ROLE_MEMBERS` (`50`). See
  [`role-inventory.md`](role-inventory.md).

## Cursor Semantics and Edge Cases

| Condition | `items` | `next_cursor` | Notes |
| --- | --- | --- | --- |
| Empty catalog / empty history | `[]` | `None` | Never errors. |
| `cursor >= total` | `[]` | `None` | Safe past-end termination. |
| `limit == 0` with `cursor < total` | `[]` | `Some(cursor)` | No entries scanned; caller retains exact resume slot. |
| `limit > LIST_PAGE_CAP` (20) | up to `20` items | `Some(next)` or `None` | Silently clamped to `LIST_PAGE_CAP`. |
| Sparse matches across slots `0..total` | matching subset | `Some(next_unscanned_slot)` or `None` | Cursor tracks absolute slot, **never** `cursor + items.len()`. |
| Status flips between two pages | at-most-once per slot | monotonic slot index | Append-only slots prevent skips and duplicates. |

### Why Slot-Based Cursors Are Stable

Both `DataKey::Index(i)` and `DataKey::VerifierHistory(verifier)` are
append-only sequences whose existing indices are never renumbered or shifted
left:

- If a resource at slot `0` changes from `Verified` to `Rejected` after page 1
  returned slots `[0, 1]` with `next_cursor = Some(2)`, page 2 resumes at slot
  `2` and never repeats slot `1` or skips slot `2`.
- If an unvisited resource at slot `3` transitions to `Revoked` before page 2
  reaches it, slot `3` is simply excluded from the `Verified` view when
  scanned.
- Each slot is visited at most once per walk, and its live on-chain status at
  the time that slot is scanned decides inclusion.

## TTL and Storage Tradeoffs

- **TTL bumps**:
  - `list_by_verification_status` calls `bump_persistent` on every scanned
    `DataKey::Index(i)` and `DataKey::Resource(id)` entry.
  - `get_verifier_status_history` calls `bump_persistent` on
    `DataKey::VerifierHistory(verifier)` whenever the history is non-empty.
- **No extra write amplification**: `list_by_verification_status` filters at
  read time over `Index(0..Count)` instead of maintaining four mutable
  per-status secondary index vectors. That avoids extra persistent writes on
  every `register`, `set_verification_status`, and `revoke_attestation` call.
- **Future scaling path**: if registry cardinality grows to the point where a
  filtered linear scan approaches Soroban's read-entry budget, an internal
  secondary index (keyed under `AuxKey`, since `DataKey` is at its 50-variant
  XDR cap) can replace the scan without changing the `list_by_verification_status`
  ABI or `CatalogPage` return type.

## Client Usage

```typescript
// Walk every Pending resource without recomputing offsets manually.
let cursor: number | undefined = 0;
while (cursor !== undefined) {
  const tx = await registry.list_by_verification_status({
    status: { tag: "Pending", values: undefined },
    cursor,
    limit: 20,
  });
  const page = tx.result;
  // process page.items
  cursor = page.next_cursor;
}

// Walk a verifier's audit log chronologically.
let histCursor: number | undefined = 0;
while (histCursor !== undefined) {
  const tx = await registry.get_verifier_status_history({
    verifier: verifierAddress,
    cursor: histCursor,
    limit: 20,
  });
  const page = tx.result;
  // process page.items
  histCursor = page.next_cursor;
}
```

## Tests

See `contract/contracts/vault-registry/src/test/verifier_status_pagination.rs`
and `contract/contracts/vault-registry/src/test/stellar_wave_804_805_806_808.rs`
for unit tests covering all four `VerificationStatus` variants (including
`Revoked`), sparse slot-based cursor progression, mid-walk status mutations,
`LIST_PAGE_CAP` clamping, zero-limit and past-end bounds, TTL refresh, and
per-verifier history pagination.
