// ─── Tag popularity counter and capped top-N read (#244) ────────────────────
//
// `top_tags(limit)` returns a bounded top-N view of tags ordered by descending
// successful-registration count, breaking ties lexicographically by normalized
// tag name. These tests complement the core assertions in `test.rs` with batch
// registrations, alternate registration entry points, tombstoning, pause reads,
// normalization, and full-table tie-break eviction.

#[test]
fn top_tags_tracks_register_batch_and_ignores_failed_batch_items() {
    let (env, creator, client) = setup();
    let metadata = String::from_str(&env, "ipfs://batchtags");

    let items = Vec::from_array(
        &env,
        [
            BatchRegisterItem {
                id: String::from_str(&env, "ttbatch1"),
                price: 100i128,
                metadata: metadata.clone(),
                tags: tags(&env, &["Rust", "Soroban"]),
                content_hash: None,
            },
            // Invalid price — must fail and must not count its tags.
            BatchRegisterItem {
                id: String::from_str(&env, "ttbatchbad"),
                price: 0i128,
                metadata: metadata.clone(),
                tags: tags(&env, &["ghost", "rust"]),
                content_hash: None,
            },
            BatchRegisterItem {
                id: String::from_str(&env, "ttbatch2"),
                price: 200i128,
                metadata,
                tags: tags(&env, &["rust", "stellar"]),
                content_hash: None,
            },
        ],
    );

    let batch = client.register_batch(&creator, &items);
    assert_eq!(batch.succeeded.len(), 2);
    assert_eq!(batch.failed.len(), 1);

    let top = client.top_tags(&10u32);
    assert_eq!(top.len(), 3);
    assert_eq!(
        top.get(0).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "rust"),
            count: 2,
        }
    );
    assert_eq!(
        top.get(1).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "soroban"),
            count: 1,
        }
    );
    assert_eq!(
        top.get(2).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "stellar"),
            count: 1,
        }
    );
}

#[test]
fn top_tags_tracks_register_with_hash_and_register_with_memo() {
    let (env, creator, client) = setup();
    let metadata = String::from_str(&env, "ipfs://hashtags");

    client.register_with_hash(
        &creator,
        &String::from_str(&env, "tthash1"),
        &100i128,
        &metadata,
        &tags(&env, &["ml", "dataset"]),
        &Some(String::from_str(&env, "sha256:hash1")),
    );
    client.register_with_memo(
        &creator,
        &String::from_str(&env, "ttmemo1"),
        &100i128,
        &metadata,
        &tags(&env, &["ML", "prompts"]),
        &None,
        &Some(BytesN::from_array(&env, &[7u8; 32])),
    );

    let top = client.top_tags(&10u32);
    assert_eq!(top.len(), 3);
    assert_eq!(
        top.get(0).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "ml"),
            count: 2,
        }
    );
    assert_eq!(
        top.get(1).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "dataset"),
            count: 1,
        }
    );
    assert_eq!(
        top.get(2).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "prompts"),
            count: 1,
        }
    );
}

#[test]
fn top_tags_preserves_historical_counts_after_tombstone() {
    let (env, creator, admin, client) = setup_with_admin();
    let id = String::from_str(&env, "tttomb1");

    client.register(
        &creator,
        &id,
        &100i128,
        &String::from_str(&env, "ipfs://tomb"),
        &tags(&env, &["historical"]),
    );
    client.tombstone_resource(&id, &admin);

    let top = client.top_tags(&5u32);
    assert_eq!(top.len(), 1);
    assert_eq!(
        top.get(0).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "historical"),
            count: 1,
        }
    );
}

#[test]
fn top_tags_works_while_paused() {
    let (env, creator, admin, client) = setup_with_admin();
    client.register(
        &creator,
        &String::from_str(&env, "ttpause1"),
        &100i128,
        &String::from_str(&env, "ipfs://paused"),
        &tags(&env, &["pausedtag"]),
    );
    client.set_paused(&admin, &true);

    let top = client.top_tags(&5u32);
    assert_eq!(top.len(), 1);
    assert_eq!(
        top.get(0).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "pausedtag"),
            count: 1,
        }
    );
}

#[test]
fn top_tags_trims_and_lowercases_tags_and_honors_sub_cap_limit() {
    let (env, creator, client) = setup();
    let metadata = String::from_str(&env, "ipfs://normtags");

    client.register(
        &creator,
        &String::from_str(&env, "ttnorm1"),
        &100i128,
        &metadata,
        &tags(&env, &["  AI  ", "beta", "gamma"]),
    );
    client.register(
        &creator,
        &String::from_str(&env, "ttnorm2"),
        &100i128,
        &metadata,
        &tags(&env, &["ai", "BETA"]),
    );
    client.register(
        &creator,
        &String::from_str(&env, "ttnorm3"),
        &100i128,
        &metadata,
        &tags(&env, &["Ai"]),
    );

    let top_two = client.top_tags(&2u32);
    assert_eq!(top_two.len(), 2);
    assert_eq!(
        top_two.get(0).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "ai"),
            count: 3,
        }
    );
    assert_eq!(
        top_two.get(1).unwrap(),
        TagPopularity {
            tag: String::from_str(&env, "beta"),
            count: 2,
        }
    );
}

#[test]
fn top_tags_evicts_lexicographically_larger_tie_when_full() {
    let (env, creator, client) = setup();
    let metadata = String::from_str(&env, "ipfs://tietags");

    // Fill all TOP_TAGS_CAP (20) slots with count = 1: "tag01" .. "tag20".
    for i in 1..=TOP_TAGS_CAP {
        let id = format!("tttie{:02}", i);
        let tag = format!("tag{:02}", i);
        client.register(
            &creator,
            &String::from_str(&env, &id),
            &100i128,
            &metadata,
            &tags(&env, &[tag.as_str()]),
        );
    }

    // "tag00" ties at count = 1 but sorts before "tag01", so it must enter at
    // index 0 and evict "tag20" to preserve the TOP_TAGS_CAP bound.
    client.register(
        &creator,
        &String::from_str(&env, "tttie00"),
        &100i128,
        &metadata,
        &tags(&env, &["tag00"]),
    );

    let top = client.top_tags(&TOP_TAGS_CAP);
    assert_eq!(top.len(), TOP_TAGS_CAP);
    assert_eq!(top.get(0).unwrap().tag, String::from_str(&env, "tag00"));
    assert_eq!(
        top.get(TOP_TAGS_CAP - 1).unwrap().tag,
        String::from_str(&env, "tag19")
    );
}
