//! Read-only, paged inventory of the protocol roles (`list_role_members`).
//!
//! The verifier, moderator and settler grants are stored per address, so they
//! cannot be enumerated on their own. This module keeps one append-ordered
//! member list per role in instance storage, maintained by the existing
//! `add_*` / `remove_*` entry points. The admin role is read straight from
//! `DataKey::Admin`.
//!
//! Limits: at most [`MAX_ROLE_MEMBERS`] members are tracked per role (a grant
//! beyond that fails with `BatchTooLarge`, the error enum being full) and one
//! page returns at most [`ROLE_PAGE_CAP`] addresses. Grants made before this
//! index existed are not listed until they are re-granted; `is_verifier`,
//! `is_moderator` and `is_settler` stay authoritative for membership.

use soroban_sdk::{contracttype, Address, Env, Vec};

use crate::{DataKey, Error};

/// Maximum addresses tracked per role.
pub const MAX_ROLE_MEMBERS: u32 = 50;
/// Maximum addresses returned by one `list_role_members` call.
pub const ROLE_PAGE_CAP: u32 = 20;

/// Protocol roles that can be enumerated.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RoleKind {
    Admin,
    Verifier,
    Moderator,
    Settler,
}

/// One deterministic page of role members.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RolePage {
    /// Members in grant order.
    pub members: Vec<Address>,
    /// Cursor for the next page, or `None` at the end of the list.
    pub next_cursor: Option<u32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum RoleIndexKey {
    Members(RoleKind),
}

fn members(env: &Env, role: RoleKind) -> Vec<Address> {
    env.storage()
        .instance()
        .get(&RoleIndexKey::Members(role))
        .unwrap_or(Vec::new(env))
}

/// Record a grant. Idempotent; fails once `MAX_ROLE_MEMBERS` is reached.
pub(crate) fn track_grant(env: &Env, role: RoleKind, who: &Address) -> Result<(), Error> {
    let mut list = members(env, role);
    if list.contains(who) {
        return Ok(());
    }
    if list.len() >= MAX_ROLE_MEMBERS {
        return Err(Error::BatchTooLarge);
    }
    list.push_back(who.clone());
    env.storage()
        .instance()
        .set(&RoleIndexKey::Members(role), &list);
    crate::VaultRegistry::bump_instance(env);
    Ok(())
}

/// Record a revocation. A no-op when `who` is not tracked.
pub(crate) fn track_revoke(env: &Env, role: RoleKind, who: &Address) {
    let mut list = members(env, role);
    if let Some(index) = list.first_index_of(who) {
        list.remove(index);
        env.storage()
            .instance()
            .set(&RoleIndexKey::Members(role), &list);
        crate::VaultRegistry::bump_instance(env);
    }
}

/// Page through the members of `role`. `limit` is capped at `ROLE_PAGE_CAP`;
/// a cursor past the end yields an empty page.
pub(crate) fn page(env: &Env, role: RoleKind, cursor: u32, limit: u32) -> RolePage {
    let all: Vec<Address> = match role {
        RoleKind::Admin => {
            let mut one: Vec<Address> = Vec::new(env);
            let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
            if let Some(admin) = admin {
                one.push_back(admin);
            }
            one
        }
        _ => members(env, role),
    };
    let total = all.len();
    let size = limit.min(ROLE_PAGE_CAP);
    let mut out: Vec<Address> = Vec::new(env);
    let mut i = cursor;
    while i < total && out.len() < size {
        out.push_back(all.get(i).unwrap());
        i += 1;
    }
    let next_cursor = if i < total { Some(i) } else { None };
    RolePage {
        members: out,
        next_cursor,
    }
}
