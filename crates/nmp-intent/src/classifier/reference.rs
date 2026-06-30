//! Rung 2 — NIP-19/21 reference recognition + entity-class allow-list (#1804).
//!
//! A valid reference is validated/classified through the existing pure decoder
//! [`nmp_core::resolve_open_uri`] (it accepts both `nostr:`-prefixed and bare
//! bech32, and rejects nsec — already filtered by rung 1). The ref is then gated
//! by the app's requested scopes on its generic entity class; a class the app
//! did not request is refused with `DisallowedScope`. Pure — no IO.

use nmp_core::substrate::{
    InputIntentClassification, InputIntentRejection, InputIntentTarget, InputScopeId,
};

use super::{one, reject, NIP50_NAMESPACE};

const REF_CLASS_PROFILE: &str = "profile";
const REF_CLASS_EVENT: &str = "event";
const REF_CLASS_ADDRESS: &str = "address";

/// If `input` is a valid NIP-19/21 reference, return its generic entity class
/// (`"profile"` / `"event"` / `"address"`); else `None`.
pub(super) fn reference_entity_class(input: &str) -> Option<&'static str> {
    use nmp_nostr_id::Nip19Entity;
    // `resolve_open_uri` proves routability; the class comes from the same
    // decoder it uses.
    if nmp_core::resolve_open_uri(input).is_err() {
        return None;
    }
    let body = input.strip_prefix("nostr:").unwrap_or(input);
    match nmp_nostr_id::parse(body).ok()? {
        Nip19Entity::Npub(_) | Nip19Entity::Nprofile(_) => Some(REF_CLASS_PROFILE),
        Nip19Entity::Note(_) | Nip19Entity::Nevent(_) => Some(REF_CLASS_EVENT),
        Nip19Entity::Naddr(_) => Some(REF_CLASS_ADDRESS),
        // nsec is filtered by rung 1; not a routable reference.
        Nip19Entity::Nsec(_) => None,
    }
}

/// Emit the `DirectRef` candidate (under the synthetic `nostr.ref` scope) iff the
/// ref's entity class is allowed by `scopes`; otherwise `DisallowedScope`.
pub(super) fn classify_reference(
    input: &str,
    entity_class: &str,
    scopes: &[InputScopeId],
) -> InputIntentClassification {
    if reference_class_allowed(entity_class, scopes) {
        one(
            InputScopeId::nostr_ref(),
            InputIntentTarget::DirectRef {
                uri: input.to_string(),
            },
        )
    } else {
        reject(InputIntentRejection::DisallowedScope {
            scope: InputScopeId::nostr_ref(),
        })
    }
}

/// A ref's entity class is allowed when the app explicitly requested the
/// synthetic `nostr.ref` scope, OR requested a `nip50.*` scope that covers the
/// class (`profile`→`profiles`, `event`→`notes`, `address`→`longform`).
fn reference_class_allowed(entity_class: &str, scopes: &[InputScopeId]) -> bool {
    if scopes.contains(&InputScopeId::nostr_ref()) {
        return true;
    }
    let covering = class_covering_scope_name(entity_class);
    scopes
        .iter()
        .any(|s| s.namespace == NIP50_NAMESPACE && covering == Some(s.name.as_str()))
}

/// The `nip50.*` scope `name` that covers a ref entity class.
fn class_covering_scope_name(entity_class: &str) -> Option<&'static str> {
    match entity_class {
        REF_CLASS_PROFILE => Some("profiles"),
        REF_CLASS_EVENT => Some("notes"),
        REF_CLASS_ADDRESS => Some("longform"),
        _ => None,
    }
}
