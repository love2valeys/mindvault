#[test]
fn payment_scope_pause_blocks_every_payment_refund_and_anchor_write_without_state_changes() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let id = register_priced(&env, &creator, &client, "pausematrix", 100);
    let payer = Address::generate(&env);
    let service = Address::generate(&env);
    let existing_buyer = Address::generate(&env);
    let direct_buyer = Address::generate(&env);
    let attempt_buyer = Address::generate(&env);

    client.add_verifier(&service);
    let payment_id = String::from_str(&env, "matrixpayment");
    let payment_hash = String::from_str(&env, "matrix-payment-tx");
    client.record_payment(
        &settler,
        &payment_id,
        &id,
        &payer,
        &100i128,
        &payment_hash,
    );
    client.settle_payment(&settler, &payment_id);
    let original_payment = client.get_payment(&payment_id);

    let existing_anchor_hash = String::from_str(&env, "original-anchor");
    client.anchor_purchase_receipt(&service, &id, &existing_buyer, &existing_anchor_hash);
    let original_anchor = client.get_purchase_receipt(&id, &existing_buyer);

    client.set_payments_paused(&admin, &true);
    let event_count = env.events().all().len();

    assert_eq!(
        client.try_record_payment(
            &settler,
            &String::from_str(&env, "blockedpayment"),
            &id,
            &payer,
            &100i128,
            &String::from_str(&env, "blocked-payment-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_payment_idempotent(
            &settler,
            &payment_id,
            &id,
            &payer,
            &100i128,
            &payment_hash,
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_settle_payment(&settler, &payment_id),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &String::from_str(&env, "blockedrefund"),
            &payment_id,
            &payer,
            &10i128,
            &String::from_str(&env, "blocked-refund-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_admin_refund(
            &admin,
            &String::from_str(&env, "blockedadminref"),
            &payment_id,
            &payer,
            &10i128,
            &String::from_str(&env, "blocked-admin-refund-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_anchor_purchase_receipt(
            &service,
            &id,
            &direct_buyer,
            &String::from_str(&env, "blocked-direct-anchor"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_attempt_anchor_purchase_receipt(
            &service,
            &id,
            &attempt_buyer,
            &String::from_str(&env, "blocked-attempt-anchor"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_override_purchase_receipt_anchor(
            &admin,
            &id,
            &existing_buyer,
            &String::from_str(&env, "blocked-override"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_anchor_failure(
            &settler,
            &id,
            &direct_buyer,
            &String::from_str(&env, "blocked-failure"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_retry_anchor_purchase_receipt(
            &settler,
            &id,
            &payer,
            &payment_hash,
        ),
        Err(Ok(Error::ContractPaused))
    );

    assert_eq!(client.get_payment(&payment_id), original_payment);
    assert_eq!(client.get_refunded_amount(&payment_id), 0);
    assert_eq!(
        client.try_get_payment(&String::from_str(&env, "blockedpayment")),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.try_get_refund(&String::from_str(&env, "blockedrefund")),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.try_get_refund(&String::from_str(&env, "blockedadminref")),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.get_purchase_receipt(&id, &existing_buyer),
        original_anchor
    );
    assert_eq!(
        client.try_get_purchase_receipt(&id, &direct_buyer),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.try_get_purchase_receipt(&id, &attempt_buyer),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.get_anchor_attempts(&id, &attempt_buyer).attempts,
        0
    );
    assert_eq!(
        client.try_get_anchor_failure(&id, &direct_buyer),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(env.events().all().len(), event_count);
}

#[test]
fn payment_scope_pause_blocks_every_lease_write_without_state_changes() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let id = register_priced(&env, &creator, &client, "leasepausematrix", 100);
    let pending_holder = Address::generate(&env);
    let active_holder = Address::generate(&env);
    let buy_holder = Address::generate(&env);
    let record_holder = Address::generate(&env);

    client.buy_lease(
        &pending_holder,
        &id,
        &LeaseTier::Hour,
        &100i128,
        &String::from_str(&env, "pending-lease-tx"),
    );
    client.record_lease(
        &settler,
        &active_holder,
        &id,
        &LeaseTier::Hour,
        &100i128,
        &String::from_str(&env, "active-lease-tx"),
    );
    let pending_lease = client.get_lease(&id, &pending_holder);
    let active_lease = client.get_lease(&id, &active_holder);

    client.set_payments_paused(&admin, &true);
    let event_count = env.events().all().len();

    assert_eq!(
        client.try_buy_lease(
            &buy_holder,
            &id,
            &LeaseTier::Hour,
            &100i128,
            &String::from_str(&env, "blocked-buy-lease-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_lease(
            &settler,
            &record_holder,
            &id,
            &LeaseTier::Hour,
            &100i128,
            &String::from_str(&env, "blocked-record-lease-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_settle_lease(&settler, &id, &pending_holder),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_renew_lease(
            &active_holder,
            &id,
            &LeaseTier::Hour,
            &100i128,
            &String::from_str(&env, "blocked-renew-lease-tx"),
        ),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_revoke_lease(&id, &active_holder),
        Err(Ok(Error::ContractPaused))
    );

    assert_eq!(client.get_lease(&id, &pending_holder), pending_lease);
    assert_eq!(client.get_lease(&id, &active_holder), active_lease);
    assert_eq!(
        client.try_get_lease(&id, &buy_holder),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.try_get_lease(&id, &record_holder),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(env.events().all().len(), event_count);
}
