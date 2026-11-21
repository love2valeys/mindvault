//! End-to-end x402 settle + anchor integration test (#227).
//!
//! Validates the full x402 lifecycle according to README specification:
//! 1. Register resource with price and metadata pointer.
//! 2. Record payment with memo (`record_payment_with_memo`) in Escrowed state.
//! 3. Verify payment receipt state and lookup by ID and (resource, payer).
//! 4. Settle payment (`settle_payment`) transitioning to Settled state.
//! 5. Anchor receipt hash via authorized verifier (`anchor_purchase_receipt`).
//! 6. Verify immutable purchase receipt anchor and duplicate protection.

#[test]
fn test_x402_record_settle_anchor_e2e_flow() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let buyer = Address::generate(&env);
    let resource_id = String::from_str(&env, "x402-res-1");
    let price = 5_000_000i128; // 5 USDC (7 decimals)
    let meta_uri = String::from_str(&env, "ipfs://bafybeix402resource");

    // 1. Register resource
    client.register(
        &creator,
        &resource_id,
        &price,
        &meta_uri,
        &empty_tags(&env),
    );

    let res = client.get(&resource_id);
    assert_eq!(res.price, price);
    assert_eq!(res.creator, creator);

    // 2. Record payment with memo (mv:<resource_id>)
    let receipt_id = String::from_str(&env, "rcpt_x402_001");
    let tx_hash = String::from_str(&env, "0x9876543210abcdef0123456789abcdef9876543210abcdef0123456789abcdef");
    let memo = String::from_str(&env, "mv:x402-res-1");

    client.record_payment_with_memo(
        &settler,
        &receipt_id,
        &resource_id,
        &buyer,
        &price,
        &tx_hash,
        &memo,
    );

    // 3. Inspect Escrowed receipt
    let receipt = client.get_payment(&receipt_id);
    assert_eq!(receipt.receipt_id, receipt_id);
    assert_eq!(receipt.resource_id, resource_id);
    assert_eq!(receipt.payer, buyer);
    assert_eq!(receipt.amount, price);
    assert_eq!(receipt.tx_hash, tx_hash);
    assert_eq!(receipt.state, PaymentState::Escrowed);

    let stored_memo = client.get_payment_memo(&receipt_id);
    assert_eq!(stored_memo, Some(memo));

    // 4. Settle payment
    client.settle_payment(&settler, &receipt_id);

    let settled_receipt = client.get_payment(&receipt_id);
    assert_eq!(settled_receipt.state, PaymentState::Settled);

    // 5. Anchor purchase receipt hash via verifier
    let receipt_hash = String::from_str(&env, "sha256:4a5b6c7d8e9f0123456789abcdef4a5b6c7d8e9f0123456789abcdef0123");
    client.anchor_purchase_receipt(
        &verifier,
        &resource_id,
        &buyer,
        &receipt_hash,
    );

    // 6. Verify purchase receipt anchor
    let anchor = client.get_purchase_receipt(&resource_id, &buyer);
    assert_eq!(anchor.resource_id, resource_id);
    assert_eq!(anchor.buyer, buyer);
    assert_eq!(anchor.receipt_hash, receipt_hash);

    // 7. Verify duplicate anchor rejection
    let dup_hash = String::from_str(&env, "sha256:differenthash");
    let dup_err = client.try_anchor_purchase_receipt(
        &verifier,
        &resource_id,
        &buyer,
        &dup_hash,
    );
    assert_eq!(dup_err, Err(Ok(Error::DuplicateReceipt)));
}
