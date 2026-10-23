use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env, String, Symbol, Vec};

const ADMIN_KEY: Symbol = symbol_short!("ADMIN");
const ENTRY_COUNT_KEY: Symbol = symbol_short!("ENTRY_CNT");
const DIGEST_VERSION: u32 = 1;
const MAX_DIGEST_PAGE: u32 = 100;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryEntry {
    pub id: u64,
    pub owner: Address,
    pub label: String,
    pub metadata: Bytes,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryDigest {
    pub version: u32,
    pub start: u64,
    pub count: u32,
    pub total: u64,
    pub next_cursor: u64,
    pub digest: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryDigestPage {
    pub version: u32,
    pub start: u64,
    pub count: u32,
    pub total: u64,
    pub next_cursor: u64,
    pub digest: BytesN<32>,
}

fn entry_key(env: &Env, id: u64) -> Bytes {
    let mut key = Bytes::new(env);
    key.append_slice(b"ENTRY");
    key.extend_from_array(&id.to_be_bytes());
    key
}

fn read_entry(env: &Env, id: u64) -> Option<RegistryEntry> {
    env.storage().persistent().get(&entry_key(env, id))
}

fn entry_count(env: &Env) -> u64 {
    env.storage().instance().get(&ENTRY_COUNT_KEY).unwrap_or(0)
}

fn encode_entry(env: &Env, buf: &mut Bytes, entry: &RegistryEntry) {
    buf.extend_from_array(&entry.id.to_be_bytes());
    let owner = entry.owner.clone();
    let owner_bytes = owner.to_string();
    let owner_len = owner_bytes.len() as u32;
    buf.extend_from_array(&owner_len.to_be_bytes());
    buf.append(&owner_bytes);
    let label_len = entry.label.len();
    buf.extend_from_array(&label_len.to_be_bytes());
    buf.append(&entry.label.clone().into());
    let meta_len = entry.metadata.len();
    buf.extend_from_array(&meta_len.to_be_bytes());
    buf.append(&entry.metadata);
}

fn compute_digest(env: &Env, start: u64, count: u32, total: u64) -> BytesN<32> {
    let mut buf = Bytes::new(env);
    buf.extend_from_array(&DIGEST_VERSION.to_be_bytes());
    buf.extend_from_array(&start.to_be_bytes());
    buf.extend_from_array(&count.to_be_bytes());
    buf.extend_from_array(&total.to_be_bytes());
    let mut i: u64 = 0;
    while i < count as u64 {
        let id = start + i;
        if let Some(entry) = read_entry(env, id) {
            encode_entry(env, &mut buf, &entry);
        }
        i += 1;
    }
    env.crypto().sha256(&buf).into()
}

#[contract]
pub struct RegistryContract;

#[contractimpl]
impl RegistryContract {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&ADMIN_KEY) {
            panic!("already initialized");
        }
        env.storage().instance().set(&ADMIN_KEY, &admin);
        env.storage().instance().set(&ENTRY_COUNT_KEY, &0u64);
    }

    pub fn register(env: Env, owner: Address, label: String, metadata: Bytes) -> u64 {
        owner.require_auth();
        let id = entry_count(&env);
        let entry = RegistryEntry {
            id,
            owner,
            label,
            metadata,
        };
        env.storage().persistent().set(&entry_key(&env, id), &entry);
        env.storage().instance().set(&ENTRY_COUNT_KEY, &(id + 1));
        id
    }

    pub fn get(env: Env, id: u64) -> Option<RegistryEntry> {
        read_entry(&env, id)
    }

    pub fn total(env: Env) -> u64 {
        entry_count(&env)
    }

    /// Returns a deterministic, versioned digest over a bounded page of
    /// registry entries. Identical state yields identical digests, allowing
    /// off-chain indexers to reconcile snapshots and locate drift.
    pub fn digest(env: Env, start: u64, limit: u32) -> RegistryDigestPage {
        let total = entry_count(&env);
        let bounded = if limit > MAX_DIGEST_PAGE {
            MAX_DIGEST_PAGE
        } else {
            limit
        };
        let remaining = if start >= total { 0 } else { total - start };
        let count = if (bounded as u64) > remaining {
            remaining as u32
        } else {
            bounded
        };
        let next_cursor = start + count as u64;
        let digest = compute_digest(&env, start, count, total);
        RegistryDigestPage {
            version: DIGEST_VERSION,
            start,
            count,
            total,
            next_cursor,
            digest,
        }
    }

    /// Convenience wrapper returning the same digest payload as `digest`.
    pub fn digest_page(env: Env, start: u64, limit: u32) -> RegistryDigest {
        let page = Self::digest(env, start, limit);
        RegistryDigest {
            version: page.version,
            start: page.start,
            count: page.count,
            total: page.total,
            next_cursor: page.next_cursor,
            digest: page.digest,
        }
    }
}
