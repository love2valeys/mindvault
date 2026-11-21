// ── Admin bootstrap replay protection tests (#293) ───────────────────────────
//
// Verifies that once an admin is established (via either single-key
// `nominate_new_admin` or dual-key `bootstrap_dual_admin`), replaying admin
// bootstrap fails cleanly and leaves stored admin, recovery admin, pending
// nomination, and all granted role memberships unchanged.

#[test]
fn bootstrap_dual_admin_rejects_replay_and_preserves_admin_and_roles() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);
    let verifier = Address::generate(&env);
    let settler = Address::generate(&env);
    let moderator = Address::generate(&env);
    let attacker = Address::generate(&env);
    let attacker_recovery = Address::generate(&env);

    client.bootstrap_dual_admin(&admin, &recovery);
    client.add_verifier(&verifier);
    client.add_settler(&settler);
    client.add_moderator(&moderator);

    // Replaying with the exact original keys must fail cleanly.
    assert_eq!(
        client.try_bootstrap_dual_admin(&admin, &recovery),
        Err(Ok(Error::AdminAlreadySet))
    );
    // Replaying with new attacker keys must also fail cleanly.
    assert_eq!(
        client.try_bootstrap_dual_admin(&attacker, &attacker_recovery),
        Err(Ok(Error::AdminAlreadySet))
    );

    // Stored admin, recovery admin, pending nomination, and all roles stay intact.
    assert_eq!(client.admin(), Some(admin));
    assert_eq!(client.recovery_admin(), Some(recovery));
    assert_eq!(client.pending_admin(), None);
    assert!(client.is_verifier(&verifier));
    assert!(client.is_settler(&settler));
    assert!(client.is_moderator(&moderator));
    assert!(!client.is_verifier(&attacker));
    assert!(!client.is_settler(&attacker));
    assert!(!client.is_moderator(&attacker));
}

#[test]
fn bootstrap_dual_admin_rejected_after_single_key_bootstrap() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let verifier = Address::generate(&env);
    let settler = Address::generate(&env);
    let moderator = Address::generate(&env);
    let attacker = Address::generate(&env);
    let attacker_recovery = Address::generate(&env);

    client.nominate_new_admin(&admin);
    client.add_verifier(&verifier);
    client.add_settler(&settler);
    client.add_moderator(&moderator);

    // Dual-key bootstrap cannot hijack or attach a recovery key after single-key setup.
    assert_eq!(
        client.try_bootstrap_dual_admin(&attacker, &attacker_recovery),
        Err(Ok(Error::AdminAlreadySet))
    );
    assert_eq!(
        client.try_bootstrap_dual_admin(&admin, &attacker_recovery),
        Err(Ok(Error::AdminAlreadySet))
    );

    assert_eq!(client.admin(), Some(admin));
    assert_eq!(client.recovery_admin(), None);
    assert_eq!(client.pending_admin(), None);
    assert!(client.is_verifier(&verifier));
    assert!(client.is_settler(&settler));
    assert!(client.is_moderator(&moderator));
}

#[test]
fn bootstrap_dual_admin_rejected_after_rotation_and_recovery() {
    let (env, _creator, client) = setup();
    let admin1 = Address::generate(&env);
    let admin2 = Address::generate(&env);
    let admin3 = Address::generate(&env);
    let recovery = Address::generate(&env);
    let verifier = Address::generate(&env);
    let attacker = Address::generate(&env);
    let attacker_recovery = Address::generate(&env);

    client.bootstrap_dual_admin(&admin1, &recovery);
    client.add_verifier(&verifier);

    // Complete a two-step admin rotation.
    client.nominate_new_admin(&admin2);
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + ADMIN_NOMINATION_MIN_GAP);
    client.accept_admin(&admin2);
    assert_eq!(client.admin(), Some(admin2.clone()));

    // Replay after rotation fails cleanly.
    assert_eq!(
        client.try_bootstrap_dual_admin(&admin1, &recovery),
        Err(Ok(Error::AdminAlreadySet))
    );
    assert_eq!(client.admin(), Some(admin2));
    assert_eq!(client.recovery_admin(), Some(recovery.clone()));
    assert!(client.is_verifier(&verifier));

    // Recover admin via recovery key, then replay bootstrap again.
    client.recover_admin(&admin3);
    assert_eq!(
        client.try_bootstrap_dual_admin(&attacker, &attacker_recovery),
        Err(Ok(Error::AdminAlreadySet))
    );
    assert_eq!(client.admin(), Some(admin3));
    assert_eq!(client.recovery_admin(), Some(recovery));
    assert_eq!(client.pending_admin(), None);
    assert!(client.is_verifier(&verifier));
}

#[test]
fn bootstrap_dual_admin_rejects_same_admin_and_recovery_before_setup() {
    let (env, _creator, client) = setup();
    let same = Address::generate(&env);
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);

    assert_eq!(
        client.try_bootstrap_dual_admin(&same, &same),
        Err(Ok(Error::SameAdmin))
    );
    assert_eq!(client.admin(), None);
    assert_eq!(client.recovery_admin(), None);

    // Rejected initial attempt leaves bootstrap available for valid distinct keys.
    client.bootstrap_dual_admin(&admin, &recovery);
    assert_eq!(client.admin(), Some(admin));
    assert_eq!(client.recovery_admin(), Some(recovery));
}

#[test]
fn single_key_bootstrap_replay_same_admin_fails_and_preserves_roles() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let verifier = Address::generate(&env);
    let settler = Address::generate(&env);
    let moderator = Address::generate(&env);

    client.nominate_new_admin(&admin);
    client.add_verifier(&verifier);
    client.add_settler(&settler);
    client.add_moderator(&moderator);

    // Replaying bootstrap with the already-installed admin fails with SameAdmin.
    assert_eq!(
        client.try_nominate_new_admin(&admin),
        Err(Ok(Error::SameAdmin))
    );

    assert_eq!(client.admin(), Some(admin));
    assert_eq!(client.pending_admin(), None);
    assert!(client.is_verifier(&verifier));
    assert!(client.is_settler(&settler));
    assert!(client.is_moderator(&moderator));
}

#[test]
#[should_panic]
fn single_key_bootstrap_replay_by_unauthenticated_attacker_panics() {
    // Without mock_all_auths, replaying `nominate_new_admin` signed only by an
    // attacker must fail authorization because `stored_admin.require_auth()` is
    // enforced as soon as the initial admin is stored.
    let env = Env::default();
    let contract_id = env.register(VaultRegistry, ());
    let client = VaultRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    let bootstrap_invoke = MockAuthInvoke {
        contract: &client.address,
        fn_name: "nominate_new_admin",
        args: (admin.clone(),).into_val(&env),
        sub_invokes: &[],
    };
    client
        .mock_auths(&[MockAuth {
            address: &admin,
            invoke: &bootstrap_invoke,
        }])
        .nominate_new_admin(&admin);
    assert_eq!(client.admin(), Some(admin));

    let replay_invoke = MockAuthInvoke {
        contract: &client.address,
        fn_name: "nominate_new_admin",
        args: (attacker.clone(),).into_val(&env),
        sub_invokes: &[],
    };
    client
        .mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &replay_invoke,
        }])
        .nominate_new_admin(&attacker);
}

#[test]
fn bootstrap_dual_admin_requires_both_auths_and_emits_rotation_events() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);

    client.bootstrap_dual_admin(&admin, &recovery);

    let signers: std::vec::Vec<Address> = env.auths().into_iter().map(|(a, _)| a).collect();
    assert!(signers.contains(&admin), "admin must authorize dual bootstrap");
    assert!(
        signers.contains(&recovery),
        "recovery_admin must authorize dual bootstrap"
    );

    let all = env.events().all();
    assert_eq!(all.len(), 2);
    let (_c0, topics0, _d0) = all.get_unchecked(0);
    let (_c1, topics1, _d1) = all.get_unchecked(1);
    let t0: Symbol = Symbol::try_from_val(&env, &topics0.get(0).unwrap()).unwrap();
    let t1: Symbol = Symbol::try_from_val(&env, &topics1.get(0).unwrap()).unwrap();
    assert_eq!(t0, symbol_short!("setadmin"));
    assert_eq!(t1, symbol_short!("setrecov"));
}
