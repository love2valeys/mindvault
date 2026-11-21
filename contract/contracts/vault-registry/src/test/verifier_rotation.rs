// ── Admin verifier recovery key rotation flow (#228) ─────────────────────────
//
// `reset_verifier` (and `rotate_verifier`) atomically revoke a compromised
// verifier key, grant the replacement key, update `role_inventory`, and emit
// `rmverif`, `addverif`, and `verrot` (`VerifierRotation`) audit events.

#[test]
fn reset_verifier_atomically_rotates_key_updates_inventory_and_emits_audit_events() {
    let (env, _creator, admin, client) = setup_with_admin();
    let old_verifier = Address::generate(&env);
    let new_verifier = Address::generate(&env);

    client.add_verifier(&old_verifier);
    let ledger = env.ledger().sequence();
    client.reset_verifier(&old_verifier, &new_verifier);

    assert!(!client.is_verifier(&old_verifier));
    assert!(client.is_verifier(&new_verifier));

    let members = client.list_role_members(&RoleKind::Verifier, &0u32, &20u32);
    assert_eq!(members.members.len(), 1);
    assert_eq!(members.members.get(0).unwrap(), new_verifier);
    assert_eq!(members.next_cursor, None);

    let events = env.events().all();
    let mut saw_rmverif = false;
    let mut saw_addverif = false;
    let mut saw_verrot = false;

    for i in 0..events.len() {
        let (contract, topics, data) = events.get(i).unwrap();
        if contract != client.address {
            continue;
        }
        let topic: Symbol = topics.get(0).unwrap().try_into_val(&env).unwrap();
        if topic == symbol_short!("rmverif") {
            let change: RoleChange = data.try_into_val(&env).unwrap();
            if change.target == old_verifier {
                assert_eq!(
                    change,
                    RoleChange {
                        admin: admin.clone(),
                        target: old_verifier.clone(),
                        granted: false,
                    }
                );
                saw_rmverif = true;
            }
        } else if topic == symbol_short!("addverif") {
            let change: RoleChange = data.try_into_val(&env).unwrap();
            if change.target == new_verifier {
                assert_eq!(
                    change,
                    RoleChange {
                        admin: admin.clone(),
                        target: new_verifier.clone(),
                        granted: true,
                    }
                );
                saw_addverif = true;
            }
        } else if topic == symbol_short!("verrot") {
            assert_eq!(topics.len(), 2);
            let topic_old: Address = topics.get(1).unwrap().try_into_val(&env).unwrap();
            let payload: VerifierRotation = data.try_into_val(&env).unwrap();
            assert_eq!(topic_old, old_verifier);
            assert_eq!(
                payload,
                VerifierRotation {
                    old_verifier: old_verifier.clone(),
                    new_verifier: new_verifier.clone(),
                    ledger,
                }
            );
            saw_verrot = true;
        }
    }

    assert!(saw_rmverif, "reset_verifier must emit rmverif");
    assert!(saw_addverif, "reset_verifier must emit addverif");
    assert!(saw_verrot, "reset_verifier must emit verrot audit event");
}

#[test]
fn reset_verifier_immediately_transfers_verification_authority() {
    let (env, creator, _admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "verreset1");
    let compromised = Address::generate(&env);
    let replacement = Address::generate(&env);

    client.add_verifier(&compromised);
    client.reset_verifier(&compromised, &replacement);

    assert_eq!(
        client.try_set_verification_status(
            &id,
            &compromised,
            &VerificationStatus::Verified,
            &None::<String>,
        ),
        Err(Ok(Error::NotVerifier))
    );

    client.set_verification_status(
        &id,
        &replacement,
        &VerificationStatus::Verified,
        &Some(String::from_str(&env, "sha256:abcdef0123456789")),
    );
    assert_eq!(client.get(&id).verified, VerificationStatus::Verified);
}

#[test]
fn reset_verifier_succeeds_during_emergency_pause() {
    let (env, _creator, admin, client) = setup_with_admin();
    let compromised = Address::generate(&env);
    let replacement = Address::generate(&env);

    client.add_verifier(&compromised);
    client.set_paused(&admin, &true);
    assert!(client.is_paused());

    client.reset_verifier(&compromised, &replacement);
    assert!(!client.is_verifier(&compromised));
    assert!(client.is_verifier(&replacement));
}

#[test]
fn reset_verifier_fails_before_admin_is_set() {
    let (env, _creator, client) = setup();
    let old_verifier = Address::generate(&env);
    let new_verifier = Address::generate(&env);

    assert_eq!(
        client.try_reset_verifier(&old_verifier, &new_verifier),
        Err(Ok(Error::AdminNotSet))
    );
}

#[test]
fn reset_verifier_requires_admin_auth() {
    let (env, _creator, _admin, client) = setup_with_admin();
    let old_verifier = Address::generate(&env);
    let new_verifier = Address::generate(&env);
    client.add_verifier(&old_verifier);

    env.mock_auths(&[]);
    assert!(client
        .try_reset_verifier(&old_verifier, &new_verifier)
        .is_err());

    env.mock_all_auths();
    assert!(client.is_verifier(&old_verifier));
    assert!(!client.is_verifier(&new_verifier));
}

#[test]
fn reset_verifier_rejects_invalid_keys_without_mutating_state() {
    let (env, _creator, _admin, client) = setup_with_admin();
    let active_a = Address::generate(&env);
    let active_b = Address::generate(&env);
    let unknown = Address::generate(&env);
    let fresh = Address::generate(&env);

    client.add_verifier(&active_a);
    client.add_verifier(&active_b);

    // Unregistered old key -> NotFound
    assert_eq!(
        client.try_reset_verifier(&unknown, &fresh),
        Err(Ok(Error::NotFound))
    );
    // Same key -> AlreadyRegistered
    assert_eq!(
        client.try_reset_verifier(&active_a, &active_a),
        Err(Ok(Error::AlreadyRegistered))
    );
    // Replacement already registered -> AlreadyRegistered
    assert_eq!(
        client.try_reset_verifier(&active_a, &active_b),
        Err(Ok(Error::AlreadyRegistered))
    );

    assert!(client.is_verifier(&active_a));
    assert!(client.is_verifier(&active_b));
    assert!(!client.is_verifier(&unknown));
    assert!(!client.is_verifier(&fresh));
}

#[test]
fn reset_verifier_succeeds_when_verifier_inventory_is_at_capacity() {
    let (env, _creator, _admin, client) = setup_with_admin();
    let first_verifier = Address::generate(&env);
    client.add_verifier(&first_verifier);
    for _ in 1..MAX_ROLE_MEMBERS {
        client.add_verifier(&Address::generate(&env));
    }

    let replacement = Address::generate(&env);
    client.reset_verifier(&first_verifier, &replacement);

    assert!(!client.is_verifier(&first_verifier));
    assert!(client.is_verifier(&replacement));
}
