//! Complete actor / previous / replacement payloads for admin-role events (#29).
//!
//! Before this module the admin and recovery-admin events carried only the new
//! account (or, for `recover`, an untyped `(old, new)` tuple), so an indexer
//! could not tell who authorized a change or which account it replaced. Every
//! admin-role mutation now publishes a [`RoleRotation`].
//!
//! Verifier recovery rotations (`rotate_verifier` / `reset_verifier`) emit
//! `rmverif`, `addverif` (`RoleChange`), and `verrot` (`VerifierRotation`) in
//! `lib.rs` so indexers can audit compromised verifier key replacements.

use soroban_sdk::{contracttype, Address, Env, Symbol};

/// Payload of the `setadmin`, `nomadmin`, `accadmin`, `setrecov` and `recover`
/// events.
///
/// - `actor`: the account whose authorization performed the change.
/// - `previous`: the account holding the role slot before the change, if any
///   (the current admin for `nomadmin`, the old admin for `accadmin` and
///   `recover`, the old recovery admin for `setrecov`).
/// - `replacement`: the account that holds, or was nominated for, the slot
///   afterwards; `None` when the slot was cleared (`set_recovery_admin(None)`).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct RoleRotation {
    pub actor: Address,
    pub previous: Option<Address>,
    pub replacement: Option<Address>,
}

/// Publish `RoleRotation` under the single-topic event `topic`.
pub(crate) fn publish_rotation(
    env: &Env,
    topic: Symbol,
    actor: Address,
    previous: Option<Address>,
    replacement: Option<Address>,
) {
    env.events().publish(
        (topic,),
        RoleRotation {
            actor,
            previous,
            replacement,
        },
    );
}
