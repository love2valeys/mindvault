// ── Scheduled maintenance pause-until windows (#235) ─────────────────────────
//
// `set_paused_until(admin, pause_until)` schedules a time-bounded pause until
// an absolute Unix ledger timestamp. Writes are blocked while
// `ledger.timestamp() < pause_until` and automatically resume once
// `ledger.timestamp() >= pause_until`.

#[test]
fn set_paused_until_requires_admin_and_rejects_before_bootstrap() {
    let (env, _creator, client) = setup();
    let stranger = Address::generate(&env);
    env.ledger().set_timestamp(100);

    // Before admin bootstrap -> AdminNotSet.
    assert_eq!(
        client.try_set_paused_until(&stranger, &200),
        Err(Ok(Error::AdminNotSet))
    );

    let admin = Address::generate(&env);
    client.nominate_new_admin(&admin);

    // Non-admin caller -> Unauthorized.
    assert_eq!(
        client.try_set_paused_until(&stranger, &200),
        Err(Ok(Error::Unauthorized))
    );

    // Admin without auth -> rejected without changing state.
    env.mock_auths(&[]);
    assert!(client.try_set_paused_until(&admin, &200).is_err());

    env.mock_all_auths();
    assert!(!client.is_paused());
    assert_eq!(client.pause_until(), None);
}

#[test]
fn scheduled_pause_blocks_multiple_write_paths_and_auto_resumes_at_deadline() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let id = register_default(&env, &creator, &client, "maintwin1");
    let payer = Address::generate(&env);
    let holder = Address::generate(&env);
    let lease_amount = client.lease_price(&id, &LeaseTier::Hour);

    env.ledger().set_timestamp(1_000);
    client.set_paused_until(&admin, &1_600);

    // Just before expiry (1_599 < 1_600): all writes are blocked while reads work.
    env.ledger().set_timestamp(1_599);
    assert!(client.is_paused());
    assert_eq!(client.pause_until(), Some(1_600));
    assert_eq!(client.get(&id).id, id);

    assert_eq!(
        client.try_set_price(&id, &200i128),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_set_verification_status(
            &id,
            &verifier,
            &VerificationStatus::Verified,
            &None::<String>,
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_payment(
            &settler,
            &String::from_str(&env, "maintpay1"),
            &id,
            &payer,
            &100i128,
            &String::from_str(&env, "maint-tx-1"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_buy_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &lease_amount,
            &String::from_str(&env, "maint-lease-1"),
        ),
        Err(Ok(Error::ContractPaused))
    );

    // Exactly at deadline (1_600 >= 1_600): automatically resumed without another tx.
    env.ledger().set_timestamp(1_600);
    assert!(!client.is_paused());
    assert_eq!(client.pause_until(), None);

    client.set_price(&id, &200i128);
    assert_eq!(client.get(&id).price, 200i128);

    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Verified,
        &None::<String>,
    );
    assert_eq!(client.get(&id).verified, VerificationStatus::Verified);

    client.record_payment(
        &settler,
        &String::from_str(&env, "maintpay1"),
        &id,
        &payer,
        &200i128,
        &String::from_str(&env, "maint-tx-1"),
    );
    let updated_lease_amount = client.lease_price(&id, &LeaseTier::Hour);
    client.buy_lease(
        &holder,
        &id,
        &LeaseTier::Hour,
        &updated_lease_amount,
        &String::from_str(&env, "maint-lease-1"),
    );
}

#[test]
fn scheduled_pause_can_be_extended_shortened_and_converted_from_indefinite_pause() {
    let (env, _creator, admin, client) = setup_with_admin();
    env.ledger().set_timestamp(100);

    // Start with an indefinite pause, then convert to a scheduled window.
    client.set_paused(&admin, &true);
    assert!(client.is_paused());
    assert_eq!(client.pause_until(), None);

    client.set_paused_until(&admin, &200);
    assert!(client.is_paused());
    assert_eq!(client.pause_until(), Some(200));

    // Extend the maintenance window.
    client.set_paused_until(&admin, &350);
    assert_eq!(client.pause_until(), Some(350));

    // Shorten the maintenance window.
    client.set_paused_until(&admin, &180);
    assert_eq!(client.pause_until(), Some(180));

    env.ledger().set_timestamp(180);
    assert!(!client.is_paused());
    assert_eq!(client.pause_until(), None);
}

#[test]
fn expired_scheduled_pause_cleans_stale_storage_on_first_write() {
    let (env, creator, admin, client) = setup_with_admin();
    env.ledger().set_timestamp(100);
    client.set_paused_until(&admin, &200);

    // Advance past the deadline so the window auto-resumes.
    env.ledger().set_timestamp(200);
    assert!(!client.is_paused());
    assert_eq!(client.pause_until(), None);

    // The first write after expiry lazily clears the stale Paused/PauseUntil entries.
    let _id = register_default(&env, &creator, &client, "maintclean1");
    let (stored_paused, stored_deadline): (Option<bool>, Option<u64>) =
        env.as_contract(&client.address, || {
            (
                env.storage().instance().get(&DataKey::Paused),
                env.storage().instance().get(&DataKey::PauseUntil),
            )
        });
    assert_eq!(stored_paused, Some(false));
    assert_eq!(stored_deadline, None);
}

#[test]
fn pause_until_returns_none_when_paused_flag_is_false() {
    let (env, _creator, _admin, client) = setup_with_admin();
    env.ledger().set_timestamp(100);

    // Even if a stray PauseUntil existed while Paused is false, pause_until()
    // must remain consistent with is_paused() == false.
    env.as_contract(&client.address, || {
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::PauseUntil, &500u64);
    });

    assert!(!client.is_paused());
    assert_eq!(client.pause_until(), None);
}

#[test]
fn scheduled_pause_auto_resume_keeps_independent_payments_pause_active() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let id = register_default(&env, &creator, &client, "maintpayind1");
    let payer = Address::generate(&env);

    env.ledger().set_timestamp(100);
    client.set_payments_paused(&admin, &true);
    client.set_paused_until(&admin, &200);

    // After global scheduled window expires, catalog writes resume while
    // payment writes remain blocked by the independent payment-scope pause.
    env.ledger().set_timestamp(200);
    assert!(!client.is_paused());
    assert!(client.is_payments_paused());

    client.set_price(&id, &150i128);
    assert_eq!(
        client.try_record_payment(
            &settler,
            &String::from_str(&env, "maintpayindr1"),
            &id,
            &payer,
            &150i128,
            &String::from_str(&env, "maint-pay-ind-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
}
