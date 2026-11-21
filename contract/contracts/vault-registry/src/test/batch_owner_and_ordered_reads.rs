// Focused tests for batch ownership reads (`get_owner_many`, #232),
// typed ordered batch resource reads (`get_many_ordered`, #236), and
// bare schema-version reads (`resource_schema_version`, #239).

// ── #232: get_owner_many ─────────────────────────────────────────────────────

#[test]
fn get_owner_many_empty_batch_returns_empty() {
    let (env, _creator, client) = setup();
    let owners = client.get_owner_many(&Vec::new(&env));
    assert_eq!(owners.len(), 0);
}

#[test]
fn get_owner_many_accepts_exact_max_batch_and_preserves_duplicates() {
    let (env, creator, client) = setup();
    let existing = register_default(&env, &creator, &client, "ownermax0");

    let mut ids: Vec<String> = Vec::new(&env);
    for i in 0..MAX_BATCH_READ {
        if i % 2 == 0 {
            ids.push_back(existing.clone());
        } else {
            ids.push_back(String::from_str(&env, &format!("absent{i}")));
        }
    }

    let owners = client.get_owner_many(&ids);
    assert_eq!(owners.len(), MAX_BATCH_READ);
    for i in 0..MAX_BATCH_READ {
        if i % 2 == 0 {
            assert_eq!(owners.get(i).unwrap(), Some(creator.clone()));
        } else {
            assert_eq!(owners.get(i).unwrap(), None);
        }
    }
}

#[test]
fn get_owner_many_rejects_invalid_resource_id() {
    let (env, creator, client) = setup();
    let valid = register_default(&env, &creator, &client, "ownervalid");
    let invalid = String::from_str(&env, "Invalid-Id");

    let ids = Vec::from_array(&env, [valid, invalid]);
    assert_eq!(
        client.try_get_owner_many(&ids),
        Err(Ok(Error::InvalidResourceId))
    );
}

#[test]
fn get_owner_many_tracks_two_step_transfer_and_tombstone_and_pause() {
    let (env, creator, admin, client) = setup_with_admin();
    let id_a = register_default(&env, &creator, &client, "ownera");
    let id_b = register_default(&env, &creator, &client, "ownerb");
    let new_owner = Address::generate(&env);

    client.propose_transfer(&id_a, &new_owner);
    client.accept_transfer(&id_a);
    client.tombstone_resource(&id_b, &admin);
    client.set_paused(&admin, &true);

    let ids = Vec::from_array(&env, [id_b, id_a]);
    let owners = client.get_owner_many(&ids);
    assert_eq!(owners.len(), 2);
    assert_eq!(owners.get(0).unwrap(), Some(creator));
    assert_eq!(owners.get(1).unwrap(), Some(new_owner));
}

#[test]
fn get_owner_many_bumps_persistent_ttl_for_found_entries() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "ownerttl");

    env.ledger().set_sequence_number(
        env.ledger().sequence() + TTL_DAY_IN_LEDGERS * 2,
    );
    let ttl_before = resource_storage_ttl(&env, &client.address, &id);

    let owners = client.get_owner_many(&Vec::from_array(&env, [id.clone()]));
    assert_eq!(owners.get(0).unwrap(), Some(creator));

    let ttl_after = resource_storage_ttl(&env, &client.address, &id);
    assert!(
        ttl_after > ttl_before,
        "get_owner_many should bump TTL for found resources"
    );
}

// ── #236: get_many_ordered ───────────────────────────────────────────────────

#[test]
fn get_many_ordered_preserves_input_order_duplicates_and_missing_slots() {
    let (env, creator, client) = setup();
    let id_a = String::from_str(&env, "ordbatcha");
    let id_b = String::from_str(&env, "ordbatchb");
    let missing = String::from_str(&env, "ordmissing");

    client.register(
        &creator,
        &id_a,
        &150i128,
        &String::from_str(&env, "ipfs://ord-a"),
        &empty_tags(&env),
    );
    client.register(
        &creator,
        &id_b,
        &250i128,
        &String::from_str(&env, "ipfs://ord-b"),
        &empty_tags(&env),
    );

    let ids = Vec::from_array(
        &env,
        [id_b.clone(), missing.clone(), id_a.clone(), id_b.clone()],
    );

    let ordered = client.get_many_ordered(&ids);
    let legacy = client.get_many(&ids);
    assert_eq!(ordered.len(), 4);
    assert_eq!(legacy.len(), 4);

    let item0 = ordered.get(0).unwrap();
    assert_eq!(item0.id, id_b);
    assert_eq!(item0.resource.as_ref().unwrap().price, 250);
    assert_eq!(item0.resource, legacy.get(0).unwrap());

    let item1 = ordered.get(1).unwrap();
    assert_eq!(item1.id, missing);
    assert!(item1.resource.is_none());
    assert!(legacy.get(1).unwrap().is_none());

    let item2 = ordered.get(2).unwrap();
    assert_eq!(item2.id, id_a);
    assert_eq!(item2.resource.as_ref().unwrap().price, 150);
    assert_eq!(item2.resource, legacy.get(2).unwrap());

    let item3 = ordered.get(3).unwrap();
    assert_eq!(item3.id, id_b);
    assert_eq!(item3.resource, item0.resource);
}

#[test]
fn get_many_ordered_empty_and_boundary_and_oversized_batches() {
    let (env, creator, client) = setup();
    assert_eq!(client.get_many_ordered(&Vec::new(&env)).len(), 0);

    let existing = register_default(&env, &creator, &client, "ordbound0");
    let mut exact_ids: Vec<String> = Vec::new(&env);
    for i in 0..MAX_BATCH_READ {
        if i == 0 {
            exact_ids.push_back(existing.clone());
        } else {
            exact_ids.push_back(String::from_str(&env, &format!("ordmiss{i}")));
        }
    }
    let exact = client.get_many_ordered(&exact_ids);
    assert_eq!(exact.len(), MAX_BATCH_READ);
    assert_eq!(exact.get(0).unwrap().id, existing);
    assert!(exact.get(0).unwrap().resource.is_some());
    assert!(exact.get(MAX_BATCH_READ - 1).unwrap().resource.is_none());

    exact_ids.push_back(String::from_str(&env, "ordoverflow"));
    assert_eq!(
        client.try_get_many_ordered(&exact_ids),
        Err(Ok(Error::BatchTooLarge))
    );
}

#[test]
fn get_many_ordered_rejects_invalid_id_and_works_while_paused_with_tombstones() {
    let (env, creator, admin, client) = setup_with_admin();
    let live = register_default(&env, &creator, &client, "ordlive");
    let tomb = register_default(&env, &creator, &client, "ordtomb");
    client.tombstone_resource(&tomb, &admin);
    client.set_paused(&admin, &true);

    let invalid_ids = Vec::from_array(&env, [live.clone(), String::from_str(&env, "BAD_ID")]);
    assert_eq!(
        client.try_get_many_ordered(&invalid_ids),
        Err(Ok(Error::InvalidResourceId))
    );

    let items = client.get_many_ordered(&Vec::from_array(&env, [tomb.clone(), live.clone()]));
    assert_eq!(items.len(), 2);
    assert_eq!(items.get(0).unwrap().id, tomb);
    assert_eq!(
        items.get(0).unwrap().resource.unwrap().state,
        ResourceState::Tombstoned
    );
    assert_eq!(items.get(1).unwrap().id, live);
    assert_eq!(
        items.get(1).unwrap().resource.unwrap().state,
        ResourceState::Listed
    );
}

// ── #239: resource_schema_version ────────────────────────────────────────────

#[test]
fn resource_schema_version_matches_constant_and_version_structs() {
    let (env, creator, client) = setup();
    let schema_ver = client.resource_schema_version();

    assert!(schema_ver > 0, "resource_schema_version must be positive");
    assert_eq!(schema_ver, RESOURCE_SCHEMA_VERSION);
    assert_eq!(schema_ver, client.contract_version().resource_schema_version);
    assert_eq!(schema_ver, client.registry_info().resource_schema_version);

    let id = register_default(&env, &creator, &client, "schemaver0");
    assert_eq!(client.get(&id).schema_version, schema_ver);
}

#[test]
fn resource_schema_version_stays_stable_across_mutations_and_pause() {
    let (env, creator, admin, client) = setup_with_admin();
    let before = client.resource_schema_version();

    let id = register_default(&env, &creator, &client, "schemaver1");
    client.set_price(&id, &500i128);
    client.set_paused(&admin, &true);

    assert_eq!(client.resource_schema_version(), before);
}
