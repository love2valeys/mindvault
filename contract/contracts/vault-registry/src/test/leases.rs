// ─── Time-limited access leases (#803) ───────────────────────────────────────
//
// On-chain lease fields (`start_ledger`, `expiry_ledger`, `tier`) plus the
// read helpers a paywall needs, and the holder / settler / creator paths
// that write them.

fn lease_setup<'a>() -> (Env, Address, Address, Address, VaultRegistryClient<'a>) {
    let (env, creator, _admin, client) = setup_with_admin();
    let settler = Address::generate(&env);
    client.add_settler(&settler);
    let holder = Address::generate(&env);
    (env, creator, settler, holder, client)
}

fn last_lease_event(env: &Env, expected_topic: Symbol, expected_id: &String) -> Lease {
    let all = env.events().all();
    let (_cid, topics, data) = all.get_unchecked(all.len() - 1);
    let topic: Symbol = Symbol::try_from_val(env, &topics.get(0).unwrap()).unwrap();
    assert_eq!(topic, expected_topic);
    let topic_id: String = String::try_from_val(env, &topics.get(1).unwrap()).unwrap();
    assert_eq!(&topic_id, expected_id);
    Lease::try_from_val(env, &data).unwrap()
}

#[test]
fn lease_tier_ledgers_and_prices_follow_the_documented_tables() {
    let (env, creator, _settler, _holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasetier", 1_000i128);

    assert_eq!(LEASE_HOUR_LEDGERS, 720);
    assert_eq!(LEASE_DAY_LEDGERS, 17_280);
    assert_eq!(LEASE_WEEK_LEDGERS, 120_960);

    assert_eq!(client.lease_price(&id, &LeaseTier::Hour), 1_000i128);
    assert_eq!(client.lease_price(&id, &LeaseTier::Day), 5_000i128);
    assert_eq!(client.lease_price(&id, &LeaseTier::Week), 20_000i128);

    // The price tracks the resource's current price.
    client.set_price(&id, &2_000i128);
    assert_eq!(client.lease_price(&id, &LeaseTier::Week), 40_000i128);

    assert_eq!(
        client.try_lease_price(&String::from_str(&env, "nosuchlease"), &LeaseTier::Hour),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn buy_lease_records_a_pending_window_from_the_current_ledger() {
    let (env, creator, _settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasebuy", 300i128);
    env.ledger().set_sequence_number(1_000);

    let lease = client.buy_lease(
        &holder,
        &id,
        &LeaseTier::Day,
        &1_500i128,
        &String::from_str(&env, "0xleasetx"),
    );

    assert_eq!(lease.resource_id, id);
    assert_eq!(lease.holder, holder);
    assert_eq!(lease.tier, LeaseTier::Day);
    assert_eq!(lease.start_ledger, 1_000);
    assert_eq!(lease.expiry_ledger, 1_000 + LEASE_DAY_LEDGERS);
    assert_eq!(lease.amount, 1_500i128);
    assert_eq!(lease.tx_hash, String::from_str(&env, "0xleasetx"));
    assert_eq!(lease.state, LeaseState::Pending);
    assert_eq!(lease.recorded_at, 1_000);
    assert_eq!(last_lease_event(&env, symbol_short!("lease"), &id), lease);

    // Pending grants nothing yet; the record is readable.
    assert!(!client.lease_is_active(&id, &holder));
    assert_eq!(client.get_lease(&id, &holder), lease);
}

#[test]
fn settle_lease_activates_and_the_window_expires_by_ledger() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasesetl", 100i128);
    env.ledger().set_sequence_number(50);
    client.buy_lease(
        &holder,
        &id,
        &LeaseTier::Hour,
        &100i128,
        &String::from_str(&env, "tx1"),
    );

    let settled = client.settle_lease(&settler, &id, &holder);
    assert_eq!(settled.state, LeaseState::Active);
    assert_eq!(last_lease_event(&env, symbol_short!("leasesetl"), &id), settled);
    assert!(client.lease_is_active(&id, &holder));

    // Last ledger inside the window.
    env.ledger().set_sequence_number(50 + LEASE_HOUR_LEDGERS - 1);
    assert!(client.lease_is_active(&id, &holder));
    // First ledger outside it.
    env.ledger().set_sequence_number(50 + LEASE_HOUR_LEDGERS);
    assert!(!client.lease_is_active(&id, &holder));
    // The record survives expiry for audit.
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Active);

    // Settling twice is a transition error, as is settling without a lease.
    assert_eq!(
        client.try_settle_lease(&settler, &id, &holder),
        Err(Ok(Error::InvalidPaymentTransition))
    );
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_settle_lease(&settler, &id, &stranger),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn record_lease_writes_an_active_lease_for_a_settler() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leaserec", 100i128);
    env.ledger().set_sequence_number(10);

    let lease = client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Week,
        &2_000i128,
        &String::from_str(&env, "x402settled"),
    );
    assert_eq!(lease.state, LeaseState::Active);
    assert_eq!(lease.expiry_ledger, 10 + LEASE_WEEK_LEDGERS);
    assert!(client.lease_is_active(&id, &holder));

    // Only a settler may record directly.
    let not_settler = Address::generate(&env);
    let other = Address::generate(&env);
    assert_eq!(
        client.try_record_lease(
            &not_settler,
            &other,
            &id,
            &LeaseTier::Hour,
            &100i128,
            &String::from_str(&env, "tx"),
        ),
        Err(Ok(Error::NotSettler))
    );
}

#[test]
fn lease_rejects_wrong_amount_bad_hash_and_unlisted_resources() {
    let (env, creator, _settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasebad", 100i128);
    let tx = String::from_str(&env, "tx");

    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Day, &100i128, &tx),
        Err(Ok(Error::PaymentAmountMismatch))
    );
    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Day, &0i128, &tx),
        Err(Ok(Error::InvalidPaymentAmount))
    );
    assert_eq!(
        client.try_buy_lease(
            &holder,
            &id,
            &LeaseTier::Day,
            &500i128,
            &String::from_str(&env, "")
        ),
        Err(Ok(Error::InvalidTxHash))
    );
    assert_eq!(
        client.try_buy_lease(
            &holder,
            &String::from_str(&env, "nosuchres"),
            &LeaseTier::Day,
            &500i128,
            &tx
        ),
        Err(Ok(Error::NotFound))
    );

    // Delisted, frozen, disputed: not for sale, so not for lease.
    client.delist(&id);
    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Day, &500i128, &tx),
        Err(Ok(Error::ResourceNotMutable))
    );
    client.freeze_resource(&id);
    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Day, &500i128, &tx),
        Err(Ok(Error::ResourceNotMutable))
    );
    client.reactivate_resource(&id);
    assert!(client
        .try_buy_lease(&holder, &id, &LeaseTier::Day, &500i128, &tx)
        .is_ok());
}

#[test]
fn one_open_lease_per_pair_but_expired_or_revoked_leases_are_replaced() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leaseonce", 100i128);
    let tx = String::from_str(&env, "tx");
    env.ledger().set_sequence_number(100);

    client.buy_lease(&holder, &id, &LeaseTier::Hour, &100i128, &tx);
    // Pending blocks a second purchase.
    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Hour, &100i128, &tx),
        Err(Ok(Error::AlreadyRegistered))
    );
    client.settle_lease(&settler, &id, &holder);
    // Active and unexpired blocks it too.
    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Day, &500i128, &tx),
        Err(Ok(Error::AlreadyRegistered))
    );
    // Another holder is independent.
    let other = Address::generate(&env);
    client.buy_lease(&other, &id, &LeaseTier::Hour, &100i128, &tx);

    // Expired: replaced by a new lease.
    env.ledger().set_sequence_number(100 + LEASE_HOUR_LEDGERS);
    let renewed = client.buy_lease(&holder, &id, &LeaseTier::Day, &500i128, &tx);
    assert_eq!(renewed.tier, LeaseTier::Day);
    assert_eq!(renewed.start_ledger, 100 + LEASE_HOUR_LEDGERS);
    assert_eq!(renewed.state, LeaseState::Pending);

    // Revoked: replaced as well.
    client.revoke_lease(&id, &holder);
    let again = client.buy_lease(&holder, &id, &LeaseTier::Week, &2_000i128, &tx);
    assert_eq!(again.tier, LeaseTier::Week);
}

#[test]
fn revoke_lease_is_creator_only_immediate_and_terminal() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leaserevk", 100i128);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Day,
        &500i128,
        &String::from_str(&env, "tx"),
    );
    assert!(client.lease_is_active(&id, &holder));

    let revoked = client.revoke_lease(&id, &holder);
    assert_eq!(revoked.state, LeaseState::Revoked);
    assert_eq!(last_lease_event(&env, symbol_short!("leaserevk"), &id), revoked);
    assert!(!client.lease_is_active(&id, &holder));
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);

    assert_eq!(
        client.try_revoke_lease(&id, &holder),
        Err(Ok(Error::InvalidPaymentTransition))
    );
    let stranger = Address::generate(&env);
    assert_eq!(
        client.try_revoke_lease(&id, &stranger),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn revoke_lease_requires_the_creator_signature() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leaseauth", 100i128);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Hour,
        &100i128,
        &String::from_str(&env, "tx"),
    );

    let intruder = Address::generate(&env);
    let invoke = MockAuthInvoke {
        contract: &client.address,
        fn_name: "revoke_lease",
        args: (id.clone(), holder.clone()).into_val(&env),
        sub_invokes: &[],
    };
    let auth = [MockAuth {
        address: &intruder,
        invoke: &invoke,
    }];
    let res = client.mock_auths(&auth).try_revoke_lease(&id, &holder);
    assert!(res.is_err(), "a non-creator must not be able to revoke a lease");
    assert!(client.lease_is_active(&id, &holder));
}

#[test]
fn lease_reads_never_panic_on_unknown_pairs_or_bad_ids() {
    let (env, creator, _settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leaseread", 100i128);

    assert!(!client.lease_is_active(&id, &holder));
    assert!(!client.lease_is_active(&String::from_str(&env, "NOT-VALID"), &holder));
    assert_eq!(client.try_get_lease(&id, &holder), Err(Ok(Error::NotFound)));
    assert_eq!(
        client.try_get_lease(&String::from_str(&env, "NOT-VALID"), &holder),
        Err(Ok(Error::InvalidResourceId))
    );
}

#[test]
fn lease_writes_respect_the_emergency_pause() {
    let (env, creator, settler, holder, client) = lease_setup();
    let admin = client.admin().unwrap();
    let id = register_priced(&env, &creator, &client, "leasepause", 100i128);
    let tx = String::from_str(&env, "tx");

    client.set_paused(&admin, &true);
    assert_eq!(
        client.try_buy_lease(&holder, &id, &LeaseTier::Hour, &100i128, &tx),
        Err(Ok(Error::ContractPaused))
    );
    assert_eq!(
        client.try_record_lease(&settler, &holder, &id, &LeaseTier::Hour, &100i128, &tx),
        Err(Ok(Error::ContractPaused))
    );
    // Reads still work while paused.
    assert!(!client.lease_is_active(&id, &holder));
    assert_eq!(client.lease_price(&id, &LeaseTier::Hour), 100i128);
}

#[test]
fn renew_lease_extends_active_lease_without_access_gap() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasernw", 100i128);
    env.ledger().set_sequence_number(1_000);

    let price_hour = client.lease_price(&id, &LeaseTier::Hour);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Hour,
        &price_hour,
        &String::from_str(&env, "tx0"),
    );
    let original = client.get_lease(&id, &holder);
    assert_eq!(original.start_ledger, 1_000);
    assert_eq!(original.expiry_ledger, 1_000 + LEASE_HOUR_LEDGERS);
    assert!(client.lease_is_active(&id, &holder));

    // Advance halfway through the lease.
    env.ledger().set_sequence_number(1_400);
    assert!(client.lease_is_active(&id, &holder));

    // Renew with Day tier.
    let price_day = client.lease_price(&id, &LeaseTier::Day);
    let renewed = client.renew_lease(
        &holder,
        &id,
        &LeaseTier::Day,
        &price_day,
        &String::from_str(&env, "tx1"),
    );

    // Start ledger preserved, expiry extended on top of original expiry (no gap, no lost time).
    assert_eq!(renewed.start_ledger, 1_000);
    assert_eq!(
        renewed.expiry_ledger,
        1_000 + LEASE_HOUR_LEDGERS + LEASE_DAY_LEDGERS
    );
    assert_eq!(renewed.tier, LeaseTier::Day);
    assert_eq!(renewed.state, LeaseState::Active);

    // Event was emitted.
    let evt = last_lease_event(&env, Symbol::new(&env, "leasernw"), &id);
    assert_eq!(evt, renewed);

    // Continuous uninterrupted access through the old boundary and up to new expiry.
    env.ledger().set_sequence_number(1_000 + LEASE_HOUR_LEDGERS);
    assert!(client.lease_is_active(&id, &holder));
    env.ledger().set_sequence_number(1_000 + LEASE_HOUR_LEDGERS + 10);
    assert!(client.lease_is_active(&id, &holder));
}

#[test]
fn renew_lease_exact_expiry_boundaries() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasernwbnd", 100i128);
    env.ledger().set_sequence_number(1_000);

    let price_hour = client.lease_price(&id, &LeaseTier::Hour);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Hour,
        &price_hour,
        &String::from_str(&env, "tx0"),
    );
    let expiry = 1_000 + LEASE_HOUR_LEDGERS;

    // At expiry - 1: renewal is valid.
    env.ledger().set_sequence_number(expiry - 1);
    assert!(client.lease_is_active(&id, &holder));
    let renewed = client.renew_lease(
        &holder,
        &id,
        &LeaseTier::Hour,
        &price_hour,
        &String::from_str(&env, "tx_renew_valid"),
    );
    assert_eq!(renewed.expiry_ledger, expiry + LEASE_HOUR_LEDGERS);

    // Move to exactly the new expiry ledger: now expired!
    let new_expiry = expiry + LEASE_HOUR_LEDGERS;
    env.ledger().set_sequence_number(new_expiry);
    assert!(!client.lease_is_active(&id, &holder));

    // Renewal at or after expiry fails with InvalidPaymentTransition.
    assert_eq!(
        client.try_renew_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &price_hour,
            &String::from_str(&env, "tx_expired"),
        ),
        Err(Ok(Error::InvalidPaymentTransition))
    );

    // Past expiry also fails.
    env.ledger().set_sequence_number(new_expiry + 10);
    assert_eq!(
        client.try_renew_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &price_hour,
            &String::from_str(&env, "tx_past"),
        ),
        Err(Ok(Error::InvalidPaymentTransition))
    );
}

#[test]
fn renew_lease_validates_pricing_state_and_auth() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leaserules", 100i128);
    env.ledger().set_sequence_number(1_000);

    let price_hour = client.lease_price(&id, &LeaseTier::Hour);
    client.buy_lease(
        &holder,
        &id,
        &LeaseTier::Hour,
        &price_hour,
        &String::from_str(&env, "tx0"),
    );

    // Pending lease cannot be renewed (must be Active).
    assert_eq!(
        client.try_renew_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &price_hour,
            &String::from_str(&env, "txrnw"),
        ),
        Err(Ok(Error::InvalidPaymentTransition))
    );

    client.settle_lease(&settler, &id, &holder);

    // Wrong price is rejected.
    assert_eq!(
        client.try_renew_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &999i128,
            &String::from_str(&env, "txrnw"),
        ),
        Err(Ok(Error::PaymentAmountMismatch))
    );

    // Delisted resource rejects renewal.
    client.delist(&id);
    assert_eq!(
        client.try_renew_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &price_hour,
            &String::from_str(&env, "txrnw"),
        ),
        Err(Ok(Error::ResourceNotMutable))
    );

    // Relist and revoke.
    client.set_listed(&id, &true);
    client.revoke_lease(&id, &holder);

    // Revoked lease cannot be renewed.
    assert_eq!(
        client.try_renew_lease(
            &holder,
            &id,
            &LeaseTier::Hour,
            &price_hour,
            &String::from_str(&env, "txrnw"),
        ),
        Err(Ok(Error::InvalidPaymentTransition))
    );
}

#[test]
fn renew_lease_keeps_holder_binding_after_resource_transfer() {
    let (env, creator, settler, holder, client) = lease_setup();
    let id = register_priced(&env, &creator, &client, "leasexfer", 100i128);
    let price = client.lease_price(&id, &LeaseTier::Hour);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Hour,
        &price,
        &String::from_str(&env, "tx_before_transfer"),
    );

    let new_creator = Address::generate(&env);
    client.transfer_ownership(&id, &new_creator);
    let renewed = client.renew_lease(
        &holder,
        &id,
        &LeaseTier::Hour,
        &price,
        &String::from_str(&env, "tx_after_transfer"),
    );
    assert_eq!(renewed.resource_id, id);
    assert_eq!(renewed.holder, holder);

    let other_holder = Address::generate(&env);
    assert_eq!(
        client.try_renew_lease(
            &other_holder,
            &id,
            &LeaseTier::Hour,
            &price,
            &String::from_str(&env, "tx_wrong_holder"),
        ),
        Err(Ok(Error::NotFound))
    );
}

#[test]
fn list_holder_leases_pages_active_leases_and_filters_expired_and_other_holders() {
    let (env, creator, settler, holder, client) = lease_setup();
    let first_id = register_priced(&env, &creator, &client, "leasepage1", 100i128);
    let other_id = register_priced(&env, &creator, &client, "leasepage2", 100i128);
    let active_id = register_priced(&env, &creator, &client, "leasepage3", 100i128);
    let expired_id = register_priced(&env, &creator, &client, "leasepage4", 100i128);
    let other_holder = Address::generate(&env);
    env.ledger().set_sequence_number(1_000);

    let week_price = client.lease_price(&first_id, &LeaseTier::Week);
    client.record_lease(
        &settler,
        &holder,
        &first_id,
        &LeaseTier::Week,
        &week_price,
        &String::from_str(&env, "tx_page_first"),
    );
    let other_week_price = client.lease_price(&other_id, &LeaseTier::Week);
    client.record_lease(
        &settler,
        &other_holder,
        &other_id,
        &LeaseTier::Week,
        &other_week_price,
        &String::from_str(&env, "tx_page_other"),
    );
    let active_week_price = client.lease_price(&active_id, &LeaseTier::Week);
    client.record_lease(
        &settler,
        &holder,
        &active_id,
        &LeaseTier::Week,
        &active_week_price,
        &String::from_str(&env, "tx_page_active"),
    );
    let hour_price = client.lease_price(&expired_id, &LeaseTier::Hour);
    client.record_lease(
        &settler,
        &holder,
        &expired_id,
        &LeaseTier::Hour,
        &hour_price,
        &String::from_str(&env, "tx_page_expired"),
    );
    env.ledger()
        .set_sequence_number(1_000 + LEASE_HOUR_LEDGERS);

    let first_page = client.list_holder_leases_page(&holder, &0, &1);
    assert_eq!(first_page.leases.len(), 1);
    assert_eq!(first_page.leases.get(0).unwrap().resource_id, first_id);
    assert_eq!(first_page.next_cursor, Some(1));

    let second_page = client.list_holder_leases_page(&holder, &1, &2);
    assert_eq!(second_page.leases.len(), 1);
    assert_eq!(second_page.leases.get(0).unwrap().resource_id, active_id);
    assert_eq!(second_page.next_cursor, Some(3));

    let third_page = client.list_holder_leases_page(&holder, &3, &1);
    assert!(third_page.leases.is_empty());
    assert_eq!(third_page.next_cursor, None);
}

#[test]
fn revoked_leases_remain_inactive_after_every_resource_state_transition() {
    let (env, creator, settler, holder, client) = lease_setup();
    let admin = client.admin().unwrap();
    let id = register_priced(&env, &creator, &client, "leasematrix", 100i128);
    env.ledger().set_sequence_number(1_000);

    let price_hour = client.lease_price(&id, &LeaseTier::Hour);
    client.record_lease(
        &settler,
        &holder,
        &id,
        &LeaseTier::Hour,
        &price_hour,
        &String::from_str(&env, "tx0"),
    );
    assert!(client.lease_is_active(&id, &holder));

    // Creator revokes the active lease.
    client.revoke_lease(&id, &holder);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));

    // Matrix step 1: freeze resource.
    client.freeze_resource(&id);
    assert_eq!(client.get(&id).state, ResourceState::Frozen);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));
    assert!(client.try_renew_lease(&holder, &id, &LeaseTier::Hour, &price_hour, &String::from_str(&env, "tx")).is_err());

    // Matrix step 2: reactivate resource (Listed). Revoked lease must stay Revoked and inactive!
    client.reactivate_resource(&id);
    assert_eq!(client.get(&id).state, ResourceState::Listed);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder), "Reactivation must NEVER revive a revoked lease");
    assert_eq!(
        client.try_renew_lease(&holder, &id, &LeaseTier::Hour, &price_hour, &String::from_str(&env, "tx")),
        Err(Ok(Error::InvalidPaymentTransition))
    );

    // Matrix step 3: delist resource.
    client.delist(&id);
    assert_eq!(client.get(&id).state, ResourceState::Delisted);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));

    // Matrix step 4: relist resource.
    client.set_listed(&id, &true);
    assert_eq!(client.get(&id).state, ResourceState::Listed);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));

    // Matrix step 5: dispute resource.
    client.open_dispute(&id, &admin);
    assert_eq!(client.get(&id).state, ResourceState::Disputed);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));

    // Matrix step 6: resolve dispute to Listed.
    client.resolve_dispute(&id, &admin, &ResourceState::Listed);
    assert_eq!(client.get(&id).state, ResourceState::Listed);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));

    // Matrix step 7: tombstone resource.
    client.tombstone_resource(&id, &admin);
    assert_eq!(client.get(&id).state, ResourceState::Tombstoned);
    assert_eq!(client.get_lease(&id, &holder).state, LeaseState::Revoked);
    assert!(!client.lease_is_active(&id, &holder));
}
