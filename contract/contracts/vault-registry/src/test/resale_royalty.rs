// Focused tests for creator opt-in resale royalty (fee on transfer, #233).

#[test]
fn resale_royalty_defaults_to_unset_and_preserves_legacy_transfers() {
    let (env, alice, client) = setup();
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);
    let id = String::from_str(&env, "reslegacy");

    client.register(
        &alice,
        &id,
        &10_000i128,
        &String::from_str(&env, "ipfs://legacy"),
        &empty_tags(&env),
    );

    assert_eq!(client.get_resale_royalty(&id), None);
    let preview = client.preview_resale_royalty(&id, &None);
    assert_eq!(preview.basis_amount, 10_000);
    assert_eq!(preview.royalty_bps, 0);
    assert_eq!(preview.royalty_amount, 0);
    assert_eq!(preview.seller_remainder, 10_000);
    assert_eq!(preview.recipient, None);

    let summary = client.get_resale_royalty_summary(&id);
    assert_eq!(summary.total_captured, 0);
    assert_eq!(summary.resale_count, 0);
    assert_eq!(summary.last_royalty_amount, 0);

    // Primary and secondary transfers without opt-in emit no `resaleroy` event.
    client.transfer_ownership(&id, &bob);
    assert!(find_event::<ResaleRoyaltyCaptured>(&env, &client.address, "resaleroy").is_none());

    client.transfer_ownership(&id, &charlie);
    assert!(find_event::<ResaleRoyaltyCaptured>(&env, &client.address, "resaleroy").is_none());
    assert_eq!(client.get_resale_royalty_summary(&id).resale_count, 0);
}

#[test]
fn resale_royalty_opt_in_cap_update_clear_and_event() {
    let (env, alice, client) = setup();
    let treasury = Address::generate(&env);
    let id = register_default(&env, &alice, &client, "resoptin");

    // Exceeding MAX_RESALE_ROYALTY_BPS fails with FeeBpsTooHigh.
    assert_eq!(
        client.try_set_resale_royalty(&id, &(MAX_RESALE_ROYALTY_BPS + 1), &None),
        Err(Ok(Error::FeeBpsTooHigh))
    );
    assert_eq!(client.get_resale_royalty(&id), None);

    // Opt in at 500 bps (5 %) with default recipient = original creator.
    client.set_resale_royalty(&id, &500u32, &None);
    let cfg = client
        .get_resale_royalty(&id)
        .expect("resale royalty should be configured");
    assert_eq!(cfg.original_creator, alice);
    assert_eq!(cfg.royalty_bps, 500);
    assert_eq!(cfg.recipient, alice);

    let updated = find_event::<ResaleRoyaltyUpdated>(&env, &client.address, "setresroy")
        .expect("setresroy event should be emitted");
    assert_eq!(updated.id, id);
    assert_eq!(updated.original_creator, alice);
    assert_eq!(updated.old_bps, 0);
    assert_eq!(updated.new_bps, 500);
    assert_eq!(updated.recipient, Some(alice.clone()));

    // Update to exact cap (1,000 bps = 10 %) with explicit recipient.
    client.set_resale_royalty(&id, &MAX_RESALE_ROYALTY_BPS, &Some(treasury.clone()));
    let cfg2 = client.get_resale_royalty(&id).unwrap();
    assert_eq!(cfg2.royalty_bps, MAX_RESALE_ROYALTY_BPS);
    assert_eq!(cfg2.recipient, treasury);

    let updated2 = find_event::<ResaleRoyaltyUpdated>(&env, &client.address, "setresroy")
        .expect("setresroy event should be emitted on update");
    assert_eq!(updated2.old_bps, 500);
    assert_eq!(updated2.new_bps, MAX_RESALE_ROYALTY_BPS);
    assert_eq!(updated2.recipient, Some(treasury));

    // Setting 0 clears opt-in.
    client.set_resale_royalty(&id, &0u32, &None);
    assert_eq!(client.get_resale_royalty(&id), None);
    let cleared = find_event::<ResaleRoyaltyUpdated>(&env, &client.address, "setresroy")
        .expect("setresroy event should be emitted on clear");
    assert_eq!(cleared.old_bps, MAX_RESALE_ROYALTY_BPS);
    assert_eq!(cleared.new_bps, 0);
    assert_eq!(cleared.recipient, None);
}

#[test]
fn resale_royalty_captures_on_secondary_transfer_ownership_and_accumulates() {
    let (env, alice, client) = setup();
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);
    let dave = Address::generate(&env);
    let id = String::from_str(&env, "rescapture");

    client.register(
        &alice,
        &id,
        &1_000_000i128,
        &String::from_str(&env, "ipfs://resale"),
        &empty_tags(&env),
    );
    // 5 % resale royalty to original creator Alice.
    client.set_resale_royalty(&id, &500u32, &None);

    // Primary transfer (Alice -> Bob): creator selling own asset -> no self-royalty.
    client.transfer_ownership(&id, &bob);
    assert!(
        find_event::<ResaleRoyaltyCaptured>(&env, &client.address, "resaleroy").is_none(),
        "primary transfer from original creator must not charge self-royalty"
    );
    assert_eq!(client.get_resale_royalty_summary(&id).resale_count, 0);

    // Secondary transfer (Bob -> Charlie): captures 5% of 1_000_000 = 50_000.
    env.ledger().set_sequence_number(42);
    client.transfer_ownership(&id, &charlie);

    let evt = find_event::<ResaleRoyaltyCaptured>(&env, &client.address, "resaleroy")
        .expect("secondary transfer must emit resaleroy event");
    assert_eq!(evt.id, id);
    assert_eq!(evt.from, bob);
    assert_eq!(evt.to, charlie);
    assert_eq!(evt.recipient, alice);
    assert_eq!(evt.basis_amount, 1_000_000);
    assert_eq!(evt.royalty_bps, 500);
    assert_eq!(evt.royalty_amount, 50_000);
    assert_eq!(evt.ledger, 42);

    let sum1 = client.get_resale_royalty_summary(&id);
    assert_eq!(sum1.total_captured, 50_000);
    assert_eq!(sum1.resale_count, 1);
    assert_eq!(sum1.last_royalty_amount, 50_000);
    assert_eq!(sum1.last_basis_amount, 1_000_000);
    assert_eq!(sum1.last_ledger, 42);

    // Charlie updates price to 2_500_000 and resells to Dave: captures 125_000.
    client.set_price(&id, &2_500_000i128);
    env.ledger().set_sequence_number(99);
    client.transfer_ownership(&id, &dave);

    let sum2 = client.get_resale_royalty_summary(&id);
    assert_eq!(sum2.total_captured, 175_000);
    assert_eq!(sum2.resale_count, 2);
    assert_eq!(sum2.last_royalty_amount, 125_000);
    assert_eq!(sum2.last_basis_amount, 2_500_000);
    assert_eq!(sum2.last_ledger, 99);

    // Returning ownership to original creator Alice does not self-charge.
    client.transfer_ownership(&id, &alice);
    assert!(find_event::<ResaleRoyaltyCaptured>(&env, &client.address, "resaleroy").is_none());
    assert_eq!(client.get_resale_royalty_summary(&id).resale_count, 2);
}

#[test]
fn resale_royalty_captures_on_accept_transfer_and_transfer_with_terms() {
    let (env, alice, client) = setup();
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);
    let dave = Address::generate(&env);
    let treasury = Address::generate(&env);
    let id = String::from_str(&env, "resaccept");

    client.register(
        &alice,
        &id,
        &400_000i128,
        &String::from_str(&env, "ipfs://resaccept"),
        &empty_tags(&env),
    );
    // 2.5 % (250 bps) to custom treasury.
    client.set_resale_royalty(&id, &250u32, &Some(treasury.clone()));

    // Primary two-step transfer (Alice -> Bob): no resale fee.
    client.propose_transfer(&id, &bob);
    client.accept_transfer(&id);
    assert_eq!(client.get_resale_royalty_summary(&id).resale_count, 0);

    // Secondary two-step transfer (Bob -> Charlie): captures 2.5 % of 400_000 = 10_000.
    client.propose_transfer(&id, &charlie);
    client.accept_transfer(&id);

    let evt = find_event::<ResaleRoyaltyCaptured>(&env, &client.address, "resaleroy")
        .expect("accept_transfer on resale must emit resaleroy");
    assert_eq!(evt.from, bob);
    assert_eq!(evt.to, charlie);
    assert_eq!(evt.recipient, treasury);
    assert_eq!(evt.royalty_amount, 10_000);

    // Secondary transfer via transfer_ownership_with_terms (Charlie -> Dave): +10_000.
    client.transfer_ownership_with_terms(
        &id,
        &dave,
        &Some(String::from_str(&env, "termsv2")),
        &false,
    );
    let sum = client.get_resale_royalty_summary(&id);
    assert_eq!(sum.resale_count, 2);
    assert_eq!(sum.total_captured, 20_000);
}

#[test]
fn secondary_owner_cannot_modify_or_bypass_resale_royalty() {
    let (env, alice, client) = setup();
    let bob = Address::generate(&env);
    let charlie = Address::generate(&env);
    let id = String::from_str(&env, "reslock");

    client.register(
        &alice,
        &id,
        &200_000i128,
        &String::from_str(&env, "ipfs://reslock"),
        &empty_tags(&env),
    );
    client.set_resale_royalty(&id, &1_000u32, &None);
    client.transfer_ownership(&id, &bob);

    // Bob (secondary owner) cannot clear or alter Alice's resale royalty.
    assert_eq!(
        client.try_set_resale_royalty(&id, &0u32, &None),
        Err(Ok(Error::Unauthorized))
    );
    assert_eq!(
        client.try_set_resale_royalty(&id, &100u32, &Some(bob.clone())),
        Err(Ok(Error::Unauthorized))
    );

    // Bob opting out of primary sale royalties does NOT disable Alice's resale royalty.
    client.set_royalties_disabled(&id, &true);
    client.transfer_ownership(&id, &charlie);
    let sum = client.get_resale_royalty_summary(&id);
    assert_eq!(sum.resale_count, 1);
    assert_eq!(sum.total_captured, 20_000);

    // Even on a resource where Alice never opted in, Bob cannot retroactively
    // pose as original creator after taking ownership.
    let id2 = register_default(&env, &alice, &client, "reslock2");
    client.transfer_ownership(&id2, &bob);
    assert_eq!(
        client.try_set_resale_royalty(&id2, &500u32, &None),
        Err(Ok(Error::Unauthorized))
    );
}

#[test]
fn resale_royalty_enforces_auth_pause_mutability_and_preview_bounds() {
    let (env, alice, admin, client) = setup_with_admin();
    let id = String::from_str(&env, "resguards");

    client.register(
        &alice,
        &id,
        &333i128,
        &String::from_str(&env, "ipfs://guards"),
        &empty_tags(&env),
    );

    // Missing auth fails.
    env.mock_auths(&[]);
    assert!(client.try_set_resale_royalty(&id, &500u32, &None).is_err());
    env.mock_all_auths();

    // Configure 500 bps (5 %). Floor rounding on 333 -> 333 * 500 / 10_000 = 16.
    client.set_resale_royalty(&id, &500u32, &None);
    let default_preview = client.preview_resale_royalty(&id, &None);
    assert_eq!(default_preview.basis_amount, 333);
    assert_eq!(default_preview.royalty_amount, 16);
    assert_eq!(default_preview.seller_remainder, 317);
    assert_eq!(default_preview.recipient, Some(alice.clone()));

    // Custom sale amount preview (including MAX_PRICE without overflow).
    let max_preview = client.preview_resale_royalty(&id, &Some(MAX_PRICE));
    assert_eq!(max_preview.royalty_amount, MAX_PRICE / 20);
    assert_eq!(
        max_preview.seller_remainder,
        MAX_PRICE - max_preview.royalty_amount
    );

    // Invalid custom amounts rejected.
    assert_eq!(
        client.try_preview_resale_royalty(&id, &Some(0i128)),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_preview_resale_royalty(&id, &Some(-10i128)),
        Err(Ok(Error::InvalidPrice))
    );
    assert_eq!(
        client.try_preview_resale_royalty(&id, &Some(MAX_PRICE + 1)),
        Err(Ok(Error::PriceExceedsMax))
    );

    // Non-mutable resource rejects set_resale_royalty.
    client.freeze_resource(&id);
    assert_eq!(
        client.try_set_resale_royalty(&id, &250u32, &None),
        Err(Ok(Error::ResourceNotMutable))
    );
    client.reactivate_resource(&id);

    // Paused contract rejects set_resale_royalty but allows all reads.
    client.set_paused(&admin, &true);
    assert_eq!(
        client.try_set_resale_royalty(&id, &250u32, &None),
        Err(Ok(Error::ContractPaused))
    );
    assert!(client.get_resale_royalty(&id).is_some());
    assert_eq!(client.preview_resale_royalty(&id, &None).royalty_bps, 500);
    assert_eq!(client.get_resale_royalty_summary(&id).resale_count, 0);
}
