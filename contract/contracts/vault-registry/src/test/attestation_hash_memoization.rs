// ── Versioned attestation hash memoization (#229) ────────────────────────────
//
// Attestation hashes are memoized under `DataKey::AttestationHash(id)` with
// backward-compatible algorithm and version tagging:
// - untagged `<digest>` -> stored as `sha256:<digest>`, parsed as `(v1, sha256, digest)`
// - `<algorithm>:<digest>` -> preserved, parsed as `(v1, algorithm, digest)`
// - `v<N>:<algorithm>:<digest>` -> preserved, parsed as `(N, algorithm, digest)`

#[test]
fn untagged_attestation_hash_memoizes_as_v1_sha256() {
    let (env, creator, _admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "attmemo1");
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let digest = String::from_str(&env, "deadbeef0123456789abcdef");
    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Verified,
        &Some(digest.clone()),
    );

    assert_eq!(
        client.get_attestation_hash(&id),
        Some(String::from_str(&env, "sha256:deadbeef0123456789abcdef"))
    );
    assert_eq!(
        client.get_tagged_attestation_hash(&id),
        Some(TaggedAttestationHash {
            version: ATTESTATION_ENCODING_VERSION_V1,
            algorithm: String::from_str(&env, DEFAULT_ATTESTATION_HASH_ALGORITHM),
            digest,
        })
    );
}

#[test]
fn algorithm_tagged_attestation_hashes_preserve_sha256_poseidon_and_keccak256() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    for (suffix, algo, digest) in [
        ("attalg1", "sha256", "0123456789abcdef"),
        ("attalg2", "poseidon", "998877665544332211"),
        ("attalg3", "keccak256", "fedcba9876543210"),
    ] {
        let id = register_default(&env, &creator, &client, suffix);
        let raw = String::from_str(&env, &format!("{algo}:{digest}"));
        client.set_verification_status(
            &id,
            &verifier,
            &VerificationStatus::Verified,
            &Some(raw.clone()),
        );

        assert_eq!(client.get_attestation_hash(&id), Some(raw));
        assert_eq!(
            client.get_tagged_attestation_hash(&id),
            Some(TaggedAttestationHash {
                version: ATTESTATION_ENCODING_VERSION_V1,
                algorithm: String::from_str(&env, algo),
                digest: String::from_str(&env, digest),
            })
        );
    }
}

#[test]
fn versioned_algorithm_tags_parse_explicit_version_algorithm_and_digest() {
    let (env, creator, _admin, client) = setup_with_admin();
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let id_v1 = register_default(&env, &creator, &client, "attver1");
    let v1_hash = String::from_str(&env, "v1:sha256:abcdef0123456789");
    client.set_verification_status(
        &id_v1,
        &verifier,
        &VerificationStatus::Verified,
        &Some(v1_hash.clone()),
    );
    assert_eq!(client.get_attestation_hash(&id_v1), Some(v1_hash));
    assert_eq!(
        client.get_tagged_attestation_hash(&id_v1),
        Some(TaggedAttestationHash {
            version: 1,
            algorithm: String::from_str(&env, "sha256"),
            digest: String::from_str(&env, "abcdef0123456789"),
        })
    );

    let id_v2 = register_default(&env, &creator, &client, "attver2");
    let v2_hash = String::from_str(&env, "v2:poseidon:00112233445566778899");
    client.set_verification_status(
        &id_v2,
        &verifier,
        &VerificationStatus::Verified,
        &Some(v2_hash.clone()),
    );
    assert_eq!(client.get_attestation_hash(&id_v2), Some(v2_hash));
    assert_eq!(
        client.get_tagged_attestation_hash(&id_v2),
        Some(TaggedAttestationHash {
            version: 2,
            algorithm: String::from_str(&env, "poseidon"),
            digest: String::from_str(&env, "00112233445566778899"),
        })
    );
}

#[test]
fn malformed_or_oversized_versioned_attestation_hashes_are_rejected() {
    let (env, creator, _admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "attbad1");
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let over_digest = format!("sha256:{}", "a".repeat(MAX_ATTESTATION_HASH_LEN as usize + 1));
    let over_algo = format!("{}:abc123", "a".repeat(MAX_ATTESTATION_HASH_ALGORITHM_LEN as usize + 1));

    for bad in [
        "",
        ":abc123",
        "sha256:",
        "SHA256:abc123",
        "v0:sha256:abc123",
        "v01:sha256:abc123",
        "vx:sha256:abc123",
        "1:sha256:abc123",
        "v1:sha256:abc:def",
        "v1::abc123",
        "v1:sha256:",
        over_digest.as_str(),
        over_algo.as_str(),
    ] {
        let res = client.try_set_verification_status(
            &id,
            &verifier,
            &VerificationStatus::Verified,
            &Some(String::from_str(&env, bad)),
        );
        assert_eq!(
            res,
            Err(Ok(Error::AttestationHashTooLong)),
            "expected malformed attestation hash `{bad}` to be rejected"
        );
    }

    assert_eq!(client.get_attestation_hash(&id), None);
    assert_eq!(client.get_tagged_attestation_hash(&id), None);
}

#[test]
fn legacy_untagged_stored_receipt_remains_readable_and_verifiable() {
    let (env, creator, _admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "attleg1");
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let doc_hash = BytesN::from_array(&env, &[0x42u8; 32]);
    let computed =
        client.compute_attestation_hash(&id, &VerificationStatus::Verified, &doc_hash);

    // Strip the `sha256:` prefix to simulate an untagged 64-byte receipt written
    // prior to algorithm tagging.
    let mut raw_bytes = alloc::vec![0u8; computed.len() as usize];
    computed.copy_into_slice(&mut raw_bytes);
    let prefix = b"sha256:";
    assert!(raw_bytes.starts_with(prefix));
    let legacy_digest = String::from_bytes(&env, &raw_bytes[prefix.len()..]);

    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Verified,
        &None::<String>,
    );
    env.as_contract(&client.address, || {
        let key = DataKey::AttestationHash(id.clone());
        env.storage().persistent().set(&key, &legacy_digest);
    });

    assert_eq!(
        client.get_tagged_attestation_hash(&id),
        Some(TaggedAttestationHash {
            version: ATTESTATION_ENCODING_VERSION_V1,
            algorithm: String::from_str(&env, "sha256"),
            digest: legacy_digest,
        })
    );
    assert!(
        client.verify_attestation_hash(&id, &VerificationStatus::Verified, &doc_hash),
        "legacy untagged sha256 receipts must remain verifiable"
    );
}

#[test]
fn verify_attestation_hash_matches_v1_sha256_and_isolates_other_algorithms_and_versions() {
    let (env, creator, _admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "attvrf1");
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let doc_hash = BytesN::from_array(&env, &[0x7au8; 32]);
    let computed =
        client.compute_attestation_hash(&id, &VerificationStatus::Verified, &doc_hash);
    let mut raw_bytes = alloc::vec![0u8; computed.len() as usize];
    computed.copy_into_slice(&mut raw_bytes);
    let digest_str = core::str::from_utf8(&raw_bytes["sha256:".len()..]).unwrap();

    // Explicit v1:sha256 tag verifies against compute_attestation_hash.
    let v1_tagged = String::from_str(&env, &format!("v1:sha256:{digest_str}"));
    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Verified,
        &Some(v1_tagged),
    );
    assert!(client.verify_attestation_hash(&id, &VerificationStatus::Verified, &doc_hash));

    // Same digest under poseidon must not verify as sha256.
    let poseidon_tagged = String::from_str(&env, &format!("poseidon:{digest_str}"));
    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Rejected,
        &Some(poseidon_tagged),
    );
    assert!(!client.verify_attestation_hash(&id, &VerificationStatus::Rejected, &doc_hash));

    // Same digest under v2:sha256 must not collide with v1 verification.
    let v2_tagged = String::from_str(&env, &format!("v2:sha256:{digest_str}"));
    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Verified,
        &Some(v2_tagged),
    );
    assert!(!client.verify_attestation_hash(&id, &VerificationStatus::Verified, &doc_hash));
}

#[test]
fn attestation_hash_memoization_reuses_identical_entry_and_refreshes_ttl() {
    let (env, creator, _admin, client) = setup_with_admin();
    let id = register_default(&env, &creator, &client, "attttl1");
    let verifier = Address::generate(&env);
    client.add_verifier(&verifier);

    let hash = String::from_str(&env, "v1:poseidon:abcdef0123456789");
    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Verified,
        &Some(hash.clone()),
    );

    env.ledger()
        .set_sequence_number(env.ledger().sequence() + TTL_DAY_IN_LEDGERS + 1);

    // Transitioning with the same normalized tagged hash reuses the memoized
    // entry and extends its TTL.
    client.set_verification_status(
        &id,
        &verifier,
        &VerificationStatus::Rejected,
        &Some(hash.clone()),
    );

    assert_eq!(client.get_attestation_hash(&id), Some(hash));
    let key = DataKey::AttestationHash(id);
    let ttl = env.as_contract(&client.address, || env.storage().persistent().get_ttl(&key));
    assert_eq!(ttl, TTL_BUMP_AMOUNT);
}
