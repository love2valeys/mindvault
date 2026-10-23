fn persistent_entry_ttl(env: &Env, contract: &Address, key: &DataKey) -> u32 {
    env.as_contract(contract, || env.storage().persistent().get_ttl(key))
}

#[test]
fn persistent_mutation_key_families_bump_ttl_near_threshold() {
    let (env, creator, admin, settler, client) = setup_with_settler();
    let contract = client.address.clone();
    let id = register_tagged(&env, &creator, &client, "ttlmut", &["old"]);
    let payer = Address::generate(&env);
    let receipt_id = String::from_str(&env, "ttlpay1");
    client.record_payment(
        &settler,
        &receipt_id,
        &id,
        &payer,
        &100i128,
        &String::from_str(&env, "ttl-tx-1"),
    );

    let holder = Address::generate(&env);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Week,
        &2_000i128,
        &String::from_str(&env, "ttl-lease-1"),
    );

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);

    let mut next_tags = Vec::new(&env);
    next_tags.push_back(String::from_str(&env, "new"));
    client.set_tags(&id, &next_tags);
    let mut index_ids = Vec::new(&env);
    index_ids.push_back(id.clone());
    client.repair_index(&index_ids);
    client.record_payment(
        &settler,
        &String::from_str(&env, "ttlpay2"),
        &id,
        &payer,
        &100i128,
        &String::from_str(&env, "ttl-tx-2"),
    );
    client.settle_payment(&settler, &receipt_id);
    client.record_admin_refund(
        &admin,
        &String::from_str(&env, "ttlrefund"),
        &receipt_id,
        &payer,
        &1i128,
        &String::from_str(&env, "ttl-refund-tx"),
    );
    client.renew_lease(
        &holder,
        &id,
        &LeaseTier::Week,
        &2_000i128,
        &String::from_str(&env, "ttl-lease-2"),
    );
    let next_creator = Address::generate(&env);
    client.transfer_ownership(&id, &next_creator);

    for key in [
        DataKey::Resource(id.clone()),
        DataKey::Index(0),
        DataKey::TagIndex(String::from_str(&env, "new")),
        DataKey::CreatorResources(next_creator),
        DataKey::PaymentReceipt(String::from_str(&env, "ttlpay2")),
        DataKey::PaymentRecipient(String::from_str(&env, "ttlpay2")),
        DataKey::PaymentIndex(id.clone(), payer.clone()),
        DataKey::PaymentTxHash(String::from_str(&env, "ttl-tx-2")),
        DataKey::BuyerPayments(payer.clone()),
        DataKey::BuyerResourcePayments(payer.clone(), id.clone()),
        DataKey::RefundDeadline(receipt_id.clone()),
        DataKey::RefundReceipt(String::from_str(&env, "ttlrefund")),
        DataKey::RefundedAmount(receipt_id.clone()),
        DataKey::RefundTxHash(String::from_str(&env, "ttl-refund-tx")),
        DataKey::Lease(id.clone(), holder.clone()),
    ] {
        assert_eq!(
            persistent_entry_ttl(&env, &contract, &key),
            TTL_BUMP_AMOUNT,
            "mutation must refresh persistent key TTL"
        );
    }
}