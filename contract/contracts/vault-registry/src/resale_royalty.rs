//! Creator opt-in resale royalty (capped transfer fee) captured on secondary
//! ownership transfers (`transfer_ownership`, `accept_transfer`, and
//! `transfer_ownership_with_terms`).
//!
//! Because `vault-registry` is non-custodial, it does not hold or transfer
//! USDC itself. Instead, an original creator may opt into a bounded
//! basis-point transfer royalty (up to [`MAX_RESALE_ROYALTY_BPS`] = 1,000 bp =
//! 10 %) while still owning the resource. Once ownership moves away from the
//! original creator, the configured rate and recipient are locked so neither a
//! reseller can remove them nor a departed creator can retroactively raise
//! fees on later owners.
//!
//! On every secondary transfer (where neither the seller nor the buyer is the
//! original creator or the royalty beneficiary), the contract computes the
//! exact integer floor royalty (`floor(price * royalty_bps / FEE_BPS_DENOM)`),
//! accumulates audit counters under `AuxKey::ResaleRoyaltySummary`, and emits
//! a canonical `resaleroy` event so settlement services and indexers can
//! enforce and reconcile payouts.
//!
//! All state lives under [`AuxKey`] so [`Resource`] (and
//! `RESOURCE_SCHEMA_VERSION`) stays unchanged and resources that never opt in
//! preserve 100 % backwards-compatible transfer behavior.

use soroban_sdk::{contracttype, symbol_short, Address, Env, String};

use crate::{AuxKey, Error, VaultRegistry, FEE_BPS_DENOM, MAX_PRICE};

/// Upper bound on creator opt-in resale royalty in basis points: 10 % (1,000 bp).
pub const MAX_RESALE_ROYALTY_BPS: u32 = 1_000;

/// Creator opt-in resale royalty configuration for one resource.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ResaleRoyaltyConfig {
    /// Original creator who registered the resource and configured the royalty.
    pub original_creator: Address,
    /// Transfer royalty rate in basis points (`1..=MAX_RESALE_ROYALTY_BPS`).
    pub royalty_bps: u32,
    /// Beneficiary entitled to receive the resale royalty.
    pub recipient: Address,
}

/// Exact integer preview of the resale royalty due on a secondary transfer.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ResaleRoyaltyPreview {
    /// Resource ID queried.
    pub id: String,
    /// Reference transfer/resale amount in USDC base units.
    pub basis_amount: i128,
    /// Effective resale royalty rate in basis points (`0` when unset).
    pub royalty_bps: u32,
    /// Royalty fee owed in USDC base units (`floor(basis_amount * royalty_bps / 10_000)`).
    pub royalty_amount: i128,
    /// Net remainder payable to the seller (`basis_amount - royalty_amount`).
    pub seller_remainder: i128,
    /// Configured beneficiary, or `None` when resale royalty is unset/disabled.
    pub recipient: Option<Address>,
}

/// Cumulative on-chain audit summary of resale royalties captured on
/// secondary ownership transfers for a resource.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ResaleRoyaltySummary {
    /// Total resale royalties captured across all secondary transfers (USDC base units).
    pub total_captured: i128,
    /// Number of secondary transfers that captured a resale royalty.
    pub resale_count: u32,
    /// Royalty captured on the most recent secondary transfer.
    pub last_royalty_amount: i128,
    /// Resource price used as basis on the most recent secondary transfer.
    pub last_basis_amount: i128,
    /// Ledger sequence of the most recent capture (`0` when none yet).
    pub last_ledger: u32,
}

/// Structured event payload emitted on topic `("resaleroy", id)` when a
/// secondary ownership transfer captures an opt-in resale royalty.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ResaleRoyaltyCaptured {
    pub id: String,
    pub from: Address,
    pub to: Address,
    pub recipient: Address,
    pub basis_amount: i128,
    pub royalty_bps: u32,
    pub royalty_amount: i128,
    pub ledger: u32,
}

/// Structured event payload emitted on topic `("setresroy", id)` when the
/// original creator configures, updates, or clears opt-in resale royalty terms.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ResaleRoyaltyUpdated {
    pub id: String,
    pub original_creator: Address,
    pub old_bps: u32,
    pub new_bps: u32,
    pub recipient: Option<Address>,
}

/// Exact integer floor calculation matching `fee_split_preview.rs`.
pub(crate) fn compute_fee(basis_amount: i128, royalty_bps: u32) -> i128 {
    if basis_amount <= 0 || royalty_bps == 0 {
        return 0;
    }
    basis_amount.saturating_mul(i128::from(royalty_bps)) / i128::from(FEE_BPS_DENOM)
}

/// Configure, update, or clear (`royalty_bps == 0`) opt-in resale royalty for
/// `id`. Only the original creator while still owning the resource may call
/// this; once ownership transfers away, terms are locked for secondary
/// holders.
pub(crate) fn set(
    env: &Env,
    id: String,
    royalty_bps: u32,
    recipient: Option<Address>,
) -> Result<(), Error> {
    VaultRegistry::require_not_paused(env)?;
    VaultRegistry::validate_resource_id(&id)?;
    let resource = VaultRegistry::load(env, &id)?;
    resource.creator.require_auth();
    VaultRegistry::ensure_mutable(&resource)?;

    if royalty_bps > MAX_RESALE_ROYALTY_BPS {
        return Err(Error::FeeBpsTooHigh);
    }

    let orig_key = AuxKey::OriginalCreator(id.clone());
    let original_creator: Address = match env.storage().persistent().get(&orig_key) {
        Some(stored) => {
            if stored != resource.creator {
                return Err(Error::Unauthorized);
            }
            VaultRegistry::bump_persistent(env, &orig_key);
            stored
        }
        None => {
            env.storage().persistent().set(&orig_key, &resource.creator);
            VaultRegistry::bump_persistent(env, &orig_key);
            resource.creator.clone()
        }
    };

    let cfg_key = AuxKey::ResaleRoyalty(id.clone());
    let old_bps = env
        .storage()
        .persistent()
        .get::<AuxKey, ResaleRoyaltyConfig>(&cfg_key)
        .map(|c| c.royalty_bps)
        .unwrap_or(0);

    let event_recipient = if royalty_bps == 0 {
        if env.storage().persistent().has(&cfg_key) {
            env.storage().persistent().remove(&cfg_key);
        }
        None
    } else {
        let resolved_recipient = recipient.unwrap_or_else(|| original_creator.clone());
        let config = ResaleRoyaltyConfig {
            original_creator: original_creator.clone(),
            royalty_bps,
            recipient: resolved_recipient.clone(),
        };
        env.storage().persistent().set(&cfg_key, &config);
        VaultRegistry::bump_persistent(env, &cfg_key);
        Some(resolved_recipient)
    };

    env.events().publish(
        (symbol_short!("setresroy"), id.clone()),
        ResaleRoyaltyUpdated {
            id,
            original_creator,
            old_bps,
            new_bps: royalty_bps,
            recipient: event_recipient,
        },
    );
    Ok(())
}

/// Read the active opt-in resale royalty configuration for `id`, or `None`
/// when the creator has not opted in.
pub(crate) fn get(env: &Env, id: String) -> Result<Option<ResaleRoyaltyConfig>, Error> {
    VaultRegistry::validate_resource_id(&id)?;
    VaultRegistry::load(env, &id)?;
    let cfg_key = AuxKey::ResaleRoyalty(id);
    let cfg: Option<ResaleRoyaltyConfig> = env.storage().persistent().get(&cfg_key);
    if cfg.is_some() {
        VaultRegistry::bump_persistent(env, &cfg_key);
    }
    Ok(cfg)
}

/// Preview the exact resale royalty for `id` against `sale_amount` (or the
/// resource's current listed `price` when `sale_amount` is `None`).
pub(crate) fn preview(
    env: &Env,
    id: String,
    sale_amount: Option<i128>,
) -> Result<ResaleRoyaltyPreview, Error> {
    VaultRegistry::validate_resource_id(&id)?;
    let resource = VaultRegistry::load(env, &id)?;

    let basis_amount = match sale_amount {
        Some(amount) => {
            if amount <= 0 {
                return Err(Error::InvalidPrice);
            }
            if amount > MAX_PRICE {
                return Err(Error::PriceExceedsMax);
            }
            amount
        }
        None => resource.price,
    };

    let cfg_key = AuxKey::ResaleRoyalty(id.clone());
    let cfg: Option<ResaleRoyaltyConfig> = env.storage().persistent().get(&cfg_key);
    if cfg.is_some() {
        VaultRegistry::bump_persistent(env, &cfg_key);
    }

    let (royalty_bps, royalty_amount, recipient) = match cfg {
        Some(c) if c.royalty_bps > 0 => {
            let fee = compute_fee(basis_amount, c.royalty_bps);
            (c.royalty_bps, fee, Some(c.recipient))
        }
        _ => (0, 0, None),
    };
    let seller_remainder = basis_amount.saturating_sub(royalty_amount);

    Ok(ResaleRoyaltyPreview {
        id,
        basis_amount,
        royalty_bps,
        royalty_amount,
        seller_remainder,
        recipient,
    })
}

/// Read the cumulative resale royalty summary for `id`. Returns zeros when no
/// secondary transfer with an active resale royalty has occurred yet.
pub(crate) fn summary(env: &Env, id: String) -> Result<ResaleRoyaltySummary, Error> {
    VaultRegistry::validate_resource_id(&id)?;
    VaultRegistry::load(env, &id)?;
    let sum_key = AuxKey::ResaleRoyaltySummary(id);
    match env
        .storage()
        .persistent()
        .get::<AuxKey, ResaleRoyaltySummary>(&sum_key)
    {
        Some(stored) => {
            VaultRegistry::bump_persistent(env, &sum_key);
            Ok(stored)
        }
        None => Ok(ResaleRoyaltySummary {
            total_captured: 0,
            resale_count: 0,
            last_royalty_amount: 0,
            last_basis_amount: 0,
            last_ledger: 0,
        }),
    }
}

/// Hook invoked whenever ownership completes a transfer (`transfer_ownership`,
/// `accept_transfer`, or `transfer_ownership_with_terms`).
///
/// 1. Locks `AuxKey::OriginalCreator(id)` on the very first transfer if not
///    already recorded, so a later owner can never impersonate the original
///    creator.
/// 2. If the resource has an active opt-in resale royalty (`royalty_bps > 0`)
///    and this is a secondary transfer (neither seller nor buyer is the
///    original creator or the royalty beneficiary), records the captured
///    transfer royalty in `AuxKey::ResaleRoyaltySummary(id)` and emits
///    `("resaleroy", id)`.
/// 3. When no resale royalty is configured, emits nothing and writes no
///    royalty summary — preserving full backwards compatibility.
pub(crate) fn on_ownership_transfer(
    env: &Env,
    id: &String,
    previous_owner: &Address,
    new_owner: &Address,
    basis_amount: i128,
) {
    let orig_key = AuxKey::OriginalCreator(id.clone());
    let original_creator: Address = match env.storage().persistent().get(&orig_key) {
        Some(stored) => {
            VaultRegistry::bump_persistent(env, &orig_key);
            stored
        }
        None => {
            env.storage().persistent().set(&orig_key, previous_owner);
            VaultRegistry::bump_persistent(env, &orig_key);
            previous_owner.clone()
        }
    };

    let cfg_key = AuxKey::ResaleRoyalty(id.clone());
    let cfg: Option<ResaleRoyaltyConfig> = env.storage().persistent().get(&cfg_key);
    let Some(cfg) = cfg else {
        return;
    };
    VaultRegistry::bump_persistent(env, &cfg_key);

    if cfg.royalty_bps == 0
        || *previous_owner == original_creator
        || *previous_owner == cfg.recipient
        || *new_owner == original_creator
        || *new_owner == cfg.recipient
    {
        return;
    }

    let royalty_amount = compute_fee(basis_amount, cfg.royalty_bps);
    let ledger = env.ledger().sequence();

    let sum_key = AuxKey::ResaleRoyaltySummary(id.clone());
    let mut sum = env
        .storage()
        .persistent()
        .get::<AuxKey, ResaleRoyaltySummary>(&sum_key)
        .unwrap_or(ResaleRoyaltySummary {
            total_captured: 0,
            resale_count: 0,
            last_royalty_amount: 0,
            last_basis_amount: 0,
            last_ledger: 0,
        });
    sum.total_captured = sum.total_captured.saturating_add(royalty_amount);
    sum.resale_count = sum.resale_count.saturating_add(1);
    sum.last_royalty_amount = royalty_amount;
    sum.last_basis_amount = basis_amount;
    sum.last_ledger = ledger;
    env.storage().persistent().set(&sum_key, &sum);
    VaultRegistry::bump_persistent(env, &sum_key);

    env.events().publish(
        (symbol_short!("resaleroy"), id.clone()),
        ResaleRoyaltyCaptured {
            id: id.clone(),
            from: previous_owner.clone(),
            to: new_owner.clone(),
            recipient: cfg.recipient,
            basis_amount,
            royalty_bps: cfg.royalty_bps,
            royalty_amount,
            ledger,
        },
    );
}
