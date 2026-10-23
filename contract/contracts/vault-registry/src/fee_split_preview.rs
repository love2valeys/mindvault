//! Fee split preview calculation for proposed sale amounts (`preview_fee_split`).

use soroban_sdk::{contracttype, Env};

use crate::{
    DataKey, Error, FeeConfig, FeeDestinationConfig, FEE_BPS_DENOM, MAX_FEE_DESTINATION_BPS,
    MAX_PRICE,
};

/// Exact integer fee and creator remainder calculation for a price before
/// payment recording.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FeeSplitPreview {
    /// Proposed sale amount or price in USDC stroops.
    pub amount: i128,
    /// Total platform fee in USDC stroops (destination_fee + recipient_fee).
    pub platform_fee: i128,
    /// Portion of the platform fee routed to destination (Burn or Charity).
    pub destination_fee: i128,
    /// Portion of the platform fee routed to the platform fee_recipient.
    pub recipient_fee: i128,
    /// Creator royalty fee in USDC stroops.
    pub royalty_fee: i128,
    /// Sum of all fee deductions in USDC stroops (platform_fee + royalty_fee).
    pub total_fee: i128,
    /// Net remainder payable to the creator in USDC stroops (amount - total_fee).
    pub creator_remainder: i128,
}

/// Calculate the exact integer fee split for a proposed sale amount according to
/// the active registry FeeConfig and FeeDestination.
pub(crate) fn compute(env: &Env, amount: i128) -> Result<FeeSplitPreview, Error> {
    compute_with_royalty(env, amount, true)
}

/// Calculate a fee split, optionally suppressing the configured royalty.
pub(crate) fn compute_with_royalty(
    env: &Env,
    amount: i128,
    include_royalty: bool,
) -> Result<FeeSplitPreview, Error> {
    if amount <= 0 {
        return Err(Error::InvalidPrice);
    }
    if amount > MAX_PRICE {
        return Err(Error::PriceExceedsMax);
    }

    let fee_config = env
        .storage()
        .instance()
        .get::<DataKey, FeeConfig>(&DataKey::FeeConfig);
    let dest_config: FeeDestinationConfig = env
        .storage()
        .instance()
        .get(&DataKey::FeeDestination)
        .unwrap_or(FeeDestinationConfig {
            bps: 0,
            destination: crate::FeeDestination::None,
        });

    let (raw_platform_fee, royalty_fee) = match fee_config {
        Some(ref config) => {
            let pf = amount
                .saturating_mul(i128::from(config.platform_fee_bps))
                / i128::from(FEE_BPS_DENOM);
            let rf = if include_royalty {
                amount
                    .saturating_mul(i128::from(config.royalty_bps))
                    / i128::from(FEE_BPS_DENOM)
            } else {
                0
            };
            (pf, rf)
        }
        None => (0i128, 0i128),
    };

    let destination_fee = if raw_platform_fee > 0 && dest_config.bps > 0 {
        raw_platform_fee
            .saturating_mul(i128::from(dest_config.bps))
            / i128::from(MAX_FEE_DESTINATION_BPS)
    } else {
        0i128
    };

    let recipient_fee = if let Some(ref config) = fee_config {
        if config.fee_recipient.is_some() {
            raw_platform_fee.saturating_sub(destination_fee)
        } else {
            0i128
        }
    } else {
        0i128
    };

    let platform_fee = destination_fee.saturating_add(recipient_fee);
    let total_fee = platform_fee.saturating_add(royalty_fee);
    let creator_remainder = amount.saturating_sub(total_fee);

    Ok(FeeSplitPreview {
        amount,
        platform_fee,
        destination_fee,
        recipient_fee,
        royalty_fee,
        total_fee,
        creator_remainder,
    })
}
