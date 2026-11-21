// ─── Verifier status query pagination design & edge-case tests (#291) ────────
//
// Exercises both on-chain verification pagination surfaces documented in
// `docs/verifier-status-pagination-design.md`:
//   1. `list_by_verification_status(status, cursor, limit) -> CatalogPage`
//   2. `get_verifier_status_history(verifier, cursor, limit) -> VerifierStatusPage`

#[test]
fn list_by_verification_status_covers_all_four_statuses_including_revoked() {
    let (env, creator, admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);
    let _ = admin;

    let id_pending = register_default(&env, &creator, &client, "vstat0");
    let id_verified = register_default(&env, &creator, &client, "vstat1");
    let id_rejected = register_default(&env, &creator, &client, "vstat2");
    let id_revoked_direct = register_default(&env, &creator, &client, "vstat3");
    let id_revoked_reason = register_default(&env, &creator, &client, "vstat4");

    client.set_verification_status(&id_verified, &verifier, &VerificationStatus::Verified, &None);
    client.set_verification_status(&id_rejected, &verifier, &VerificationStatus::Rejected, &None);

    client.set_verification_status(
        &id_revoked_direct,
        &verifier,
        &VerificationStatus::Verified,
        &None,
    );
    client.set_verification_status(
        &id_revoked_direct,
        &verifier,
        &VerificationStatus::Revoked,
        &None,
    );

    client.set_verification_status(
        &id_revoked_reason,
        &verifier,
        &VerificationStatus::Verified,
        &None,
    );
    client.revoke_attestation(
        &id_revoked_reason,
        &verifier,
        &String::from_str(&env, "sha256:revokedreason"),
    );

    let pending = client.list_by_verification_status(&VerificationStatus::Pending, &0u32, &20u32);
    assert_eq!(ids_of(&env, &pending.items), ["vstat0"]);
    assert_eq!(pending.next_cursor, None);

    let verified = client.list_by_verification_status(&VerificationStatus::Verified, &0u32, &20u32);
    assert_eq!(ids_of(&env, &verified.items), ["vstat1"]);
    assert_eq!(verified.next_cursor, None);

    let rejected = client.list_by_verification_status(&VerificationStatus::Rejected, &0u32, &20u32);
    assert_eq!(ids_of(&env, &rejected.items), ["vstat2"]);
    assert_eq!(rejected.next_cursor, None);

    let revoked = client.list_by_verification_status(&VerificationStatus::Revoked, &0u32, &20u32);
    assert_eq!(ids_of(&env, &revoked.items), ["vstat3", "vstat4"]);
    assert_eq!(revoked.next_cursor, None);
}

#[test]
fn list_by_verification_status_cursor_advances_by_catalog_slot_not_match_count() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    // Verified at sparse catalog slots 0, 2, 4; Pending at slots 1, 3, 5.
    for i in 0..6u32 {
        let id = register_default(&env, &creator, &client, &format!("vcur{i}"));
        if i % 2 == 0 {
            client.set_verification_status(&id, &verifier, &VerificationStatus::Verified, &None);
        }
    }

    // Page 1 (limit 2): collects slots 0 and 2, stops at catalog slot 3.
    let p1 = client.list_by_verification_status(&VerificationStatus::Verified, &0u32, &2u32);
    assert_eq!(ids_of(&env, &p1.items), ["vcur0", "vcur2"]);
    assert_eq!(p1.next_cursor, Some(3u32));

    // Page 2 from slot 3: collects slot 4 and reaches end-of-catalog.
    let p2 = client.list_by_verification_status(&VerificationStatus::Verified, &3u32, &2u32);
    assert_eq!(ids_of(&env, &p2.items), ["vcur4"]);
    assert_eq!(p2.next_cursor, None);
}

#[test]
fn list_by_verification_status_cursor_is_stable_when_status_flips_between_pages() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    for i in 0..5u32 {
        let id = register_default(&env, &creator, &client, &format!("vflip{i}"));
        client.set_verification_status(&id, &verifier, &VerificationStatus::Verified, &None);
    }

    let first = client.list_by_verification_status(&VerificationStatus::Verified, &0u32, &2u32);
    assert_eq!(ids_of(&env, &first.items), ["vflip0", "vflip1"]);
    let cursor = first.next_cursor.unwrap();
    assert_eq!(cursor, 2u32);

    // Flip an already-returned slot (vflip0) and an unseen slot (vflip3) while
    // the caller holds `cursor`.
    client.set_verification_status(
        &String::from_str(&env, "vflip0"),
        &verifier,
        &VerificationStatus::Rejected,
        &None,
    );
    client.set_verification_status(
        &String::from_str(&env, "vflip3"),
        &verifier,
        &VerificationStatus::Revoked,
        &None,
    );

    let mut seen = ids_of(&env, &first.items);
    let mut next = Some(cursor);
    while let Some(c) = next {
        let page = client.list_by_verification_status(&VerificationStatus::Verified, &c, &2u32);
        seen.extend(ids_of(&env, &page.items));
        next = page.next_cursor;
    }

    // Slot-based cursor never repeats vflip1 and skips newly revoked vflip3.
    assert_eq!(seen, ["vflip0", "vflip1", "vflip2", "vflip4"]);
}

#[test]
fn list_by_verification_status_enforces_page_cap_zero_limit_and_past_end_cursor() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    for i in 0..25u32 {
        let id = register_default(&env, &creator, &client, &format!("vcap{i:02}"));
        client.set_verification_status(&id, &verifier, &VerificationStatus::Verified, &None);
    }

    // Limit above LIST_PAGE_CAP (20) is clamped to 20.
    let first = client.list_by_verification_status(&VerificationStatus::Verified, &0u32, &100u32);
    assert_eq!(first.items.len(), LIST_PAGE_CAP);
    assert_eq!(first.next_cursor, Some(LIST_PAGE_CAP));

    let second = client.list_by_verification_status(
        &VerificationStatus::Verified,
        &first.next_cursor.unwrap(),
        &100u32,
    );
    assert_eq!(second.items.len(), 5);
    assert_eq!(second.next_cursor, None);

    // Zero limit returns an empty page and preserves the cursor.
    let zero = client.list_by_verification_status(&VerificationStatus::Verified, &0u32, &0u32);
    assert_eq!(zero.items.len(), 0);
    assert_eq!(zero.next_cursor, Some(0u32));

    // Cursor at or past end returns empty page with next_cursor = None.
    let at_end = client.list_by_verification_status(&VerificationStatus::Verified, &25u32, &10u32);
    assert_eq!(at_end.items.len(), 0);
    assert_eq!(at_end.next_cursor, None);

    let past_end =
        client.list_by_verification_status(&VerificationStatus::Verified, &999u32, &10u32);
    assert_eq!(past_end.items.len(), 0);
    assert_eq!(past_end.next_cursor, None);
}

#[test]
fn list_by_verification_status_bumps_ttl_on_scanned_entries() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let id = register_default(&env, &creator, &client, "vttl01");
    client.set_verification_status(&id, &verifier, &VerificationStatus::Verified, &None);

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);

    let page = client.list_by_verification_status(&VerificationStatus::Verified, &0u32, &20u32);
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        resource_storage_ttl(&env, &client.address, &id),
        TTL_BUMP_AMOUNT
    );
    assert_eq!(
        persistent_entry_ttl(&env, &client.address, &DataKey::Index(0)),
        TTL_BUMP_AMOUNT
    );
}

#[test]
fn verifier_status_history_enforces_cap_zero_limit_and_past_end_cursor() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    for i in 0..24u32 {
        let id = register_default(&env, &creator, &client, &format!("vhist{i:02}"));
        client.set_verification_status(&id, &verifier, &VerificationStatus::Verified, &None);
    }
    // Add a 25th entry via revoke_attestation to confirm chronological ordering across both writers.
    let last_id = String::from_str(&env, "vhist23");
    client.revoke_attestation(
        &last_id,
        &verifier,
        &String::from_str(&env, "sha256:histrevk"),
    );

    let first = client.get_verifier_status_history(&verifier, &0u32, &100u32);
    assert_eq!(first.items.len(), LIST_PAGE_CAP);
    assert_eq!(first.next_cursor, Some(LIST_PAGE_CAP));
    assert_eq!(
        first.items.get(0).unwrap().resource_id,
        String::from_str(&env, "vhist00")
    );

    let second = client.get_verifier_status_history(&verifier, &first.next_cursor.unwrap(), &100u32);
    assert_eq!(second.items.len(), 5);
    assert_eq!(second.next_cursor, None);
    let final_entry = second.items.get(4).unwrap();
    assert_eq!(final_entry.resource_id, last_id);
    assert_eq!(final_entry.old_status, VerificationStatus::Verified);
    assert_eq!(final_entry.new_status, VerificationStatus::Revoked);

    // Zero limit returns empty page without losing position.
    let zero = client.get_verifier_status_history(&verifier, &0u32, &0u32);
    assert_eq!(zero.items.len(), 0);
    assert_eq!(zero.next_cursor, Some(0u32));

    // Past-end cursor returns empty page and None cursor.
    let past = client.get_verifier_status_history(&verifier, &25u32, &10u32);
    assert_eq!(past.items.len(), 0);
    assert_eq!(past.next_cursor, None);
}
