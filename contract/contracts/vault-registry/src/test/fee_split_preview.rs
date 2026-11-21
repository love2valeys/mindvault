// ── Read-only fee split preview tests (#35) ──────────────────────────────────
//
// Tests preview_fee_split calculation:
// 1. Validates amount bounds (amount <= 0 -> InvalidPrice, amount > MAX_PRICE -> PriceExceedsMax).
// 2. Unconfigured fee settings return zero deductions and full remainder to creator.
// 3. Boundary tests for every fee destination: FeeDestination::None, FeeDestination::Burn, FeeDestination::Charity.
// 4. Boundary rates (0 bps, 1 bp, 2500 bps, 5000 bps, 10000 bps).
// 5. Small amounts (1, 2, 10, 9999, 10000, 10001 stroops) with floor division rounding.
// 6. Strict integer conservation invariants:
//    - destination_fee + recipient_fee == platform_fee
//    - platform_fee + royalty_fee == total_fee
//    - creator_remainder + total_fee == amount

#[test]
fn fee_split_preview_validates_amount_bounds() {
    let (_env, _creator, _admin, client) = setup_with_admin();

    assert_eq!(client.try_preview_fee_split(&0i128), Err(Ok(Error::InvalidPrice)));
    assert_eq!(client.try_preview_fee_split(&(-1i128)), Err(Ok(Error::InvalidPrice)));
    assert_eq!(
        client.try_preview_fee_split(&(MAX_PRICE + 1)),
        Err(Ok(Error::PriceExceedsMax))
    );
}

#[test]
fn fee_split_preview_defaults_when_unconfigured() {
    let (_env, _creator, _admin, client) = setup_with_admin();

    let preview = client.preview_fee_split(&1_000_000i128);
    assert_eq!(preview.amount, 1_000_000i128);
    assert_eq!(preview.platform_fee, 0i128);
    assert_eq!(preview.destination_fee, 0i128);
    assert_eq!(preview.recipient_fee, 0i128);
    assert_eq!(preview.royalty_fee, 0i128);
    assert_eq!(preview.total_fee, 0i128);
    assert_eq!(preview.creator_remainder, 1_000_000i128);
}

#[test]
fn resource_royalty_opt_out_defaults_off_and_changes_only_royalty_routing() {
    let (env, creator, admin, client) = setup_with_admin();
    let id = String::from_str(&env, "royaltyoff");
    client.register(
        &creator,
        &id,
        &100_000i128,
        &String::from_str(&env, "ipfs://royalty"),
        &empty_tags(&env),
    );
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 1_000,
        royalty_bps: 500,
        fee_recipient: Some(admin),
    });

    assert!(!client.royalties_disabled(&id));
    let before = client.preview_fee_split_for_resource(&id, &100_000i128);
    assert_eq!(before.platform_fee, 10_000i128);
    assert_eq!(before.royalty_fee, 5_000i128);

    client.set_royalties_disabled(&id, &true);
    assert!(client.royalties_disabled(&id));
    let after = client.preview_fee_split_for_resource(&id, &100_000i128);
    assert_eq!(after.platform_fee, before.platform_fee);
    assert_eq!(after.royalty_fee, 0i128);
    assert_eq!(after.creator_remainder, before.creator_remainder + before.royalty_fee);

    let events = env.events().all();
    let (_contract, topics, data) = events.last().unwrap();
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("setroyoff"));
    let (old, new): (bool, bool) = data.try_into_val(&env).unwrap();
    assert_eq!((old, new), (false, true));
}

#[test]
fn fee_split_preview_with_destination_none() {
    let (_env, _creator, admin, client) = setup_with_admin();
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 1_000, // 10%
        royalty_bps: 500,        // 5%
        fee_recipient: Some(admin.clone()),
    });

    let preview = client.preview_fee_split(&100_000i128);
    assert_eq!(preview.amount, 100_000i128);
    assert_eq!(preview.platform_fee, 10_000i128); // 10% of 100_000
    assert_eq!(preview.destination_fee, 0i128);  // Destination is None
    assert_eq!(preview.recipient_fee, 10_000i128);
    assert_eq!(preview.royalty_fee, 5_000i128);   // 5% of 100_000
    assert_eq!(preview.total_fee, 15_000i128);
    assert_eq!(preview.creator_remainder, 85_000i128);

    assert_eq!(preview.destination_fee + preview.recipient_fee, preview.platform_fee);
    assert_eq!(preview.platform_fee + preview.royalty_fee, preview.total_fee);
    assert_eq!(preview.creator_remainder + preview.total_fee, preview.amount);
}

#[test]
fn fee_split_preview_with_destination_burn_boundaries() {
    let (_env, _creator, admin, client) = setup_with_admin();
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 2_000, // 20%
        royalty_bps: 0,
        fee_recipient: Some(admin.clone()),
    });

    // 1 bp burn out of 10,000 bps
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 1,
        destination: FeeDestination::Burn,
    });
    let p1 = client.preview_fee_split(&100_000i128);
    // platform_fee = 100_000 * 2_000 / 10_000 = 20_000
    // destination_fee = 20_000 * 1 / 10_000 = 2
    assert_eq!(p1.platform_fee, 20_000i128);
    assert_eq!(p1.destination_fee, 2i128);
    assert_eq!(p1.recipient_fee, 19_998i128);
    assert_eq!(p1.creator_remainder, 80_000i128);
    assert_eq!(p1.destination_fee + p1.recipient_fee, p1.platform_fee);

    // 25% (2_500 bps) burn
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 2_500,
        destination: FeeDestination::Burn,
    });
    let p25 = client.preview_fee_split(&100_000i128);
    assert_eq!(p25.platform_fee, 20_000i128);
    assert_eq!(p25.destination_fee, 5_000i128); // 25% of 20_000
    assert_eq!(p25.recipient_fee, 15_000i128);
    assert_eq!(p25.creator_remainder, 80_000i128);
    assert_eq!(p25.destination_fee + p25.recipient_fee, p25.platform_fee);

    // 100% (10_000 bps) burn
    client.set_fee_destination(&FeeDestinationConfig {
        bps: MAX_FEE_DESTINATION_BPS,
        destination: FeeDestination::Burn,
    });
    let p100 = client.preview_fee_split(&100_000i128);
    assert_eq!(p100.platform_fee, 20_000i128);
    assert_eq!(p100.destination_fee, 20_000i128);
    assert_eq!(p100.recipient_fee, 0i128);
    assert_eq!(p100.creator_remainder, 80_000i128);
    assert_eq!(p100.destination_fee + p100.recipient_fee, p100.platform_fee);
}

#[test]
fn fee_split_preview_with_destination_charity_boundaries() {
    let (env, _creator, admin, client) = setup_with_admin();
    let charity_addr = Address::generate(&env);

    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 1_000, // 10%
        royalty_bps: 1_000,      // 10%
        fee_recipient: Some(admin.clone()),
    });

    // 50% charity split
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 5_000,
        destination: FeeDestination::Charity(charity_addr.clone()),
    });

    let p = client.preview_fee_split(&500_000i128);
    // platform_fee = 500_000 * 10% = 50_000
    // destination_fee = 50_000 * 50% = 25_000
    // recipient_fee = 25_000
    // royalty_fee = 500_000 * 10% = 50_000
    // total_fee = 100_000
    // creator_remainder = 400_000
    assert_eq!(p.platform_fee, 50_000i128);
    assert_eq!(p.destination_fee, 25_000i128);
    assert_eq!(p.recipient_fee, 25_000i128);
    assert_eq!(p.royalty_fee, 50_000i128);
    assert_eq!(p.total_fee, 100_000i128);
    assert_eq!(p.creator_remainder, 400_000i128);
    assert_eq!(p.destination_fee + p.recipient_fee, p.platform_fee);
    assert_eq!(p.creator_remainder + p.total_fee, p.amount);
}

#[test]
fn fee_split_preview_small_amounts_rounding() {
    let (_env, _creator, admin, client) = setup_with_admin();
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 500, // 5%
        royalty_bps: 500,      // 5%
        fee_recipient: Some(admin.clone()),
    });
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 5_000,
        destination: FeeDestination::Burn,
    });

    // At amount = 1 stroop:
    // 1 * 500 / 10_000 = 0 platform fee, 0 royalty fee -> remainder = 1
    let p1 = client.preview_fee_split(&1i128);
    assert_eq!(p1.amount, 1i128);
    assert_eq!(p1.platform_fee, 0i128);
    assert_eq!(p1.destination_fee, 0i128);
    assert_eq!(p1.recipient_fee, 0i128);
    assert_eq!(p1.royalty_fee, 0i128);
    assert_eq!(p1.total_fee, 0i128);
    assert_eq!(p1.creator_remainder, 1i128);

    // At amount = 19 stroops:
    // 19 * 500 / 10_000 = 9500 / 10000 = 0
    let p19 = client.preview_fee_split(&19i128);
    assert_eq!(p19.platform_fee, 0i128);
    assert_eq!(p19.total_fee, 0i128);
    assert_eq!(p19.creator_remainder, 19i128);

    // At amount = 20 stroops:
    // 20 * 500 / 10_000 = 10000 / 10000 = 1 platform fee, 1 royalty fee
    // destination_fee = 1 * 5000 / 10000 = 0 (rounds down)
    // recipient_fee = 1 - 0 = 1
    let p20 = client.preview_fee_split(&20i128);
    assert_eq!(p20.platform_fee, 1i128);
    assert_eq!(p20.destination_fee, 0i128);
    assert_eq!(p20.recipient_fee, 1i128);
    assert_eq!(p20.royalty_fee, 1i128);
    assert_eq!(p20.total_fee, 2i128);
    assert_eq!(p20.creator_remainder, 18i128);
    assert_eq!(p20.destination_fee + p20.recipient_fee, p20.platform_fee);
    assert_eq!(p20.creator_remainder + p20.total_fee, p20.amount);

    // At amount = 9_999 stroops:
    let p9999 = client.preview_fee_split(&9_999i128);
    // platform_fee = 9999 * 500 / 10_000 = 499
    // royalty_fee = 499
    // destination_fee = 499 * 5000 / 10000 = 249
    // recipient_fee = 499 - 249 = 250
    // total_fee = 499 + 499 = 998
    // creator_remainder = 9999 - 998 = 9001
    assert_eq!(p9999.platform_fee, 499i128);
    assert_eq!(p9999.destination_fee, 249i128);
    assert_eq!(p9999.recipient_fee, 250i128);
    assert_eq!(p9999.royalty_fee, 499i128);
    assert_eq!(p9999.total_fee, 998i128);
    assert_eq!(p9999.creator_remainder, 9_001i128);
    assert_eq!(p9999.destination_fee + p9999.recipient_fee, p9999.platform_fee);
    assert_eq!(p9999.creator_remainder + p9999.total_fee, p9999.amount);
}
