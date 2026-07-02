//! NIP-29 input-scope recognizer — claims the `nip29.groups` input scope.
//!
//! # What this does
//!
//! A single `GroupInputScopeRecognizer` implements
//! [`nmp_core::substrate::InputScopeRecognizer`] for scope id `nip29.groups`.
//! It accepts two kinds of already-decoded input:
//!
//! 1. **NIP-29 URI form** — `<host>'<local-id>` (e.g. `groups.nostr.com'abc-123`)
//!    detected in a [`ResolvedInputKind::FreeText`] that matches
//!    [`GroupId::from_uri`]. This is the canonical shareable group identifier
//!    form defined by the NIP-29 spec.
//!
//! 2. **NIP-19 `naddr` reference** — a [`ResolvedInputKind::Reference`] whose
//!    `entity_class` is `"address"`. An `naddr` pointing to a kind:39000 group
//!    contains the host relay and the group slug; the dispatch layer resolves the
//!    naddr further, so the recognizer passes the `uri` through as a
//!    `DirectRef` target, not a `Registered` one.
//!
//! `text_candidate()` returns `None` — free-text group search is handled by the
//! FTS scope in [`crate::search`] (NIP-50 on the FTS index), not here.
//!
//! # Payload JSON shape
//!
//! When the recognizer claims a NIP-29 URI input it returns
//! [`InputIntentTarget::Registered`] carrying a [`GroupIdentPayload`] serialized
//! as JSON:
//!
//! ```json
//! { "host_relay_url": "wss://groups.nostr.com", "local_id": "abc-123" }
//! ```
//!
//! The dispatch layer (shell / FFI) deserializes this back into a [`GroupId`] and
//! routes to the hydrating NIP-29 typed read-session lane (#2088).
//!
//! # D0 compliance
//!
//! This file lives in `nmp-nip29`; `nmp-core` gains zero NIP-29 nouns. The
//! `InputScopeRecognizer` trait is noun-free and lives in
//! `nmp_core::substrate::intent`.
//!
//! # Registration
//!
//! Call [`register_input_scopes`] once during host composition (before
//! `nmp_app_start`). Duplicate scope ids yield to the existing registration
//! (ADR-0069 — first wins).

use std::sync::Arc;

use nmp_core::substrate::{
    InputIntentTarget, InputScopeId, InputScopeRecognizer, InputScopeRegistrar, ResolvedInput,
    ResolvedInputKind, TextSearchTargets,
};
use serde::{Deserialize, Serialize};

use crate::group_id::GroupId;

/// Stable label for the NIP-29 group input scope.
///
/// Matches [`crate::search::GROUP_SEARCH_SCOPE_LABEL`] by namespace convention
/// but lives in a SEPARATE registry (the [`nmp_core::substrate::InputScopeRegistry`],
/// not the FTS [`nmp_core::substrate::SearchScopeRegistry`]).
pub const GROUP_INPUT_SCOPE_LABEL: &str = "nip29.groups";

/// The JSON payload carried in [`InputIntentTarget::Registered`] when this
/// recognizer claims an input.
///
/// Serialized as `{"host_relay_url": "wss://…", "local_id": "…"}`. The
/// dispatch layer (shell / FFI) deserializes back into a [`GroupId`] and
/// routes to the group-open lane.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupIdentPayload {
    /// The `wss://`-prefixed host relay URL.
    pub host_relay_url: String,
    /// The NIP-29 local id (`[a-z0-9-_]+`).
    pub local_id: String,
}

impl GroupIdentPayload {
    fn from_group_id(id: &GroupId) -> Self {
        Self {
            host_relay_url: id.host_relay_url.clone(),
            local_id: id.local_id.clone(),
        }
    }
}

/// NIP-29 group input recognizer.
///
/// Claims the `nip29.groups` input scope and accepts:
/// - NIP-29 URI form (`host'local-id`) in free-text input.
/// - `naddr` references (entity_class `"address"`) — passed through as
///   [`InputIntentTarget::DirectRef`] so the dispatch layer can open the
///   group via the normal open-uri path.
#[derive(Clone, Copy, Debug, Default)]
pub struct GroupInputScopeRecognizer;

impl GroupInputScopeRecognizer {
    /// Construct the recognizer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl InputScopeRecognizer for GroupInputScopeRecognizer {
    fn scope(&self) -> InputScopeId {
        InputScopeId::new("nip29", "groups")
    }

    /// Inspect already-decoded input and claim it if it is a NIP-29 group
    /// reference.
    ///
    /// - `FreeText` that parses as a NIP-29 URI (`host'local-id`) via
    ///   [`GroupId::from_uri`] → `Registered { payload_json }` carrying a
    ///   [`GroupIdentPayload`].
    /// - `Reference { entity_class: "address", .. }` (naddr / nostr:naddr…) →
    ///   `DirectRef { uri }` so the shell opens the group via the open-uri lane.
    ///   The recognizer does NOT inspect the inner kind number (no IO needed;
    ///   the open-uri handler resolves the full naddr).
    /// - Everything else → `None`.
    fn recognize(&self, input: &ResolvedInput) -> Option<InputIntentTarget> {
        match &input.kind {
            ResolvedInputKind::FreeText { text } => {
                let group_id = GroupId::from_uri(text)?;
                let payload = GroupIdentPayload::from_group_id(&group_id);
                let payload_json = serde_json::to_string(&payload).ok()?;
                Some(InputIntentTarget::Registered { payload_json })
            }
            ResolvedInputKind::Reference { uri, entity_class } if entity_class == "address" => {
                // An naddr ref: pass it through to the open-uri lane. The
                // dispatch layer resolves the full naddr (kind, relay, pubkey,
                // d-tag) and routes to the group. We do not attempt to decode
                // the naddr bytes here (no IO, pure recognizer).
                Some(InputIntentTarget::DirectRef { uri: uri.clone() })
            }
            _ => None,
        }
    }

    /// NIP-29 groups are not free-text-searched here; the FTS scope
    /// (`nip29.groups` in the [`nmp_core::substrate::SearchScopeRegistry`])
    /// handles that separately.
    fn text_candidate(
        &self,
        _free_text: &str,
        _targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget> {
        None
    }
}

/// Register the NIP-29 group input scope against `host`.
///
/// Composition-root house style (ADR-0069 / ADR-0069 — no linkme/inventory):
/// a host calls this one-liner during composition to add the `nip29.groups`
/// input recognizer. A duplicate scope id yields to the existing registration
/// (first wins).
pub(crate) fn register_input_scopes(host: &impl InputScopeRegistrar) {
    host.register_input_scope(Arc::new(GroupInputScopeRecognizer::new()));
}

#[cfg(test)]
#[path = "input_scope/tests.rs"]
mod tests;
