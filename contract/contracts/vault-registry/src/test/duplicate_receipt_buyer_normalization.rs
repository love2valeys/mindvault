// ── Duplicate receipt buyer normalization tests (#294) ───────────────────────
//
// Verifies payment receipt duplicate detection across both distinct buyers and
// canonical Stellar StrKey round-tripped buyer addresses: retrying an existing
// `receipt_id` with a different buyer is rejected, stores no second payment or
// secondary buyer index entry, and preserves the original buyer's receipt.

#[test]
fn duplicate_payment_receipt_id_with_different_buyer_rejected_and_preserves_original() {
    let (env, creator, _admin, settler, client) = setup_with_settler();
    let price = 1_000_000i128;
    let id = register_priced(&env, &creator, &client, "dupnorm1", price);
    let buyer_a = Address::generate(&env);
    let buyer_b = Address::generate(&env);
    let receipt_id = String::from_str(&env, "rcpt-norm-1");
    let tx_a = String::from_str(&env, "0xtxnorma");
    let tx_b = String::from_str(&env, "0xtxnormb");

    env.ledger().set_sequence_number(42);
    client.record_payment(&settler, &receipt_id, &id, &buyer_a, &price, &tx_a);

    // Retrying the same receipt_id under a different buyer (with either a fresh
    // tx_hash or the same tx_hash) must fail with ReceiptAlreadyExists.
    env.ledger().set_sequence_number(99);
    assert_eq!(
        client.try_record_payment(&settler, &receipt_id, &id, &buyer_b, &price, &tx_b),
        Err(Ok(Error::ReceiptAlreadyExists))
    );
    assert_eq!(
        client.try_record_payment(&settler, &receipt_id, &id, &buyer_b, &price, &tx_a),
        Err(Ok(Error::ReceiptAlreadyExists))
    );

    // Original receipt and buyer are preserved untouched.
    let stored = client.get_payment(&receipt_id);
    assert_eq!(stored.receipt_id, receipt_id);
    assert_eq!(stored.resource_id, id);
    assert_eq!(stored.payer, buyer_a);
    assert_eq!(stored.amount, price);
    assert_eq!(stored.tx_hash, tx_a);
    assert_eq!(stored.recorded_at, 42);
    assert_eq!(stored.ledger, 42);

    // No secondary index or buyer page entry was created for buyer_b, and no
    // duplicate entry was appended for buyer_a.
    assert_eq!(client.get_payment_receipt(&id, &buyer_a), stored);
    assert_eq!(
        client.try_get_payment_receipt(&id, &buyer_b),
        Err(Ok(Error::NotFound))
    );
    assert!(client
        .payment_receipt_page(&buyer_b, &None, &0, &20)
        .items
        .is_empty());
    assert!(client
        .payment_receipt_page(&buyer_b, &Some(id.clone()), &0, &20)
        .items
        .is_empty());
    assert_eq!(
        client
            .payment_receipt_page(&buyer_a, &None, &0, &20)
            .items
            .len(),
        1
    );
    assert_eq!(
        client
            .payment_receipt_page(&buyer_a, &Some(id.clone()), &0, &20)
            .items
            .len(),
        1
    );

    // The rejected attempt did not consume `tx_b`, so buyer_b can still record
    // a legitimate payment under a distinct receipt_id.
    let receipt_id_b = String::from_str(&env, "rcpt-norm-2");
    client.record_payment(&settler, &receipt_id_b, &id, &buyer_b, &price, &tx_b);
    assert_eq!(client.get_payment(&receipt_id_b).payer, buyer_b);
    assert_eq!(client.get_payment(&receipt_id).payer, buyer_a);
}

#[test]
fn duplicate_payment_receipt_id_with_different_buyer_rejected_by_idempotent_path() {
    let (env, creator, _admin, settler, client) = setup_with_settler();
    let price = 500_000i128;
    let id = register_priced(&env, &creator, &client, "dupnorm2", price);
    let buyer_a = Address::generate(&env);
    let buyer_b = Address::generate(&env);
    let receipt_id = String::from_str(&env, "rcpt-idem-norm");
    let tx_a = String::from_str(&env, "0xtxidemnorma");
    let tx_b = String::from_str(&env, "0xtxidemnormb");

    let original = client.record_payment_idempotent(
        &settler,
        &receipt_id,
        &id,
        &buyer_a,
        &price,
        &tx_a,
    );

    // Idempotent retry rejects a buyer mismatch even when every other argument
    // matches, and also when paired with a fresh tx_hash.
    assert_eq!(
        client.try_record_payment_idempotent(&settler, &receipt_id, &id, &buyer_b, &price, &tx_a),
        Err(Ok(Error::ReceiptAlreadyExists))
    );
    assert_eq!(
        client.try_record_payment_idempotent(&settler, &receipt_id, &id, &buyer_b, &price, &tx_b),
        Err(Ok(Error::ReceiptAlreadyExists))
    );

    // Original buyer and single payment entry remain unchanged.
    assert_eq!(client.get_payment(&receipt_id), original);
    assert_eq!(client.get_payment_receipt(&id, &buyer_a), original);
    assert_eq!(
        client.try_get_payment_receipt(&id, &buyer_b),
        Err(Ok(Error::NotFound))
    );
    assert!(client
        .payment_receipt_page(&buyer_b, &None, &0, &20)
        .items
        .is_empty());
    assert_eq!(
        client
            .payment_receipt_page(&buyer_a, &None, &0, &20)
            .items
            .len(),
        1
    );
}

#[test]
fn canonical_strkey_roundtrip_buyer_blocks_duplicate_payment_and_anchor() {
    let (env, creator, _admin, settler, verifier, client) = setup_with_settler_and_verifier();
    let price = 250_000i128;
    let id = register_priced(&env, &creator, &client, "dupnorm3", price);
    let buyer = Address::generate(&env);

    // Round-trip the buyer through its canonical Stellar StrKey string
    // representation; Soroban normalizes both to the exact same Address.
    let buyer_strkey = buyer.to_string();
    let buyer_normalized = Address::from_string(&buyer_strkey);
    assert_eq!(buyer, buyer_normalized);

    let receipt_id = String::from_str(&env, "rcpt-strkey-1");
    let tx_hash = String::from_str(&env, "0xtxstrkey1");
    client.record_payment(&settler, &receipt_id, &id, &buyer, &price, &tx_hash);

    // Strict record_payment rejects the normalized duplicate without adding a
    // second entry to buyer payment pages.
    assert_eq!(
        client.try_record_payment(
            &settler,
            &receipt_id,
            &id,
            &buyer_normalized,
            &price,
            &String::from_str(&env, "0xtxstrkey2"),
        ),
        Err(Ok(Error::ReceiptAlreadyExists))
    );
    assert_eq!(
        client
            .payment_receipt_page(&buyer, &None, &0, &20)
            .items
            .len(),
        1
    );

    // Idempotent retry with the StrKey-reconstructed buyer recognizes the same
    // canonical buyer and returns the stored receipt without writing a second one.
    let retried = client.record_payment_idempotent(
        &settler,
        &receipt_id,
        &id,
        &buyer_normalized,
        &price,
        &tx_hash,
    );
    assert_eq!(retried.payer, buyer);
    assert_eq!(
        client
            .payment_receipt_page(&buyer_normalized, &Some(id.clone()), &0, &20)
            .items
            .len(),
        1
    );

    // Purchase receipt anchoring also treats the StrKey-reconstructed buyer as
    // identical, rejecting duplicate anchors and preserving the original hash.
    let anchor_hash = String::from_str(&env, "sha256-canonical-anchor");
    client.anchor_purchase_receipt(&verifier, &id, &buyer, &anchor_hash);
    assert_eq!(
        client.try_anchor_purchase_receipt(
            &verifier,
            &id,
            &buyer_normalized,
            &String::from_str(&env, "sha256-replacement"),
        ),
        Err(Ok(Error::DuplicateReceipt))
    );
    let stored_anchor = client.get_purchase_receipt(&id, &buyer_normalized);
    assert_eq!(stored_anchor.buyer, buyer);
    assert_eq!(stored_anchor.receipt_hash, anchor_hash);
}

#[test]
fn duplicate_tx_hash_with_different_buyer_and_new_receipt_id_rejected() {
    let (env, creator, _admin, settler, client) = setup_with_settler();
    let price = 300_000i128;
    let id = register_priced(&env, &creator, &client, "dupnorm4", price);
    let buyer_a = Address::generate(&env);
    let buyer_b = Address::generate(&env);
    let receipt_a = String::from_str(&env, "rcpt-txnorm-a");
    let receipt_b = String::from_str(&env, "rcpt-txnorm-b");
    let shared_tx = String::from_str(&env, "0xtxsharednorm");

    client.record_payment(&settler, &receipt_a, &id, &buyer_a, &price, &shared_tx);

    // Reusing the same settlement tx_hash under a new receipt_id and different
    // buyer must fail with DuplicateTxHash on both strict and idempotent paths.
    assert_eq!(
        client.try_record_payment(&settler, &receipt_b, &id, &buyer_b, &price, &shared_tx),
        Err(Ok(Error::DuplicateTxHash))
    );
    assert_eq!(
        client.try_record_payment_idempotent(
            &settler,
            &receipt_b,
            &id,
            &buyer_b,
            &price,
            &shared_tx,
        ),
        Err(Ok(Error::DuplicateTxHash))
    );

    assert_eq!(
        client.try_get_payment(&receipt_b),
        Err(Ok(Error::NotFound))
    );
    assert!(client
        .payment_receipt_page(&buyer_b, &None, &0, &20)
        .items
        .is_empty());
    assert_eq!(client.get_payment(&receipt_a).payer, buyer_a);
    assert_eq!(
        client
            .payment_receipt_page(&buyer_a, &None, &0, &20)
            .items
            .len(),
        1
    );
}
