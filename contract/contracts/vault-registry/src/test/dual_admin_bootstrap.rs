// ─── 2-of-2 dual-key admin bootstrap & recovery transfer (#242) ──────────────
//
// Deployments that need recovery if a sole admin key is lost or misconfigured
// can bootstrap with `bootstrap_dual_admin(admin, recovery_admin)` (2-of-2
// authorization) and later transfer admin authority via `recover_admin`, while
// preserving backward compatibility with single-key `nominate_new_admin`.

fn build_bootstrap_dual_admin_invoke<'a>(
    env: &Env,
    client: &'a VaultRegistryClient<'a>,
    admin: &Address,
    recovery_admin: &Address,
) -> MockAuthInvoke<'a> {
    MockAuthInvoke {
        contract: &client.address,
        fn_name: "bootstrap_dual_admin",
        args: (admin.clone(), recovery_admin.clone()).into_val(env),
        sub_invokes: &[],
    }
}

fn build_recover_admin_invoke<'a>(
    env: &Env,
    client: &'a VaultRegistryClient<'a>,
    new_admin: &Address,
) -> MockAuthInvoke<'a> {
    MockAuthInvoke {
        contract: &client.address,
        fn_name: "recover_admin",
        args: (new_admin.clone(),).into_val(env),
        sub_invokes: &[],
    }
}

#[test]
fn bootstrap_dual_admin_sets_admin_and_recovery_and_emits_rotation_events() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);

    client.bootstrap_dual_admin(&admin, &recovery);

    assert_eq!(client.admin(), Some(admin.clone()));
    assert_eq!(client.recovery_admin(), Some(recovery.clone()));

    let all = env.events().all();
    assert_eq!(
        all.len(),
        2,
        "bootstrap_dual_admin must emit setadmin and setrecov"
    );

    let (_, topics0, data0) = all.get_unchecked(0);
    let sym0: Symbol = Symbol::try_from_val(&env, &topics0.get(0).unwrap()).unwrap();
    assert_eq!(sym0, symbol_short!("setadmin"));
    assert_eq!(
        RoleRotation::try_from_val(&env, &data0).unwrap(),
        RoleRotation {
            actor: admin.clone(),
            previous: None,
            replacement: Some(admin.clone()),
        }
    );

    let (_, topics1, data1) = all.get_unchecked(1);
    let sym1: Symbol = Symbol::try_from_val(&env, &topics1.get(0).unwrap()).unwrap();
    assert_eq!(sym1, symbol_short!("setrecov"));
    assert_eq!(
        RoleRotation::try_from_val(&env, &data1).unwrap(),
        RoleRotation {
            actor: admin,
            previous: None,
            replacement: Some(recovery),
        }
    );
}

#[test]
fn bootstrap_dual_admin_requires_2_of_2_authorization() {
    let env = Env::default();
    let contract_id = env.register(VaultRegistry, ());
    let client = VaultRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);

    // No auths at all -> rejected.
    assert!(client
        .mock_auths(&[])
        .try_bootstrap_dual_admin(&admin, &recovery)
        .is_err());

    // Only primary admin signed -> rejected (must be 2-of-2).
    let invoke_admin_only = build_bootstrap_dual_admin_invoke(&env, &client, &admin, &recovery);
    let auth_admin_only = [MockAuth {
        address: &admin,
        invoke: &invoke_admin_only,
    }];
    assert!(client
        .mock_auths(&auth_admin_only)
        .try_bootstrap_dual_admin(&admin, &recovery)
        .is_err());

    // Only recovery admin signed -> rejected (must be 2-of-2).
    let invoke_recovery_only = build_bootstrap_dual_admin_invoke(&env, &client, &admin, &recovery);
    let auth_recovery_only = [MockAuth {
        address: &recovery,
        invoke: &invoke_recovery_only,
    }];
    assert!(client
        .mock_auths(&auth_recovery_only)
        .try_bootstrap_dual_admin(&admin, &recovery)
        .is_err());

    // Both admin and recovery_admin sign -> succeeds.
    let invoke_both = build_bootstrap_dual_admin_invoke(&env, &client, &admin, &recovery);
    let auth_both = [
        MockAuth {
            address: &admin,
            invoke: &invoke_both,
        },
        MockAuth {
            address: &recovery,
            invoke: &invoke_both,
        },
    ];
    client
        .mock_auths(&auth_both)
        .bootstrap_dual_admin(&admin, &recovery);

    assert_eq!(client.admin(), Some(admin));
    assert_eq!(client.recovery_admin(), Some(recovery));
}

#[test]
fn bootstrap_dual_admin_rejects_identical_keys_without_mutating_state() {
    let (env, _creator, client) = setup();
    let same = Address::generate(&env);

    assert_eq!(
        client.try_bootstrap_dual_admin(&same, &same),
        Err(Ok(Error::SameAdmin))
    );
    assert_eq!(client.admin(), None);
    assert_eq!(client.recovery_admin(), None);
}

#[test]
fn bootstrap_dual_admin_rejects_after_single_or_dual_bootstrap() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);
    let other_admin = Address::generate(&env);
    let other_recovery = Address::generate(&env);

    // Single-key bootstrap first -> dual bootstrap must be rejected.
    client.nominate_new_admin(&admin);
    assert_eq!(
        client.try_bootstrap_dual_admin(&other_admin, &other_recovery),
        Err(Ok(Error::AdminAlreadySet))
    );
    assert_eq!(client.admin(), Some(admin));
    assert_eq!(client.recovery_admin(), None);

    // Separate deployment with dual bootstrap -> second dual bootstrap rejected.
    let (env2, _creator2, client2) = setup();
    let a2 = Address::generate(&env2);
    let r2 = Address::generate(&env2);
    client2.bootstrap_dual_admin(&a2, &r2);
    assert_eq!(
        client2.try_bootstrap_dual_admin(&Address::generate(&env2), &Address::generate(&env2)),
        Err(Ok(Error::AdminAlreadySet))
    );
    assert_eq!(client2.admin(), Some(a2));
    assert_eq!(client2.recovery_admin(), Some(r2));
}

#[test]
fn dual_bootstrap_recovery_transfers_admin_and_clears_all_pending_nomination_keys() {
    let (env, _creator, client) = setup();
    let lost_admin = Address::generate(&env);
    let recovery = Address::generate(&env);
    let pending = Address::generate(&env);
    let replacement = Address::generate(&env);

    client.bootstrap_dual_admin(&lost_admin, &recovery);
    client.nominate_new_admin(&pending);
    assert_eq!(client.pending_admin(), Some(pending.clone()));
    assert!(client.pending_admin_expiry().is_some());

    // Recovery admin transfers admin authority immediately.
    client.recover_admin(&replacement);

    assert_eq!(client.admin(), Some(replacement.clone()));
    assert_eq!(client.recovery_admin(), Some(recovery));
    assert_eq!(client.pending_admin(), None);
    assert_eq!(client.pending_admin_expiry(), None);

    // Verify stale PendingAdminNominatedAt was also removed from instance storage.
    let has_nominated_at = env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .has(&DataKey::PendingAdminNominatedAt)
    });
    assert!(
        !has_nominated_at,
        "recover_admin must remove DataKey::PendingAdminNominatedAt alongside PendingAdmin"
    );

    // The cancelled pending nominee can no longer accept.
    env.ledger()
        .set_sequence_number(env.ledger().sequence() + ADMIN_NOMINATION_MIN_GAP);
    assert_eq!(
        client.try_accept_admin(&pending),
        Err(Ok(Error::AdminNominationExpired))
    );

    // Replacement admin now holds full governance authority; lost admin does not.
    assert_eq!(
        client.try_set_paused(&lost_admin, &true),
        Err(Ok(Error::Unauthorized))
    );
    client.set_paused(&replacement, &true);
    assert!(client.is_paused());
}

#[test]
fn dual_bootstrap_recovery_requires_recovery_auth_and_works_while_paused() {
    let env = Env::default();
    let contract_id = env.register(VaultRegistry, ());
    let client = VaultRegistryClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);
    let replacement = Address::generate(&env);

    client
        .mock_all_auths()
        .bootstrap_dual_admin(&admin, &recovery);
    client.mock_all_auths().set_paused(&admin, &true);
    assert!(client.is_paused());

    // Stranger / old admin cannot authorize recovery.
    let invoke = build_recover_admin_invoke(&env, &client, &replacement);
    let wrong_auth = [MockAuth {
        address: &admin,
        invoke: &invoke,
    }];
    assert!(client
        .mock_auths(&wrong_auth)
        .try_recover_admin(&replacement)
        .is_err());

    // Recovery admin succeeds even while registry mutations are paused.
    let recovery_auth = [MockAuth {
        address: &recovery,
        invoke: &invoke,
    }];
    client
        .mock_auths(&recovery_auth)
        .recover_admin(&replacement);

    assert_eq!(client.admin(), Some(replacement));
    assert_eq!(client.recovery_admin(), Some(recovery));
}

#[test]
fn single_admin_bootstrap_rejects_recover_admin_without_recovery_key() {
    let (env, _creator, admin, client) = setup_with_admin();
    let replacement = Address::generate(&env);

    assert_eq!(client.admin(), Some(admin.clone()));
    assert_eq!(client.recovery_admin(), None);
    assert_eq!(
        client.try_recover_admin(&replacement),
        Err(Ok(Error::RecoveryAdminNotSet))
    );
    assert_eq!(client.admin(), Some(admin));
}

#[test]
fn dual_admin_role_separation_prevents_collapsing_admin_and_recovery_keys() {
    let (env, _creator, client) = setup();
    let admin = Address::generate(&env);
    let recovery = Address::generate(&env);
    let pending = Address::generate(&env);

    client.bootstrap_dual_admin(&admin, &recovery);

    // Nominating the active recovery_admin as pending admin is rejected.
    assert_eq!(
        client.try_nominate_new_admin(&recovery),
        Err(Ok(Error::SameAdmin))
    );
    assert_eq!(client.pending_admin(), None);

    // Nominating a distinct pending admin succeeds, and setting that live
    // pending admin as recovery_admin is rejected.
    client.nominate_new_admin(&pending);
    assert_eq!(
        client.try_set_recovery_admin(&Some(pending.clone())),
        Err(Ok(Error::SameAdmin))
    );
    assert_eq!(client.recovery_admin(), Some(recovery));

    // Once the nomination expires, setting that address as recovery_admin works.
    let expiry = client.pending_admin_expiry().unwrap();
    env.ledger().set_sequence_number(expiry);
    client.set_recovery_admin(&Some(pending.clone()));
    assert_eq!(client.recovery_admin(), Some(pending));
}
