// ── Explicit events for receipt anchor failures (#290) ───────────────────────
//
// Covers the structured `anchrfail` event emitted across both the verifier
// reporting path (`attempt_anchor_purchase_receipt`) and the settler recovery
// paths (`record_anchor_failure`, `retry_anchor_purchase_receipt`), plus
// `get_anchor_failure` and `get_anchor_status` consistency.

fn setup_with_settler_and_verifier<'a>() -> (
    Env,
    Address,
    Address,
    Address,
    Address,
    VaultRegistryClient<'a>,
) {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);
    (env, creator, admin, settler, verifier, client)
}

#[test]
fn record_anchor_failure_persists_emits_anchrfail_and_updates_status() {
    let (env, creator, _admin, settler, _verifier, client) = setup_with_settler_and_verifier();
    let id = register_default(&env, &creator, &client, "anchf1");
    let buyer = Address::generate(&env);
    let hash_one = String::from_str(&env, "tx-anchor-fail-1");

    env.ledger().set_sequence_number(250);
    client.record_anchor_failure(&settler, &id, &buyer, &hash_one);

    // The explicit `anchrfail` event must carry the resource id topic and
    // structured AnchorFailure payload with VerifierUnavailable reason.
    let emitted = last_anchor_failure(&env, &id);
    assert_eq!(emitted.resource_id, id);
    assert_eq!(emitted.buyer, buyer);
    assert_eq!(emitted.receipt_hash, hash_one);
    assert_eq!(emitted.reason, AnchorFailureReason::VerifierUnavailable);
    assert_eq!(emitted.ledger, 250);

    // Durable failure record and derived lifecycle status agree with the event.
    assert_eq!(client.get_anchor_failure(&id, &buyer), emitted);
    assert_eq!(
        client.get_anchor_status(&id, &buyer),
        Some(AnchorStatus::Failed)
    );

    // Last-write-wins: recording a newer failure overwrites the record and
    // emits a fresh `anchrfail` event at the updated ledger.
    let hash_two = String::from_str(&env, "tx-anchor-fail-2");
    env.ledger().set_sequence_number(310);
    client.record_anchor_failure(&settler, &id, &buyer, &hash_two);

    let updated = last_anchor_failure(&env, &id);
    assert_eq!(updated.receipt_hash, hash_two);
    assert_eq!(updated.reason, AnchorFailureReason::VerifierUnavailable);
    assert_eq!(updated.ledger, 310);
    assert_eq!(client.get_anchor_failure(&id, &buyer), updated);
}

#[test]
fn record_anchor_failure_validates_role_resource_and_hash() {
    let (env, creator, _admin, settler, _verifier, client) = setup_with_settler_and_verifier();
    let id = register_default(&env, &creator, &client, "anchfval");
    let buyer = Address::generate(&env);
    let stranger = Address::generate(&env);
    let valid_hash = String::from_str(&env, "tx-valid-hash");

    // Non-settler reverts and cannot forge an `anchrfail` event.
    assert_eq!(
        client.try_record_anchor_failure(&stranger, &id, &buyer, &valid_hash),
        Err(Ok(Error::NotSettler))
    );

    // Unknown resource reverts.
    assert_eq!(
        client.try_record_anchor_failure(
            &settler,
            &String::from_str(&env, "anchfmissing"),
            &buyer,
            &valid_hash,
        ),
        Err(Ok(Error::NotFound))
    );

    // Empty and oversized receipt hashes revert with InvalidTxHash.
    assert_eq!(
        client.try_record_anchor_failure(&settler, &id, &buyer, &String::from_str(&env, "")),
        Err(Ok(Error::InvalidTxHash))
    );
    let too_long = String::from_str(&env, &"x".repeat(MAX_TX_HASH_LEN as usize + 1));
    assert_eq!(
        client.try_record_anchor_failure(&settler, &id, &buyer, &too_long),
        Err(Ok(Error::InvalidTxHash))
    );

    assert_eq!(
        client.try_get_anchor_failure(&id, &buyer),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(client.get_anchor_status(&id, &buyer), None);
}

#[test]
fn record_anchor_failure_clears_stale_failure_when_already_anchored() {
    let (env, creator, _admin, settler, verifier, client) = setup_with_settler_and_verifier();
    let id = register_default(&env, &creator, &client, "anchfclr");
    let buyer = Address::generate(&env);
    let hash = String::from_str(&env, "tx-anchored-1");

    // Record a failure first, then confirm the anchor via verifier.
    client.record_anchor_failure(&settler, &id, &buyer, &hash);
    assert_eq!(
        client.get_anchor_status(&id, &buyer),
        Some(AnchorStatus::Failed)
    );

    client.anchor_purchase_receipt(&verifier, &id, &buyer, &hash);
    assert_eq!(
        client.get_anchor_status(&id, &buyer),
        Some(AnchorStatus::Confirmed)
    );
    assert_eq!(
        client.try_get_anchor_failure(&id, &buyer),
        Err(Ok(Error::NotFound))
    );

    // Calling record_anchor_failure after the anchor exists is a clean no-op
    // that emits no `anchrfail` event and keeps the pair Confirmed.
    client.record_anchor_failure(&settler, &id, &buyer, &hash);
    assert_eq!(env.events().all().len(), 0);
    assert_eq!(
        client.try_get_anchor_failure(&id, &buyer),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.get_anchor_status(&id, &buyer),
        Some(AnchorStatus::Confirmed)
    );
}

#[test]
fn retry_anchor_purchase_receipt_emits_anchrfail_for_settler_reasons() {
    let (env, creator, _admin, settler, verifier, client) = setup_with_settler_and_verifier();
    let id = register_default(&env, &creator, &client, "anchrtry");
    let buyer = Address::generate(&env);
    let paid_tx = String::from_str(&env, "tx-paid-retry-1");

    // 1. No payment recorded yet -> Ok(false) + anchrfail(NoPaymentRecorded).
    env.ledger().set_sequence_number(400);
    assert!(!client.retry_anchor_purchase_receipt(&settler, &id, &buyer, &paid_tx));
    let no_pay = last_anchor_failure(&env, &id);
    assert_eq!(no_pay.reason, AnchorFailureReason::NoPaymentRecorded);
    assert_eq!(no_pay.buyer, buyer);
    assert_eq!(no_pay.receipt_hash, paid_tx);
    assert_eq!(no_pay.ledger, 400);
    assert_eq!(client.get_anchor_failure(&id, &buyer), no_pay);
    assert_eq!(
        client.get_anchor_status(&id, &buyer),
        Some(AnchorStatus::Failed)
    );

    // Record the payment so subsequent retry checks run against `paid_tx`.
    client.record_payment(
        &settler,
        &String::from_str(&env, "rcpt-retry-1"),
        &id,
        &buyer,
        &100i128,
        &paid_tx,
    );

    // 2. Empty receipt hash -> Ok(false) + anchrfail(InvalidReceiptHash).
    assert!(!client.retry_anchor_purchase_receipt(
        &settler,
        &id,
        &buyer,
        &String::from_str(&env, "")
    ));
    assert_eq!(
        last_anchor_failure(&env, &id).reason,
        AnchorFailureReason::InvalidReceiptHash
    );

    // 3. Oversized receipt hash -> Ok(false) + anchrfail(InvalidReceiptHash).
    let too_long = String::from_str(&env, &"z".repeat(MAX_TX_HASH_LEN as usize + 1));
    assert!(!client.retry_anchor_purchase_receipt(&settler, &id, &buyer, &too_long));
    assert_eq!(
        last_anchor_failure(&env, &id).reason,
        AnchorFailureReason::InvalidReceiptHash
    );

    // 4. Hash that does not match the recorded payment's tx_hash ->
    // Ok(false) + anchrfail(ReceiptHashMismatch).
    let wrong_hash = String::from_str(&env, "tx-different-hash");
    assert!(!client.retry_anchor_purchase_receipt(&settler, &id, &buyer, &wrong_hash));
    let mismatch = last_anchor_failure(&env, &id);
    assert_eq!(mismatch.reason, AnchorFailureReason::ReceiptHashMismatch);
    assert_eq!(mismatch.receipt_hash, wrong_hash);

    // 5. Matching hash -> Ok(true), emits `anchor`, clears failure, sets Confirmed.
    assert!(client.retry_anchor_purchase_receipt(&settler, &id, &buyer, &paid_tx));
    let all = env.events().all();
    let (_cid, topics, _data) = all.get_unchecked(all.len() - 1);
    let topic0: Symbol = Symbol::try_from_val(&env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic0, symbol_short!("anchor"));
    assert_eq!(
        client.try_get_anchor_failure(&id, &buyer),
        Err(Ok(Error::NotFound))
    );
    assert_eq!(
        client.get_anchor_status(&id, &buyer),
        Some(AnchorStatus::Confirmed)
    );
    assert_eq!(client.get_purchase_receipt(&id, &buyer).receipt_hash, paid_tx);

    // 6. Blind retry with the same already-anchored hash converges idempotently.
    assert!(client.retry_anchor_purchase_receipt(&settler, &id, &buyer, &paid_tx));
    assert_eq!(env.events().all().len(), 0);

    // 7. Conflicting anchor already written with a different hash ->
    // Ok(false) + anchrfail(DuplicateReceipt), preserving the original anchor.
    let buyer_conflict = Address::generate(&env);
    let conflict_paid_tx = String::from_str(&env, "tx-conflict-paid");
    let conflict_existing_hash = String::from_str(&env, "tx-conflict-existing");
    client.record_payment(
        &settler,
        &String::from_str(&env, "rcpt-retry-conflict"),
        &id,
        &buyer_conflict,
        &100i128,
        &conflict_paid_tx,
    );
    client.anchor_purchase_receipt(&verifier, &id, &buyer_conflict, &conflict_existing_hash);

    assert!(!client.retry_anchor_purchase_receipt(
        &settler,
        &id,
        &buyer_conflict,
        &conflict_paid_tx
    ));
    let dup = last_anchor_failure(&env, &id);
    assert_eq!(dup.reason, AnchorFailureReason::DuplicateReceipt);
    assert_eq!(dup.buyer, buyer_conflict);
    assert_eq!(dup.receipt_hash, conflict_paid_tx);
    assert_eq!(
        client
            .get_purchase_receipt(&id, &buyer_conflict)
            .receipt_hash,
        conflict_existing_hash
    );
}

#[test]
fn retry_anchor_purchase_receipt_rejects_non_settler_without_event() {
    let (env, creator, _admin, _settler, _verifier, client) = setup_with_settler_and_verifier();
    let id = register_default(&env, &creator, &client, "anchrns");
    let buyer = Address::generate(&env);
    let stranger = Address::generate(&env);

    assert_eq!(
        client.try_retry_anchor_purchase_receipt(
            &stranger,
            &id,
            &buyer,
            &String::from_str(&env, "tx-unauth")
        ),
        Err(Ok(Error::NotSettler))
    );
    assert_eq!(client.get_anchor_status(&id, &buyer), None);
}

#[test]
fn anchor_failure_reason_as_error_covers_every_variant() {
    assert_eq!(
        AnchorFailureReason::ResourceNotFound.as_error(),
        Error::NotFound
    );
    assert_eq!(
        AnchorFailureReason::InvalidReceiptHash.as_error(),
        Error::InvalidTxHash
    );
    assert_eq!(
        AnchorFailureReason::DuplicateReceipt.as_error(),
        Error::DuplicateReceipt
    );
    assert_eq!(
        AnchorFailureReason::RetryTooSoon.as_error(),
        Error::InvalidPaymentTransition
    );
    assert_eq!(
        AnchorFailureReason::AttemptsExhausted.as_error(),
        Error::InvalidPaymentTransition
    );
    assert_eq!(
        AnchorFailureReason::VerifierUnavailable.as_error(),
        Error::NotVerifier
    );
    assert_eq!(
        AnchorFailureReason::NoPaymentRecorded.as_error(),
        Error::NotFound
    );
    assert_eq!(
        AnchorFailureReason::ReceiptHashMismatch.as_error(),
        Error::InvalidTxHash
    );
}
