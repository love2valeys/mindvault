// ─── Listed index cleanup on tombstone transitions (#292) ────────────────────
//
// Tombstoning permanently retires a resource. Every derived discovery/listed
// index (`ListedCount`, `CreatorListedCount`, `CreatorResources`,
// `CreatorCount`, `TagIndex`, `PendingTransfer`) must be cleaned consistently
// regardless of which non-terminal state the resource transitioned from, while
// canonical audit state (`Resource`, `Index`, `Count`) stays intact.

#[test]
fn tombstone_cleans_all_derived_listed_indexes_from_every_non_terminal_state() {
    let (env, creator, admin, client) = setup_with_admin();
    let tag_name = String::from_str(&env, "cleanup");
    let from_listed = register_tagged(&env, &creator, &client, "tmbcln1", &["cleanup"]);
    let from_delisted = register_tagged(&env, &creator, &client, "tmbcln2", &["cleanup"]);
    let from_frozen = register_tagged(&env, &creator, &client, "tmbcln3", &["cleanup"]);
    let from_disputed = register_tagged(&env, &creator, &client, "tmbcln4", &["cleanup"]);
    let live = register_tagged(&env, &creator, &client, "tmbcln5", &["cleanup"]);

    client.delist(&from_delisted);
    client.freeze_resource(&from_frozen);
    client.open_dispute(&from_disputed, &admin);

    assert_eq!(client.listed_count(), 2);
    assert_eq!(client.creator_listed_count(&creator), 2);
    assert_eq!(client.creator_resource_count(&creator), 5);

    // Tombstone from Listed -> listed counts drop by 1, resource count drops by 1.
    client.tombstone_resource(&from_listed, &admin);
    assert_eq!(client.listed_count(), 1);
    assert_eq!(client.creator_listed_count(&creator), 1);
    assert_eq!(client.creator_resource_count(&creator), 4);

    // Tombstone from Delisted, Frozen, Disputed -> listed counts stay at 1.
    client.tombstone_resource(&from_delisted, &admin);
    client.tombstone_resource(&from_frozen, &admin);
    client.tombstone_resource(&from_disputed, &admin);

    assert_eq!(client.listed_count(), 1);
    assert_eq!(client.creator_listed_count(&creator), 1);
    assert_eq!(client.creator_resource_count(&creator), 1);

    // Every discovery query surfaces only the surviving live resource.
    assert_eq!(ids_of(&env, &client.list_listed(&0u32, &20u32)), ["tmbcln5"]);
    let listed_page = client.list_listed_page(&0u32, &20u32);
    assert_eq!(ids_of(&env, &listed_page.items), ["tmbcln5"]);
    assert_eq!(listed_page.next_cursor, None);

    assert_eq!(
        ids_of(&env, &client.list_by_creator(&creator, &0u32, &20u32)),
        ["tmbcln5"]
    );
    let creator_page = client.list_by_creator_page(&creator, &IdCursor::Start, &20u32);
    assert_eq!(ids_of(&env, &creator_page.items), ["tmbcln5"]);
    assert_eq!(creator_page.next_cursor, IdCursor::End);

    assert_eq!(
        ids_of(&env, &client.list_by_tag(&tag_name, &0u32, &20u32)),
        ["tmbcln5"]
    );
    let tag_page = client.list_by_tag_page(&tag_name, &IdCursor::Start, &20u32);
    assert_eq!(ids_of(&env, &tag_page.items), ["tmbcln5"]);
    assert_eq!(tag_page.next_cursor, IdCursor::End);

    // Tombstoned entries are still visible via list_by_state(Tombstoned) and list_page.
    let tombstoned_page = client.list_by_state(&ResourceState::Tombstoned, &0u32, &20u32);
    assert_eq!(
        ids_of(&env, &tombstoned_page.items),
        ["tmbcln1", "tmbcln2", "tmbcln3", "tmbcln4"]
    );
    assert_eq!(client.count(), 5);
    assert_eq!(client.get(&live).state, ResourceState::Listed);
}

#[test]
fn tombstone_removes_empty_derived_index_entries_from_persistent_storage() {
    let (env, creator, admin, client) = setup_with_admin();
    let tag = String::from_str(&env, "ephemeral");
    let id = register_tagged(&env, &creator, &client, "tmbempty", &["ephemeral"]);
    let proposed = Address::generate(&env);
    client.propose_transfer(&id, &proposed);

    client.tombstone_resource(&id, &admin);

    env.as_contract(&client.address, || {
        assert!(
            !env.storage()
                .persistent()
                .has(&DataKey::TagIndex(tag.clone())),
            "empty TagIndex entry must be removed on tombstone"
        );
        assert!(
            !env.storage()
                .persistent()
                .has(&DataKey::CreatorResources(creator.clone())),
            "empty CreatorResources entry must be removed on tombstone"
        );
        assert!(
            !env.storage()
                .persistent()
                .has(&DataKey::PendingTransfer(id.clone())),
            "PendingTransfer must be removed on tombstone"
        );
        assert!(
            env.storage()
                .persistent()
                .has(&DataKey::Resource(id.clone())),
            "canonical Resource entry must remain for audit"
        );
        assert!(
            env.storage().persistent().has(&DataKey::Index(0)),
            "canonical Index entry must remain for audit"
        );
    });

    // Cancelled pending transfer cannot resurrect the tombstoned resource.
    assert_eq!(
        client.try_accept_transfer(&id),
        Err(Ok(Error::NoPendingTransfer))
    );
    assert_eq!(client.listed_count(), 0);
    assert_eq!(client.creator_listed_count(&creator), 0);
    assert_eq!(client.creator_listed_count(&proposed), 0);
}

#[test]
fn list_listed_page_excludes_resources_tombstoned_between_pages() {
    let (env, creator, admin, client) = setup_with_admin();
    for i in 0..5u32 {
        register_default(&env, &creator, &client, &format!("tmblpg{i}"));
    }

    let first = client.list_listed_page(&0u32, &2u32);
    assert_eq!(ids_of(&env, &first.items), ["tmblpg0", "tmblpg1"]);
    assert_eq!(first.next_cursor, Some(2u32));

    // Tombstone an already-visited slot and an upcoming slot mid-walk.
    client.tombstone_resource(&String::from_str(&env, "tmblpg0"), &admin);
    client.tombstone_resource(&String::from_str(&env, "tmblpg2"), &admin);

    let second = client.list_listed_page(&2u32, &2u32);
    assert_eq!(ids_of(&env, &second.items), ["tmblpg3", "tmblpg4"]);
    assert_eq!(second.next_cursor, None);
    assert_eq!(client.listed_count(), 3);
    assert_eq!(client.creator_listed_count(&creator), 3);
}

#[test]
fn tombstone_after_transfer_cleans_new_owners_listed_and_creator_indexes() {
    let (env, original_creator, admin, client) = setup_with_admin();
    let new_owner = Address::generate(&env);
    let id = register_default(&env, &original_creator, &client, "tmbxfr1");

    client.transfer_ownership(&id, &new_owner);
    assert_eq!(client.creator_listed_count(&original_creator), 0);
    assert_eq!(client.creator_listed_count(&new_owner), 1);

    client.tombstone_resource(&id, &admin);

    assert_eq!(client.listed_count(), 0);
    assert_eq!(client.creator_listed_count(&original_creator), 0);
    assert_eq!(client.creator_listed_count(&new_owner), 0);
    assert_eq!(client.creator_resource_count(&new_owner), 0);
    assert_eq!(client.list_listed(&0u32, &20u32).len(), 0);
    assert_eq!(client.list_by_creator(&new_owner, &0u32, &20u32).len(), 0);
}

#[test]
fn repair_tag_index_never_reindexes_tombstoned_resources() {
    let (env, creator, admin, client) = setup_with_admin();
    let tag = String::from_str(&env, "repairkeep");
    let live = register_tagged(&env, &creator, &client, "tmbrep1", &["repairkeep"]);
    let retired = register_tagged(&env, &creator, &client, "tmbrep2", &["repairkeep"]);

    client.tombstone_resource(&retired, &admin);

    // Even if an operator includes the tombstoned id in a tag-index repair list,
    // TagIndex must only store the surviving non-tombstoned resource.
    client.repair_tag_index(&Vec::from_array(&env, [live.clone(), retired.clone()]));

    let stored_ids: Vec<String> = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get(&DataKey::TagIndex(tag.clone()))
            .unwrap()
    });
    assert_eq!(stored_ids.len(), 1);
    assert_eq!(stored_ids.get(0).unwrap(), live);
    assert_eq!(
        ids_of(&env, &client.list_by_tag(&tag, &0u32, &20u32)),
        ["tmbrep1"]
    );
}
