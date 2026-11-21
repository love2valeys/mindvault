// ── Fee preview and recorded payment rounding property tests (#38) ───────────
//
// Acceptance criteria:
// - Property tests compare preview calculations with persisted receipt amounts
//   across fee and price boundaries.
// - Check all fee calculations for rounding discrepancies that could make a
//   quoted split differ from a recorded receipt.

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]
    #[test]
    fn test_fee_preview_aligns_with_persisted_receipt(
        price_raw in 1i128..=5_000_000i128,
        platform_fee_bps in 0u32..=5_000u32,
        royalty_seed in 0u32..=5_000u32,
        dest_bps in 0u32..=10_000u32,
        dest_kind in 0u8..=2u8,
    ) {
        // Combined fee cannot exceed MAX_FEE_BPS (5_000)
        let max_royalty = MAX_FEE_BPS.saturating_sub(platform_fee_bps);
        let royalty_bps = if max_royalty > 0 {
            royalty_seed % (max_royalty + 1)
        } else {
            0
        };

        // Price in stroops must strictly exceed combined_fee_bps for registration
        let combined_fee_bps = (platform_fee_bps + royalty_bps) as i128;
        let price = price_raw.max(combined_fee_bps + 1);

        let (env, creator, settler, _holder, client) = lease_setup();
        let admin = client.admin().unwrap();
        let payer = Address::generate(&env);

        // Configure fee recipient
        let fee_recipient = Some(admin.clone());

        client.set_fee_config(&FeeConfig {
            platform_fee_bps,
            royalty_bps,
            fee_recipient,
        });

        // Configure fee destination
        let dest_config = match dest_kind {
            1 if platform_fee_bps > 0 && dest_bps > 0 => FeeDestinationConfig {
                bps: dest_bps,
                destination: FeeDestination::Burn,
            },
            2 if platform_fee_bps > 0 && dest_bps > 0 => FeeDestinationConfig {
                bps: dest_bps,
                destination: FeeDestination::Charity(Address::generate(&env)),
            },
            _ => FeeDestinationConfig {
                bps: 0,
                destination: FeeDestination::None,
            },
        };
        client.set_fee_destination(&dest_config);

        // Register resource and record payment receipt
        let res_id = register_priced(&env, &creator, &client, "propfee", price);
        let receipt_id = String::from_str(&env, "rcpt_propfee");
        let tx_hash = String::from_str(&env, "tx_propfee");

        let recorded = client.record_payment(
            &settler,
            &receipt_id,
            &res_id,
            &payer,
            &price,
            &tx_hash,
        );

        // 1. Quoted preview calculation matches persisted receipt amount
        let preview = client.preview_fee_split(&price);
        assert_eq!(
            preview.amount,
            recorded.amount,
            "Preview amount must exactly equal persisted receipt amount"
        );

        // 2. Exact integer conservation invariant across all boundaries
        assert_eq!(
            preview.creator_remainder + preview.total_fee,
            recorded.amount,
            "Creator remainder plus total fee must exactly equal receipt amount"
        );

        // 3. Platform fee partition invariant
        assert_eq!(
            preview.destination_fee + preview.recipient_fee,
            preview.platform_fee,
            "Destination fee plus recipient fee must exactly equal platform fee"
        );

        // 4. Total fee composition invariant
        assert_eq!(
            preview.total_fee,
            preview.platform_fee + preview.royalty_fee,
            "Total fee must exactly equal platform fee plus royalty fee"
        );

        // 5. Non-negativity across small and large prices
        assert!(preview.creator_remainder >= 0, "Creator remainder cannot be negative");
        assert!(preview.platform_fee >= 0, "Platform fee cannot be negative");
        assert!(preview.destination_fee >= 0, "Destination fee cannot be negative");
        assert!(preview.recipient_fee >= 0, "Recipient fee cannot be negative");
        assert!(preview.royalty_fee >= 0, "Royalty fee cannot be negative");
        assert!(preview.total_fee >= 0, "Total fee cannot be negative");
    }
}

#[test]
fn test_small_amounts_exact_rounding_alignment() {
    let (env, creator, settler, _holder, client) = lease_setup();
    let admin = client.admin().unwrap();
    let payer = Address::generate(&env);

    // Platform fee 500 bps (5%), Royalty 250 bps (2.5%)
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 500,
        royalty_bps: 250,
        fee_recipient: Some(admin),
    });
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 5_000, // 50% of platform fee
        destination: FeeDestination::Burn,
    });

    let small_prices = [
        1i128, 2, 3, 5, 10, 19, 20, 21, 50, 100, 200, 500, 750, 999, 1000, 1001,
        2500, 5000, 7501, 9999, 10000, 10001
    ];

    for (i, &price) in small_prices.iter().enumerate() {
        let preview = client.preview_fee_split(&price);

        // Preview checks
        assert_eq!(preview.amount, price);
        assert_eq!(preview.creator_remainder + preview.total_fee, price);
        assert_eq!(preview.destination_fee + preview.recipient_fee, preview.platform_fee);
        assert_eq!(preview.total_fee, preview.platform_fee + preview.royalty_fee);
        assert!(preview.creator_remainder >= 0);

        // If price is above combined_fee_bps (750), record payment and compare with receipt
        if price > 750 {
            let res_name = format!("sm{}", i);
            let res_id = register_priced(&env, &creator, &client, &res_name, price);
            let rcpt_id = format!("rcpt{}", i);
            let rcpt_str = String::from_str(&env, &rcpt_id);
            let tx_str = String::from_str(&env, &format!("tx{}", i));

            let receipt = client.record_payment(
                &settler,
                &rcpt_str,
                &res_id,
                &payer,
                &price,
                &tx_str,
            );

            assert_eq!(preview.amount, receipt.amount);
            assert_eq!(preview.creator_remainder + preview.total_fee, receipt.amount);
        }
    }
}
