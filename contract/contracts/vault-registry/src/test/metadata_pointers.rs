// ─── Multi-scheme metadata pointers (#245) ──────────────────────────────────
//
// `set_metadata_pointers(id, pointers, primary)` attaches up to
// `MAX_METADATA_POINTERS` (4) redundant pointers across schemes (IPFS, Arweave,
// hosted HTTPS, content hashes) with a preferred `primary` index so a resource
// survives a single storage outage. Single-pointer readers continue reading
// `Resource.metadata`, which always mirrors `pointers[primary]`.

fn ptr_vec(env: &Env, items: &[&str]) -> Vec<String> {
    let mut out = Vec::new(env);
    for item in items {
        out.push_back(String::from_str(env, item));
    }
    out
}

#[test]
fn get_metadata_pointers_falls_back_to_single_primary_when_unset() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpfallback");

    let record = client.get_metadata_pointers(&id);
    assert_eq!(
        record,
        MetadataPointers {
            pointers: ptr_vec(&env, &["ipfs://m"]),
            primary: 0,
        }
    );
}

#[test]
fn set_metadata_pointers_stores_multi_scheme_mirrors_and_updates_primary() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpmulti");
    let sha_ptr = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mirrors = ptr_vec(
        &env,
        &[
            "ipfs://QmPrimaryMirror",
            "ar://arweaveTxMirror123",
            "https://cdn.example.org/vault/mpmulti.json",
            sha_ptr,
        ],
    );

    client.set_metadata_pointers(&id, &mirrors, &1u32);

    assert_eq!(last_call_topics(&env), ["updmeta", "setptrs"]);
    assert_eq!(
        client.get_metadata_pointers(&id),
        MetadataPointers {
            pointers: mirrors,
            primary: 1,
        }
    );
    // Single-pointer readers see the preferred primary transparently.
    assert_eq!(
        client.get(&id).metadata,
        String::from_str(&env, "ar://arweaveTxMirror123")
    );
}

#[test]
fn set_metadata_pointers_keeping_primary_skips_updmeta_event() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpsamepri");
    let mirrors = ptr_vec(
        &env,
        &[
            "ar://mirror1",
            "ipfs://m",
            "https://hosted.example.org/mpsamepri",
        ],
    );

    client.set_metadata_pointers(&id, &mirrors, &1u32);

    assert_eq!(last_call_topics(&env), ["setptrs"]);
    assert_eq!(
        client.get_metadata_pointers(&id),
        MetadataPointers {
            pointers: mirrors,
            primary: 1,
        }
    );
    assert_eq!(client.get(&id).metadata, String::from_str(&env, "ipfs://m"));
}

#[test]
fn set_metadata_pointers_allows_mirror_updates_when_frozen_or_hashed_if_primary_unchanged() {
    let (env, creator, client) = setup();
    let frozen_id = register_default(&env, &creator, &client, "mpfrozen");
    client.freeze_metadata(&frozen_id);

    // Adding mirrors while keeping the frozen primary succeeds.
    let frozen_mirrors = ptr_vec(
        &env,
        &["ipfs://m", "ar://frozen-backup", "https://cdn.example.org/f"],
    );
    client.set_metadata_pointers(&frozen_id, &frozen_mirrors, &0u32);
    assert_eq!(
        client.get_metadata_pointers(&frozen_id),
        MetadataPointers {
            pointers: frozen_mirrors,
            primary: 0,
        }
    );

    // Changing the primary on a frozen resource is rejected with MetadataFrozen.
    assert_eq!(
        client.try_set_metadata_pointers(
            &frozen_id,
            &ptr_vec(&env, &["ipfs://m", "ar://frozen-backup"]),
            &1u32,
        ),
        Err(Ok(Error::MetadataFrozen))
    );

    // Same rule for a resource registered with an immutable content_hash.
    let hashed_id = String::from_str(&env, "mphashed");
    client.register_with_hash(
        &creator,
        &hashed_id,
        &100i128,
        &String::from_str(&env, "ipfs://hashed"),
        &empty_tags(&env),
        &Some(String::from_str(&env, "sha256:content-digest")),
    );
    let hashed_mirrors = ptr_vec(&env, &["ar://hashed-mirror", "ipfs://hashed"]);
    client.set_metadata_pointers(&hashed_id, &hashed_mirrors, &1u32);
    assert_eq!(client.get_metadata_pointers(&hashed_id).primary, 1);

    assert_eq!(
        client.try_set_metadata_pointers(&hashed_id, &hashed_mirrors, &0u32),
        Err(Ok(Error::MetadataFrozen))
    );
}

#[test]
fn set_metadata_pointers_rejects_empty_oversized_out_of_range_and_duplicate_lists() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpinvalid");

    // Empty pointer list.
    assert_eq!(
        client.try_set_metadata_pointers(&id, &Vec::new(&env), &0u32),
        Err(Ok(Error::InvalidMetadataPointers))
    );

    // More than MAX_METADATA_POINTERS (4) entries.
    let too_many = ptr_vec(
        &env,
        &[
            "ipfs://1",
            "ar://2",
            "https://example.org/3",
            "http://example.org/4",
            "0xdeadbeef",
        ],
    );
    assert!(too_many.len() > MAX_METADATA_POINTERS);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &too_many, &0u32),
        Err(Ok(Error::InvalidMetadataPointers))
    );

    // Out-of-range primary index.
    let two = ptr_vec(&env, &["ipfs://1", "ar://2"]);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &two, &2u32),
        Err(Ok(Error::InvalidMetadataPointers))
    );

    // Duplicate pointers.
    let dup = ptr_vec(&env, &["ipfs://dup", "ar://ok", "ipfs://dup"]);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &dup, &0u32),
        Err(Ok(Error::InvalidMetadataPointers))
    );
}

#[test]
fn set_metadata_pointers_validates_every_pointer_entry() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpvalidate");

    assert_eq!(
        client.try_set_metadata_pointers(&id, &ptr_vec(&env, &["ipfs://ok", ""]), &0u32),
        Err(Ok(Error::EmptyMetadata))
    );
    assert_eq!(
        client.try_set_metadata_pointers(
            &id,
            &ptr_vec(&env, &["ipfs://ok", "ftp://unsupported.example.org/file"]),
            &0u32,
        ),
        Err(Ok(Error::InvalidMetadataPointer))
    );

    let overlong = format!(
        "ipfs://{}",
        "a".repeat(MAX_METADATA_POINTER_LEN as usize)
    );
    assert_eq!(
        client.try_set_metadata_pointers(
            &id,
            &ptr_vec(&env, &["ipfs://ok", overlong.as_str()]),
            &0u32,
        ),
        Err(Ok(Error::MetadataTooLong))
    );
}

#[test]
fn update_metadata_clears_mirrors_and_noop_preserves_them() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpclear");
    let mirrors = ptr_vec(&env, &["ipfs://m", "ar://mirror"]);

    client.set_metadata_pointers(&id, &mirrors, &0u32);

    // No-op update_metadata leaves existing mirrors intact.
    client.update_metadata(&id, &String::from_str(&env, "ipfs://m"));
    assert_eq!(client.get_metadata_pointers(&id).pointers.len(), 2);

    // Changing primary via update_metadata invalidates stale mirrors.
    let updated = String::from_str(&env, "https://hosted.example.org/new");
    client.update_metadata(&id, &updated);
    assert_eq!(
        client.get_metadata_pointers(&id),
        MetadataPointers {
            pointers: Vec::from_array(&env, [updated]),
            primary: 0,
        }
    );
}

#[test]
#[should_panic]
fn set_metadata_pointers_rejects_non_creator() {
    let (env, _owner, stranger, client, id) = setup_strict_auth();
    let pointers = ptr_vec(&env, &["ipfs://auth", "ar://stranger"]);
    let invoke = MockAuthInvoke {
        contract: &client.address,
        fn_name: "set_metadata_pointers",
        args: (id.clone(), pointers.clone(), 0u32).into_val(&env),
        sub_invokes: &[],
    };
    let auths = [MockAuth {
        address: &stranger,
        invoke: &invoke,
    }];
    client
        .mock_auths(&auths)
        .set_metadata_pointers(&id, &pointers, &0u32);
}

#[test]
fn set_metadata_pointers_enforces_pause_and_mutability_while_reads_stay_available() {
    let (env, creator, admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "mppause");
    let mirrors = ptr_vec(&env, &["ipfs://m", "ar://backup"]);
    client.set_metadata_pointers(&id, &mirrors, &0u32);

    // While paused, writes fail with ContractPaused while reads keep working.
    client.set_paused(&admin, &true);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &mirrors, &0u32),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(client.get_metadata_pointers(&id).pointers.len(), 2);
    client.set_paused(&admin, &false);

    // Frozen, disputed, and tombstoned resources reject mirror writes.
    client.freeze_resource(&id);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &mirrors, &0u32),
        Err(Ok(Error::ResourceNotMutable))
    );
    client.open_dispute(&id, &admin);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &mirrors, &0u32),
        Err(Ok(Error::ResourceNotMutable))
    );
    client.tombstone_resource(&id, &admin);
    assert_eq!(
        client.try_set_metadata_pointers(&id, &mirrors, &0u32),
        Err(Ok(Error::ResourceNotMutable))
    );
    // Mirrors stay readable for audit after tombstoning.
    assert_eq!(client.get_metadata_pointers(&id).pointers.len(), 2);
}

#[test]
fn get_metadata_pointers_rejects_missing_and_malformed_ids() {
    let (env, _creator, client) = setup();
    let missing = String::from_str(&env, "mpmissing");
    let malformed = String::from_str(&env, "Bad Id!");

    assert_eq!(
        client.try_get_metadata_pointers(&missing),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.try_get_metadata_pointers(&malformed),
        Err(Ok(Error::InvalidResourceId))
    );
}

#[test]
fn metadata_pointers_ttl_is_bumped_on_write_save_and_extend_resource_ttl() {
    let (env, creator, client) = setup();
    let id = register_default(&env, &creator, &client, "mpttl");
    let mirrors = ptr_vec(&env, &["ipfs://m", "ar://ttl-mirror"]);
    client.set_metadata_pointers(&id, &mirrors, &0u32);

    // 1. extend_resource_ttl refreshes both Resource and MetadataPointers TTL.
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    client.extend_resource_ttl(&creator, &id);
    let (res_ttl, ptrs_ttl) = env.as_contract(&client.address, || {
        (
            env.storage()
                .persistent()
                .get_ttl(&DataKey::Resource(id.clone())),
            env.storage()
                .persistent()
                .get_ttl(&DataKey::MetadataPointers(id.clone())),
        )
    });
    assert_eq!(res_ttl, TTL_BUMP_AMOUNT);
    assert_eq!(ptrs_ttl, TTL_BUMP_AMOUNT);

    // 2. Resource mutations via save() (e.g. set_price) refresh MetadataPointers TTL.
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    client.set_price(&id, &250i128);
    let ptrs_ttl_after_save = env.as_contract(&client.address, || {
        env.storage()
            .persistent()
            .get_ttl(&DataKey::MetadataPointers(id.clone()))
    });
    assert_eq!(ptrs_ttl_after_save, TTL_BUMP_AMOUNT);

    // 3. Mirror-only set_metadata_pointers refreshes Resource TTL as well.
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);
    client.set_metadata_pointers(
        &id,
        &ptr_vec(&env, &["ipfs://m", "ar://ttl-mirror-2"]),
        &0u32,
    );
    let (res_ttl_after_ptrs, ptrs_ttl_after_ptrs) = env.as_contract(&client.address, || {
        (
            env.storage()
                .persistent()
                .get_ttl(&DataKey::Resource(id.clone())),
            env.storage()
                .persistent()
                .get_ttl(&DataKey::MetadataPointers(id.clone())),
        )
    });
    assert_eq!(res_ttl_after_ptrs, TTL_BUMP_AMOUNT);
    assert_eq!(ptrs_ttl_after_ptrs, TTL_BUMP_AMOUNT);
}
