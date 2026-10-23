// ── Partial refund accounting and remaining balance tests (#36) ─────────────
//
// Verifies:
// 1. Multiple partial refunds against a single payment receipt accumulate correctly.
// 2. Cumulative refunds cannot exceed the total payment amount (ceiling invariant).
// 3. Duplicate refund IDs with identical parameters are idempotent (Ok(())),
//    preserving cumulative state without double counting.
// 4. Duplicate refund IDs with conflicting parameters are rejected (ReceiptAlreadyExists).
// 5. Emitted refund events carry the exact remaining refundable balance.
// 6. get_remaining_refundable accurately reflects remaining balance at every step.
// 7. Refund window expiration applies to creator refunds while admin override can
//    still refund within the remaining ceiling.

#[test]
fn partial_refunds_track_remaining_balance_and_enforce_ceiling() {
    let (env, creator, _admin, settler, client) = setup_with_settler();
    let id = register_default(&env, &creator, &client, "pref01");
    let payer = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_pref01");
    let total_payment = 1_000_000i128;

    client.record_payment(
        &settler,
        &payment_id,
        &id,
        &payer,
        &total_payment,
        &String::from_str(&env, "tx_pay01"),
    );
    client.settle_payment(&settler, &payment_id);

    // Initial remaining refundable balance equals total payment
    assert_eq!(
        client.get_remaining_refundable(&payment_id),
        total_payment
    );
    assert_eq!(client.get_refunded_amount(&payment_id), 0);

    // First partial refund: 300_000 stroops
    let ref1_id = String::from_str(&env, "ref01_a");
    client.record_creator_refund(
        &creator,
        &ref1_id,
        &payment_id,
        &payer,
        &300_000i128,
        &String::from_str(&env, "tx_ref01_a"),
    );

    let ref1 = client.get_refund(&ref1_id);
    assert_eq!(ref1.amount, 300_000i128);
    assert_eq!(ref1.remaining_balance, 700_000i128);
    assert_eq!(client.get_refunded_amount(&payment_id), 300_000i128);
    assert_eq!(client.get_remaining_refundable(&payment_id), 700_000i128);

    // Check event payload exposes remaining balance
    let events = env.events().all();
    let (_cid, topics, data) = events.get_unchecked(events.len() - 1);
    let topic: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, symbol_short!("refund"));
    let ev_refund = RefundReceipt::try_from_val(&env, &data).unwrap();
    assert_eq!(ev_refund.refund_id, ref1_id);
    assert_eq!(ev_refund.amount, 300_000i128);
    assert_eq!(ev_refund.remaining_balance, 700_000i128);

    // Second partial refund: 500_000 stroops
    let ref2_id = String::from_str(&env, "ref01_b");
    client.record_creator_refund(
        &creator,
        &ref2_id,
        &payment_id,
        &payer,
        &500_000i128,
        &String::from_str(&env, "tx_ref01_b"),
    );

    let ref2 = client.get_refund(&ref2_id);
    assert_eq!(ref2.amount, 500_000i128);
    assert_eq!(ref2.remaining_balance, 200_000i128);
    assert_eq!(client.get_refunded_amount(&payment_id), 800_000i128);
    assert_eq!(client.get_remaining_refundable(&payment_id), 200_000i128);

    // Attempt partial refund exceeding remaining balance: 200_001 stroops fails
    let ref3_fail_id = String::from_str(&env, "ref01_c_fail");
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &ref3_fail_id,
            &payment_id,
            &payer,
            &200_001i128,
            &String::from_str(&env, "tx_ref01_c_fail"),
        ),
        Err(Ok(Error::PaymentAmountMismatch))
    );
    // Remaining balance unchanged after failure
    assert_eq!(client.get_remaining_refundable(&payment_id), 200_000i128);

    // Third partial refund: exact remaining 200_000 stroops
    let ref3_id = String::from_str(&env, "ref01_c");
    client.record_creator_refund(
        &creator,
        &ref3_id,
        &payment_id,
        &payer,
        &200_000i128,
        &String::from_str(&env, "tx_ref01_c"),
    );

    let ref3 = client.get_refund(&ref3_id);
    assert_eq!(ref3.amount, 200_000i128);
    assert_eq!(ref3.remaining_balance, 0i128);
    assert_eq!(client.get_refunded_amount(&payment_id), total_payment);
    assert_eq!(client.get_remaining_refundable(&payment_id), 0i128);

    // Further refund attempt fails
    let ref4_fail_id = String::from_str(&env, "ref01_d");
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &ref4_fail_id,
            &payment_id,
            &payer,
            &1i128,
            &String::from_str(&env, "tx_ref01_d"),
        ),
        Err(Ok(Error::PaymentAmountMismatch))
    );
}

#[test]
fn partial_refund_duplicate_id_idempotency_and_rejection() {
    let (env, creator, _admin, settler, client) = setup_with_settler();
    let id = register_default(&env, &creator, &client, "pref02");
    let payer = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_pref02");

    client.record_payment(
        &settler,
        &payment_id,
        &id,
        &payer,
        &500_000i128,
        &String::from_str(&env, "tx_pay02"),
    );
    client.settle_payment(&settler, &payment_id);

    let refund_id = String::from_str(&env, "ref_idempotent");
    let tx_hash = String::from_str(&env, "tx_ref_idem");

    // First recording succeeds
    client.record_creator_refund(
        &creator,
        &refund_id,
        &payment_id,
        &payer,
        &150_000i128,
        &tx_hash,
    );
    assert_eq!(client.get_refunded_amount(&payment_id), 150_000i128);
    assert_eq!(client.get_remaining_refundable(&payment_id), 350_000i128);

    // Exact duplicate retry: idempotent success without double counting
    client.record_creator_refund(
        &creator,
        &refund_id,
        &payment_id,
        &payer,
        &150_000i128,
        &tx_hash,
    );
    assert_eq!(client.get_refunded_amount(&payment_id), 150_000i128);
    assert_eq!(client.get_remaining_refundable(&payment_id), 350_000i128);

    // Duplicate refund ID with conflicting amount: rejected with ReceiptAlreadyExists
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &refund_id,
            &payment_id,
            &payer,
            &200_000i128,
            &tx_hash,
        ),
        Err(Ok(Error::ReceiptAlreadyExists))
    );

    // Duplicate refund ID with conflicting tx_hash: rejected with ReceiptAlreadyExists
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &refund_id,
            &payment_id,
            &payer,
            &150_000i128,
            &String::from_str(&env, "tx_different"),
        ),
        Err(Ok(Error::ReceiptAlreadyExists))
    );

    // New refund ID reusing the same tx_hash: rejected with DuplicateTxHash
    let new_refund_id = String::from_str(&env, "ref_new_duptx");
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &new_refund_id,
            &payment_id,
            &payer,
            &50_000i128,
            &tx_hash,
        ),
        Err(Ok(Error::DuplicateTxHash))
    );

    // Refunded totals remain intact
    assert_eq!(client.get_refunded_amount(&payment_id), 150_000i128);
    assert_eq!(client.get_remaining_refundable(&payment_id), 350_000i128);
}

#[test]
fn partial_refund_window_and_admin_override() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let id = register_default(&env, &creator, &client, "pref03");
    let payer = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_pref03");

    client.set_refund_window(&admin, &50u32);
    client.record_payment(
        &settler,
        &payment_id,
        &id,
        &payer,
        &1_000_000i128,
        &String::from_str(&env, "tx_pay03"),
    );
    client.settle_payment(&settler, &payment_id);
    let deadline = client.get_refund_deadline(&payment_id);

    // Creator partial refund before deadline
    client.record_creator_refund(
        &creator,
        &String::from_str(&env, "ref03_early"),
        &payment_id,
        &payer,
        &400_000i128,
        &String::from_str(&env, "tx_ref03_early"),
    );
    assert_eq!(client.get_remaining_refundable(&payment_id), 600_000i128);

    // Fast-forward past deadline
    env.ledger().set_sequence_number(deadline);

    // Creator refund after deadline fails
    assert_eq!(
        client.try_record_creator_refund(
            &creator,
            &String::from_str(&env, "ref03_late"),
            &payment_id,
            &payer,
            &100_000i128,
            &String::from_str(&env, "tx_ref03_late"),
        ),
        Err(Ok(Error::InvalidPaymentTransition))
    );

    // Admin override can still refund within remaining balance after deadline
    client.record_admin_refund(
        &admin,
        &String::from_str(&env, "ref03_admin"),
        &payment_id,
        &payer,
        &600_000i128,
        &String::from_str(&env, "tx_ref03_admin"),
    );
    let admin_ref = client.get_refund(&String::from_str(&env, "ref03_admin"));
    assert_eq!(admin_ref.remaining_balance, 0i128);
    assert_eq!(client.get_remaining_refundable(&payment_id), 0i128);

    // Admin override cannot exceed ceiling
    assert_eq!(
        client.try_record_admin_refund(
            &admin,
            &String::from_str(&env, "ref03_admin_excess"),
            &payment_id,
            &payer,
            &1i128,
            &String::from_str(&env, "tx_ref03_admin_excess"),
        ),
        Err(Ok(Error::PaymentAmountMismatch))
    );
}


#[test]
fn creator_refund_with_optional_reason_hash() {
    let (env, creator, _admin, settler, client) = setup_with_settler();
    let id = register_default(&env, &creator, &client, "pref_rsn01");
    let payer = Address::generate(&env);
    let payment_id = String::from_str(&env, "pay_pref_rsn01");
    let total_payment = 500_000i128;

    client.record_payment(
        &settler,
        &payment_id,
        &id,
        &payer,
        &total_payment,
        &String::from_str(&env, "tx_pay_rsn01"),
    );
    client.settle_payment(&settler, &payment_id);

    // 1. Successful refund with reason hash
    let ref1_id = String::from_str(&env, "ref_rsn_a");
    let reason_hash = String::from_str(&env, "sha256_audit_dispute_reason_hash_example");
    client.record_creator_refund_with_reason(
        &creator,
        &ref1_id,
        &payment_id,
        &payer,
        &200_000i128,
        &String::from_str(&env, "tx_ref_rsn_a"),
        &Some(reason_hash.clone()),
    );

    let ref1 = client.get_refund(&ref1_id);
    assert_eq!(ref1.reason_hash, Some(reason_hash.clone()));
    assert_eq!(client.get_refund_reason_hash(&ref1_id), Some(reason_hash));

    // 2. Refund with None reason hash
    let ref2_id = String::from_str(&env, "ref_rsn_b");
    client.record_creator_refund_with_reason(
        &creator,
        &ref2_id,
        &payment_id,
        &payer,
        &100_000i128,
        &String::from_str(&env, "tx_ref_rsn_b"),
        &None,
    );
    let ref2 = client.get_refund(&ref2_id);
    assert_eq!(ref2.reason_hash, None);
    assert_eq!(client.get_refund_reason_hash(&ref2_id), None);

    // 3. Reason hash exceeding MAX_REFUND_REASON_HASH_LEN (64 bytes) is rejected
    let too_long_hash = String::from_str(&env, "12345678901234567890123456789012345678901234567890123456789012345"); // 65 chars
    let ref3_id = String::from_str(&env, "ref_rsn_c");
    let res = client.try_record_creator_refund_with_reason(
        &creator,
        &ref3_id,
        &payment_id,
        &payer,
        &50_000i128,
        &String::from_str(&env, "tx_ref_rsn_c"),
        &Some(too_long_hash),
    );
    assert_eq!(res, Err(Ok(Error::RefundReasonHashTooLong)));
}