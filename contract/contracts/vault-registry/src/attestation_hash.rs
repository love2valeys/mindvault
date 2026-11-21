//! Versioned algorithm-tagged attestation hash normalization and parsing (#229).
//!
//! On-chain attestation hashes stored under `DataKey::AttestationHash(id)` use a
//! compact string representation so legacy receipts remain readable while newer
//! verifiers can adopt alternative hash algorithms or encoding versions:
//!
//! - Legacy untagged `<digest>` (1..=64 bytes): normalized on write to
//!   `sha256:<digest>` and parsed as `(version = 1, algorithm = "sha256")`.
//! - Algorithm-tagged `<algorithm>:<digest>`: preserved as-is and parsed as
//!   `(version = 1, algorithm, digest)`.
//! - Versioned `<version>:<algorithm>:<digest>` (e.g. `v1:sha256:<digest>`,
//!   `v2:poseidon:<digest>`): preserved as-is and parsed with its explicit
//!   `version`.

use soroban_sdk::{contracttype, Env, String};

use crate::{
    Error, ATTESTATION_ENCODING_VERSION_V1, DEFAULT_ATTESTATION_HASH_ALGORITHM,
    MAX_ATTESTATION_HASH_ALGORITHM_LEN, MAX_ATTESTATION_HASH_LEN,
};

/// Parsed, versioned representation of a memoized on-chain attestation hash.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TaggedAttestationHash {
    /// Encoding version (`ATTESTATION_ENCODING_VERSION_V1` = `1` when omitted).
    pub version: u32,
    /// Lowercase hashing algorithm tag (e.g. `"sha256"`, `"keccak256"`, `"poseidon"`).
    pub algorithm: String,
    /// Raw digest string (1..=`MAX_ATTESTATION_HASH_LEN` bytes).
    pub digest: String,
}

fn string_bytes(value: &String) -> alloc::vec::Vec<u8> {
    let mut bytes = alloc::vec![0u8; value.len() as usize];
    value.copy_into_slice(&mut bytes);
    bytes
}

fn validate_algorithm(algorithm: &[u8]) -> Result<(), Error> {
    if algorithm.is_empty() || algorithm.len() > MAX_ATTESTATION_HASH_ALGORITHM_LEN as usize {
        return Err(Error::AttestationHashTooLong);
    }
    for &byte in algorithm {
        if !matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_') {
            return Err(Error::AttestationHashTooLong);
        }
    }
    Ok(())
}

fn validate_digest(digest: &[u8]) -> Result<(), Error> {
    if digest.is_empty() || digest.len() > MAX_ATTESTATION_HASH_LEN as usize {
        return Err(Error::AttestationHashTooLong);
    }
    Ok(())
}

fn parse_version_tag(tag: &[u8]) -> Result<u32, Error> {
    if tag.len() < 2 || tag[0] != b'v' || tag[1] == b'0' {
        return Err(Error::AttestationHashTooLong);
    }
    let mut version: u32 = 0;
    for &byte in &tag[1..] {
        if !byte.is_ascii_digit() {
            return Err(Error::AttestationHashTooLong);
        }
        version = version
            .checked_mul(10)
            .and_then(|v| v.checked_add(u32::from(byte - b'0')))
            .ok_or(Error::AttestationHashTooLong)?;
    }
    Ok(version)
}

fn parse_parts(bytes: &[u8]) -> Result<(u32, &[u8], &[u8], bool), Error> {
    let mut colons = [0usize; 2];
    let mut colon_count = 0usize;
    for (idx, &byte) in bytes.iter().enumerate() {
        if byte == b':' {
            if colon_count == 2 {
                return Err(Error::AttestationHashTooLong);
            }
            colons[colon_count] = idx;
            colon_count += 1;
        }
    }

    match colon_count {
        0 => {
            validate_digest(bytes)?;
            Ok((
                ATTESTATION_ENCODING_VERSION_V1,
                DEFAULT_ATTESTATION_HASH_ALGORITHM.as_bytes(),
                bytes,
                false,
            ))
        }
        1 => {
            let algorithm = &bytes[..colons[0]];
            let digest = &bytes[colons[0] + 1..];
            validate_algorithm(algorithm)?;
            validate_digest(digest)?;
            Ok((ATTESTATION_ENCODING_VERSION_V1, algorithm, digest, true))
        }
        2 => {
            let version_tag = &bytes[..colons[0]];
            let algorithm = &bytes[colons[0] + 1..colons[1]];
            let digest = &bytes[colons[1] + 1..];
            let version = parse_version_tag(version_tag)?;
            validate_algorithm(algorithm)?;
            validate_digest(digest)?;
            Ok((version, algorithm, digest, true))
        }
        _ => Err(Error::AttestationHashTooLong),
    }
}

/// Validate and normalize an attestation hash for persistent memoization.
///
/// Untagged digests are prefixed with `DEFAULT_ATTESTATION_HASH_ALGORITHM`
/// (`sha256:<digest>`). Explicitly tagged (`<algorithm>:<digest>`) and
/// versioned (`v<version>:<algorithm>:<digest>`) values are validated and
/// preserved verbatim.
pub(crate) fn normalize_attestation_hash(env: &Env, hash: &String) -> Result<String, Error> {
    let bytes = string_bytes(hash);
    let (_version, algorithm, digest, had_tag) = parse_parts(&bytes)?;
    if had_tag {
        return Ok(hash.clone());
    }
    let mut tagged = alloc::vec::Vec::with_capacity(algorithm.len() + 1 + digest.len());
    tagged.extend_from_slice(algorithm);
    tagged.push(b':');
    tagged.extend_from_slice(digest);
    Ok(String::from_bytes(env, &tagged))
}

/// Parse a stored attestation hash (including legacy untagged receipts) into
/// its versioned algorithm tag and raw digest.
pub(crate) fn parse_tagged_attestation_hash(
    env: &Env,
    stored: &String,
) -> Result<TaggedAttestationHash, Error> {
    let bytes = string_bytes(stored);
    let (version, algorithm, digest, _had_tag) = parse_parts(&bytes)?;
    Ok(TaggedAttestationHash {
        version,
        algorithm: String::from_bytes(env, algorithm),
        digest: String::from_bytes(env, digest),
    })
}
