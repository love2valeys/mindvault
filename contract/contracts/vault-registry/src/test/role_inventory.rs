// `list_role_members` (#28): deterministic, capped, read-only role pages.

#[test]
fn role_inventory_admin_page_lists_only_the_admin() {
    let (env, _creator, client) = setup();
    assert_eq!(client.list_role_members(&RoleKind::Admin, &0, &10).members.len(), 0);
    let admin = Address::generate(&env);
    client.nominate_new_admin(&admin);
    let page = client.list_role_members(&RoleKind::Admin, &0, &10);
    assert_eq!(page.members, Vec::from_array(&env, [admin]));
    assert_eq!(page.next_cursor, None);
}

#[test]
fn role_inventory_tracks_grants_and_revocations_per_role() {
    let (env, _creator, client) = setup();
    client.nominate_new_admin(&Address::generate(&env));
    let (v1, v2) = (Address::generate(&env), Address::generate(&env));
    let (m1, s1) = (Address::generate(&env), Address::generate(&env));
    client.add_verifier(&v1);
    client.add_verifier(&v2);
    client.add_verifier(&v1); // idempotent
    client.add_moderator(&m1);
    client.add_settler(&s1);

    let verifiers = client.list_role_members(&RoleKind::Verifier, &0, &10);
    assert_eq!(verifiers.members, Vec::from_array(&env, [v1.clone(), v2.clone()]));
    assert_eq!(
        client.list_role_members(&RoleKind::Moderator, &0, &10).members,
        Vec::from_array(&env, [m1.clone()])
    );
    assert_eq!(
        client.list_role_members(&RoleKind::Settler, &0, &10).members,
        Vec::from_array(&env, [s1.clone()])
    );

    client.remove_verifier(&v1);
    client.remove_moderator(&m1);
    client.remove_settler(&s1);
    assert_eq!(
        client.list_role_members(&RoleKind::Verifier, &0, &10).members,
        Vec::from_array(&env, [v2.clone()])
    );
    assert_eq!(client.list_role_members(&RoleKind::Moderator, &0, &10).members.len(), 0);
    assert_eq!(client.list_role_members(&RoleKind::Settler, &0, &10).members.len(), 0);

    let v3 = Address::generate(&env);
    client.rotate_verifier(&v2, &v3);
    assert_eq!(
        client.list_role_members(&RoleKind::Verifier, &0, &10).members,
        Vec::from_array(&env, [v3])
    );
}

#[test]
fn role_index_and_fee_mutations_bump_instance_ttl_near_threshold() {
    let (env, _creator, _admin, client) = setup_with_admin();
    let contract = client.address.clone();
    let verifier = Address::generate(&env);

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    env.as_contract(&contract, || {
        role_inventory::track_grant(&env, RoleKind::Verifier, &verifier)
    })
    .unwrap();
    assert_eq!(
        env.as_contract(&contract, || env.storage().instance().get_ttl()),
        TTL_BUMP_AMOUNT
    );
    assert_eq!(
        client.list_role_members(&RoleKind::Verifier, &0, &10).members,
        Vec::from_array(&env, [verifier.clone()])
    );

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    env.as_contract(&contract, || {
        role_inventory::track_revoke(&env, RoleKind::Verifier, &verifier)
    });
    assert_eq!(
        env.as_contract(&contract, || env.storage().instance().get_ttl()),
        TTL_BUMP_AMOUNT
    );

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    client.set_fee_config(&FeeConfig {
        platform_fee_bps: 0,
        royalty_bps: 0,
        fee_recipient: None,
    });
    assert_eq!(
        env.as_contract(&contract, || env.storage().instance().get_ttl()),
        TTL_BUMP_AMOUNT
    );

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    client.set_fee_destination(&FeeDestinationConfig {
        bps: 0,
        destination: FeeDestination::None,
    });
    assert_eq!(
        env.as_contract(&contract, || env.storage().instance().get_ttl()),
        TTL_BUMP_AMOUNT
    );
}

#[test]
fn role_inventory_pages_are_ordered_and_capped() {
    let (env, _creator, client) = setup();
    client.nominate_new_admin(&Address::generate(&env));
    let mut granted: std::vec::Vec<Address> = std::vec::Vec::new();
    for _ in 0..25 {
        let who = Address::generate(&env);
        client.add_settler(&who);
        granted.push(who);
    }

    // A huge limit is clamped to ROLE_PAGE_CAP.
    let first = client.list_role_members(&RoleKind::Settler, &0, &1_000);
    assert_eq!(first.members.len(), ROLE_PAGE_CAP);
    assert_eq!(first.next_cursor, Some(ROLE_PAGE_CAP));
    assert_eq!(first.members.get(0).unwrap(), granted[0]);

    let second = client.list_role_members(&RoleKind::Settler, &ROLE_PAGE_CAP, &1_000);
    assert_eq!(second.members.len(), 5);
    assert_eq!(second.members.get(0).unwrap(), granted[20]);
    assert_eq!(second.next_cursor, None);

    let past_end = client.list_role_members(&RoleKind::Settler, &99, &10);
    assert_eq!(past_end.members.len(), 0);
    assert_eq!(past_end.next_cursor, None);
    assert_eq!(client.list_role_members(&RoleKind::Settler, &0, &0).members.len(), 0);
}

#[test]
fn role_inventory_enforces_member_limit() {
    let (env, _creator, client) = setup();
    client.nominate_new_admin(&Address::generate(&env));
    for _ in 0..MAX_ROLE_MEMBERS {
        client.add_moderator(&Address::generate(&env));
    }
    let extra = Address::generate(&env);
    assert_eq!(client.try_add_moderator(&extra), Err(Ok(Error::BatchTooLarge)));
    assert!(!client.is_moderator(&extra));
}
