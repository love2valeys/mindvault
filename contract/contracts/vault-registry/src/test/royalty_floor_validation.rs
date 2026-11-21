// ── Explicit reserve-price / minimum-royalty floor validation (#247) ─────────
//
// `validate_price` enforces that every resource price in USDC base units
// strictly exceeds the active combined fee take (`platform_fee_bps +
// royalty_bps`), which also guarantees `royalty_bps < price`. Tiny prices
// cannot produce zero-remainder or negative splits.

fn setup_with_fee_rates<'a>(
    platform_fee_bps: u32,
    royalty_bps: u32,
) -> (Env, Address, Address, VaultRegistryClient<'a>) {
    let (env, creator, client) = setup();
    let admin = Address::generate(&env);
    client.nominate_new_admin(&admin);
    client.set_fee_config(&FeeConfig {
        platform_fee_bps,
        royalty_bps,
        fee_recipient: Some(admin.clone()),
    });
    (env, creator, admin, client)
}

#[test]
fn min_price_floor_matches_active_fee_and_royalty_rates() {
    assert_eq!(VaultRegistry::min_price_for_fee_config(None), 1);

    let zero = FeeConfig {
        platform_fee_bps: 0,
        royalty_bps: 0,
        fee_recipient: None,
    };
    assert_eq!(VaultRegistry::min_price_for_fee_config(Some(&zero)), 1);

    let royalty_only = FeeConfig {
        platform_fee_bps: 0,
        royalty_bps: 500,
        fee_recipient: None,
    };
    assert_eq!(
        VaultRegistry::min_price_for_fee_config(Some(&royalty_only)),
        501
    );

    let combined = FeeConfig {
        platform_fee_bps: 1_500,
        royalty_bps: 1_000,
        fee_recipient: None,
    };
    assert_eq!(
        VaultRegistry::min_price_for_fee_config(Some(&combined)),
        2_501
    );

    let at_cap = FeeConfig {
        platform_fee_bps: 2_500,
        royalty_bps: 2_500,
        fee_recipient: None,
    };
    assert_eq!(
        VaultRegistry::min_price_for_fee_config(Some(&at_cap)),
        i128::from(MAX_FEE_BPS) + 1
    );
}

#[test]
fn register_variants_enforce_combined_fee_and_royalty_floor() {
    let (env, creator, _admin, client) = setup_with_fee_rates(1_500, 1_000);
    let metadata = String::from_str(&env, "ipfs://floor");
    let content_hash = Some(String::from_str(&env, "sha256:floorhash"));
    let memo_hash = Some(BytesN::from_array(&env, &[9u8; 32]));

    // Combined fee rate is 2_500 bps (1_500 platform + 1_000 royalty):
    // prices at or below 2_500 must be rejected across every register entry point.
    for price in [1i128, 1_000, 1_500, 2_499, 2_500] {
        assert_eq!(
            client.try_register(
                &creator,
                &String::from_str(&env, "flrreg"),
                &price,
                &metadata,
                &empty_tags(&env),
            ),
            Err(Ok(Error::InvalidPrice))
        );
        assert_eq!(
            client.try_register_with_hash(
                &creator,
                &String::from_str(&env, "flrhash"),
                &price,
                &metadata,
                &empty_tags(&env),
                &content_hash,
            ),
            Err(Ok(Error::InvalidPrice))
        );
        assert_eq!(
            client.try_register_with_memo(
                &creator,
                &String::from_str(&env, "flrmemo"),
                &price,
                &metadata,
                &empty_tags(&env),
                &content_hash,
                &memo_hash,
            ),
            Err(Ok(Error::InvalidPrice))
        );
    }
    assert_eq!(client.count(), 0);

    // Exact floor (2_501) is accepted across all registration variants and yields
    // non-negative fee splits with a strictly positive creator remainder.
    let floor = 2_501i128;
    let id_plain = String::from_str(&env, "flrok1");
    let id_hash = String::from_str(&env, "flrok2");
    let id_memo = String::from_str(&env, "flrok3");

    client.register(&creator, &id_plain, &floor, &metadata, &empty_tags(&env));
    client.register_with_hash(
        &creator,
        &id_hash,
        &floor,
        &metadata,
        &empty_tags(&env),
        &content_hash,
    );
    client.register_with_memo(
        &creator,
        &id_memo,
        &floor,
        &metadata,
        &empty_tags(&env),
        &content_hash,
        &memo_hash,
    );

    assert_eq!(client.get(&id_plain).price, floor);
    assert_eq!(client.get(&id_hash).price, floor);
    assert_eq!(client.get(&id_memo).price, floor);

    let preview = client.preview_fee_split(&floor);
    assert_eq!(preview.platform_fee, 375);
    assert_eq!(preview.royalty_fee, 250);
    assert_eq!(preview.total_fee, 625);
    assert_eq!(preview.creator_remainder, 1_876);
    assert!(preview.creator_remainder > 0);
    assert_eq!(preview.creator_remainder + preview.total_fee, floor);
}

#[test]
fn register_batch_rejects_prices_at_or_below_reserve_floor() {
    let (env, creator, _admin, client) = setup_with_fee_rates(800, 400);
    let metadata = String::from_str(&env, "ipfs://batchfloor");

    // Combined floor is 1_201: index 0 (400) and index 1 (1_200) fail with
    // InvalidPrice; index 2 (1_201) succeeds.
    let items = soroban_sdk::vec![
        &env,
        BatchRegisterItem {
            id: String::from_str(&env, "bflr0"),
            price: 400,
            metadata: metadata.clone(),
            tags: empty_tags(&env),
            content_hash: None,
        },
        BatchRegisterItem {
            id: String::from_str(&env, "bflr1"),
            price: 1_200,
            metadata: metadata.clone(),
            tags: empty_tags(&env),
            content_hash: None,
        },
        BatchRegisterItem {
            id: String::from_str(&env, "bflr2"),
            price: 1_201,
            metadata,
            tags: empty_tags(&env),
            content_hash: None,
        },
    ];

    let result = client.register_batch(&creator, &items);
    assert_eq!(result.succeeded.len(), 1);
    assert_eq!(
        result.succeeded.get(0).unwrap(),
        String::from_str(&env, "bflr2")
    );
    assert_eq!(result.failed.len(), 2);
    assert_eq!(result.failed.get(0).unwrap(), (0, Error::InvalidPrice as u32));
    assert_eq!(result.failed.get(1).unwrap(), (1, Error::InvalidPrice as u32));
    assert!(!client.exists(&String::from_str(&env, "bflr0")));
    assert!(!client.exists(&String::from_str(&env, "bflr1")));
    assert_eq!(client.get(&String::from_str(&env, "bflr2")).price, 1_201);
}

#[test]
fn set_price_and_batch_update_enforce_combined_fee_floor() {
    let (env, creator, _admin, client) = setup_with_fee_rates(2_000, 1_000);
    let id_a = register_priced(&env, &creator, &client, "spflra", 10_000);
    let id_b = register_priced(&env, &creator, &client, "spflrb", 20_000);

    // Combined rate is 3_000 bps -> prices <= 3_000 are rejected, even when
    // strictly above royalty_bps (1_000) alone.
    assert_eq!(
        client.try_set_price(&id_a, &1_500i128),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_set_price(&id_a, &3_000i128),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(client.get(&id_a).price, 10_000);

    // Atomic batch price update rejects the whole batch if any item is at or
    // below the combined floor.
    let bad_updates = soroban_sdk::vec![
        &env,
        BatchPriceUpdate {
            id: id_a.clone(),
            new_price: 5_000,
        },
        BatchPriceUpdate {
            id: id_b.clone(),
            new_price: 3_000,
        },
    ];
    assert_eq!(
        client.try_set_price_many(&creator, &bad_updates),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(client.get(&id_a).price, 10_000);
    assert_eq!(client.get(&id_b).price, 20_000);

    // Updating both to >= 3_001 succeeds.
    let good_updates = soroban_sdk::vec![
        &env,
        BatchPriceUpdate {
            id: id_a.clone(),
            new_price: 3_001,
        },
        BatchPriceUpdate {
            id: id_b.clone(),
            new_price: 4_000,
        },
    ];
    client.set_price_many(&creator, &good_updates);
    assert_eq!(client.get(&id_a).price, 3_001);
    assert_eq!(client.get(&id_b).price, 4_000);
}

#[test]
fn fee_config_updates_dynamically_adjust_price_floor() {
    let (env, creator, admin, client) = setup_with_fee_rates(500, 500);
    let existing = register_priced(&env, &creator, &client, "dynflr0", 1_001);

    // Raising the combined fee config to 4_000 bps raises the floor to 4_001
    // for future registrations and reprices, while leaving existing resources intact.
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 2_500,
        royalty_bps: 1_500,
        fee_recipient: Some(admin.clone()),
    });
    assert_eq!(client.get(&existing).price, 1_001);
    assert_eq!(
        client.try_register(
            &creator,
            &String::from_str(&env, "dynflr1"),
            &4_000i128,
            &String::from_str(&env, "ipfs://dyn"),
            &empty_tags(&env),
        ),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_set_price(&existing, &4_000i128),
        Err(Ok(Error::InvalidPrice))
    );
    client.set_price(&existing, &4_001i128);
    assert_eq!(client.get(&existing).price, 4_001);

    // Lowering the fee config back to 0 restores the 1 base-unit floor.
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 0,
        royalty_bps: 0,
        fee_recipient: Some(admin),
    });
    client.set_price(&existing, &1i128);
    assert_eq!(client.get(&existing).price, 1);
}
