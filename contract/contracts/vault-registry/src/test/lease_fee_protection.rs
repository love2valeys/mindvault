// ── Active lease pricing and access invariance under fee config changes (#37) ─
//
// Tests that:
// 1. Existing active leases retain their original price terms (`Lease::amount`) when
//    global fee configuration changes (set_fee_config, set_fee_recipient, set_fee_destination).
// 2. Existing active leases retain uninterrupted access (`lease_is_active == true`).
// 3. Global fee changes do not mutate, invalidate, or terminate existing active leases.
// 4. New leases created after fee changes adhere to the documented lease_price terms.
// 5. Active leases expire at their originally scheduled expiry_ledger regardless of fee changes.

#[test]
fn active_leases_retain_price_terms_and_access_when_fee_config_changes() {
    let (env, creator, settler, holder, client) = lease_setup();
    let admin = client.admin().unwrap();

    // 1. Initial registry fee config: 500 bps platform fee, no destination
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 500,
        royalty_bps: 200,
        fee_recipient: Some(admin.clone()),
    });

    let res_id = register_priced(&env, &creator, &client, "leasefee01", 1_000i128);
    env.ledger().set_sequence_number(1_000);

    let hour_price = client.lease_price(&res_id, &LeaseTier::Hour);
    assert_eq!(hour_price, 1_000i128 * LEASE_HOUR_MULTIPLIER);

    // Holder buys and settler settles a lease
    client.buy_lease(
        &holder,
        &res_id,
        &LeaseTier::Hour,
        &hour_price,
        &String::from_str(&env, "tx_lease_01"),
    );
    client.settle_lease(&settler, &res_id, &holder);

    let initial_lease = client.get_lease(&res_id, &holder);
    assert_eq!(initial_lease.amount, hour_price);
    assert_eq!(initial_lease.state, LeaseState::Active);
    assert_eq!(initial_lease.expiry_ledger, 1_000 + LEASE_HOUR_LEDGERS);
    assert!(client.lease_is_active(&res_id, &holder));

    // 2. Admin dramatically updates fee configuration:
    //    - platform fee raised to max (5,000 bps)
    //    - fee destination set to Burn at 5,000 bps (50% burn)
    //    - fee recipient changed
    let new_recipient = Address::generate(&env);
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: MAX_FEE_BPS,
        royalty_bps: 0,
        fee_recipient: Some(new_recipient.clone()),
    });
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 5_000,
        destination: FeeDestination::Burn,
    });

    // 3. Verify existing lease retained exact original price terms and active state
    let post_fee_lease = client.get_lease(&res_id, &holder);
    assert_eq!(post_fee_lease.amount, hour_price);
    assert_eq!(post_fee_lease.amount, initial_lease.amount);
    assert_eq!(post_fee_lease.tier, initial_lease.tier);
    assert_eq!(post_fee_lease.start_ledger, initial_lease.start_ledger);
    assert_eq!(post_fee_lease.expiry_ledger, initial_lease.expiry_ledger);
    assert_eq!(post_fee_lease.state, LeaseState::Active);

    // Verify uninterrupted access
    assert!(client.lease_is_active(&res_id, &holder));

    // 4. Verify a new lease created after fee changes behaves consistently
    let holder2 = Address::generate(&env);
    let day_price = client.lease_price(&res_id, &LeaseTier::Day);
    assert_eq!(day_price, 1_000i128 * LEASE_DAY_MULTIPLIER);

    client.buy_lease(
        &holder2,
        &res_id,
        &LeaseTier::Day,
        &day_price,
        &String::from_str(&env, "tx_lease_02"),
    );
    client.settle_lease(&settler, &res_id, &holder2);

    let lease2 = client.get_lease(&res_id, &holder2);
    assert_eq!(lease2.amount, day_price);
    assert_eq!(lease2.state, LeaseState::Active);
    assert!(client.lease_is_active(&res_id, &holder2));

    // Original lease is still active and unmodified
    let lease1_check = client.get_lease(&res_id, &holder);
    assert_eq!(lease1_check.amount, hour_price);
    assert!(client.lease_is_active(&res_id, &holder));

    // 5. Expiry boundary check: once expiry_ledger is reached, original lease expires as scheduled
    env.ledger().set_sequence_number(initial_lease.expiry_ledger);
    assert!(!client.lease_is_active(&res_id, &holder));
    // But holder2 (Day lease) is still active
    assert!(client.lease_is_active(&res_id, &holder2));
}

#[test]
fn active_lease_renewal_preserves_pricing_under_fee_changes() {
    let (env, creator, settler, holder, client) = lease_setup();
    let admin = client.admin().unwrap();

    let res_id = register_priced(&env, &creator, &client, "leasefee02", 500i128);
    env.ledger().set_sequence_number(500);

    let hour_price = client.lease_price(&res_id, &LeaseTier::Hour);
    client.record_lease(
        &settler,
        &holder,
        &res_id,
        &LeaseTier::Hour,
        &hour_price,
        &String::from_str(&env, "tx_rec_01"),
    );
    assert!(client.lease_is_active(&res_id, &holder));

    // Fee settings updated
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 2_500,
        royalty_bps: 1_000,
        fee_recipient: Some(admin),
    });

    // Renew active lease before expiry
    env.ledger().set_sequence_number(600);
    client.renew_lease(
        &holder,
        &res_id,
        &LeaseTier::Hour,
        &hour_price,
        &String::from_str(&env, "tx_rnw_01"),
    );

    let renewed = client.get_lease(&res_id, &holder);
    assert_eq!(renewed.amount, hour_price);
    assert_eq!(renewed.state, LeaseState::Active);
    // Expiry extended by LEASE_HOUR_LEDGERS
    assert_eq!(renewed.expiry_ledger, 500 + LEASE_HOUR_LEDGERS + LEASE_HOUR_LEDGERS);
    assert!(client.lease_is_active(&res_id, &holder));
}
