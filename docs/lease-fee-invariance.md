# Lease Pricing and Access Invariance Under Fee Configuration Changes

This document details the pricing and access invariance rules for time-limited access leases on the `vault-registry` contract (#37).

## Context & Invariant

MindVault allows users to purchase time-limited access leases (`buy_lease`, `record_lease`, `settle_lease`, `renew_lease`) for resources at fixed duration tiers (`Hour`, `Day`, `Week`). Each lease records:

- `amount`: The exact USDC stroop amount paid at recording time (`lease_price(resource_id, tier)`).
- `tier`: The duration tier.
- `start_ledger`: The ledger sequence where access begins.
- `expiry_ledger`: The exclusive ledger sequence where access terminates.
- `state`: The lifecycle state (`Pending`, `Active`, `Revoked`).

Administrators can update registry-wide fee configurations via:

- `set_fee_config`: Modifying `platform_fee_bps` and `royalty_bps`.
- `set_fee_recipient`: Modifying the platform fee recipient address.
- `set_fee_destination`: Configuring fee routing to `Burn` or `Charity(address)`.

Resource creators can disable future royalty routing with
`set_royalties_disabled(resource_id, true)`. The setting is creator-authorized,
defaults to false for existing resources, and is exposed by
`royalties_disabled`. `preview_fee_split_for_resource` applies the flag to the
royalty component while leaving platform and destination fees unchanged; the
existing `preview_fee_split(amount)` keeps its registry-wide behavior.

### Core Rule

**Existing active leases retain their original price terms and access rights regardless of subsequent global fee configuration updates.**

1. **Price Terms Retention**: The `amount` field in `Lease` reflects the price at which the lease was acquired. Fee configuration changes never rewrite or invalidate `Lease::amount`.
2. **Access Invariance**: `lease_is_active(resource_id, holder)` continues to return `true` without interruption throughout the entire duration until `expiry_ledger` is reached.
3. **No Retroactive Invalidation**: Administrative fee adjustments are policy updates governing future settlement distributions; they do not mutate, revoke, or terminate existing active leases.
4. **New Leases & Renewals**:
   - New leases created after a fee change calculate their terms from `lease_price(resource_id, tier)`.
   - Renewals (`renew_lease`) extend an active lease's `expiry_ledger` by the tier duration with zero access gap, matching `lease_price(resource_id, tier)`.
