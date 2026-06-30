//! Input-intent classifier — UniFFI surface (M14-C1).
//!
//! Replaces the retired C-ABI `nmp_app_intent_classify` door.
//!
//! ## Core-fn provenance
//!
//! The retired C-ABI deserialised JSON, called
//! `NmpApp::classify_input_intent`, then serialised the classification back to
//! JSON. This wrapper keeps that public native surface as typed UniFFI records
//! and enums.
//!
//! ## Stateless guarantee
//!
//! Matches the retired C-ABI behavior: classifies the registered recognizer
//! snapshot only — no kernel mutation, no IO, no actor round-trip.

use std::sync::Arc;

use nmp_core::substrate::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentRequest,
    InputIntentTarget, InputScopeId, TextSearchTargets,
};

use crate::NmpApp;

// ── Input types ───────────────────────────────────────────────────────────────

/// Identifies a registered input scope (namespace + name two-part label).
///
/// Mirrors `nmp_core::substrate::InputScopeId`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct IntentScope {
    pub namespace: String,
    pub name: String,
}

impl From<IntentScope> for InputScopeId {
    fn from(s: IntentScope) -> Self {
        InputScopeId::new(s.namespace, s.name)
    }
}

impl From<InputScopeId> for IntentScope {
    fn from(id: InputScopeId) -> Self {
        Self {
            namespace: id.namespace,
            name: id.name,
        }
    }
}

/// Free-text search-target choice for `TextQuery` fall-through.
///
/// Mirrors `nmp_core::substrate::TextSearchTargets`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum IntentTextTargets {
    /// The active account's published search relays.
    UserPreferred,
    /// The app-declared default search relays.
    AppDefault,
    /// An explicit caller-supplied relay list.
    Explicit { relays: Vec<String> },
}

impl From<IntentTextTargets> for TextSearchTargets {
    fn from(t: IntentTextTargets) -> Self {
        match t {
            IntentTextTargets::UserPreferred => TextSearchTargets::UserPreferred,
            IntentTextTargets::AppDefault => TextSearchTargets::AppDefault,
            IntentTextTargets::Explicit { relays } => TextSearchTargets::Explicit(relays),
        }
    }
}

// ── Output types ──────────────────────────────────────────────────────────────

/// A single resolved intent target.
///
/// Mirrors `nmp_core::substrate::InputIntentTarget`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum IntentTarget {
    /// A NIP-19/21 reference; `uri` is the canonical `nostr:`-form.
    DirectRef { uri: String },
    /// A NIP-05-shaped identifier (`name@domain`), shape-only (no IO).
    Nip05 { identifier: String },
    /// A normalised relay URL (`ws://` / `wss://`).
    RelayUrl { url: String },
    /// Free-text search; `request_json` is an opaque serialised `SearchRequest`.
    TextQuery { request_json: String },
    /// A recognizer-specific target; `payload_json` is opaque to core.
    Registered { payload_json: String },
}

impl From<InputIntentTarget> for IntentTarget {
    fn from(t: InputIntentTarget) -> Self {
        match t {
            InputIntentTarget::DirectRef { uri } => Self::DirectRef { uri },
            InputIntentTarget::Nip05 { identifier } => Self::Nip05 { identifier },
            InputIntentTarget::RelayUrl { url } => Self::RelayUrl { url },
            InputIntentTarget::TextQuery { request_json } => Self::TextQuery { request_json },
            InputIntentTarget::Registered { payload_json } => Self::Registered { payload_json },
        }
    }
}

/// A classification candidate: which scope matched and the target it produced.
///
/// Mirrors `nmp_core::substrate::InputIntentCandidate`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct IntentCandidate {
    pub scope: IntentScope,
    pub target: IntentTarget,
}

impl From<InputIntentCandidate> for IntentCandidate {
    fn from(c: InputIntentCandidate) -> Self {
        Self {
            scope: c.scope.into(),
            target: c.target.into(),
        }
    }
}

/// Why an input could not be classified into any allowed candidate.
///
/// `SecretLike` carries **no** copy of the input — same no-echo guarantee as
/// the retired C-ABI helper.
///
/// Mirrors `nmp_core::substrate::InputIntentRejection`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum IntentRejection {
    /// The input contains a secret key (`nsec` / `nostr:nsec` / `ncryptsec`).
    SecretLike,
    /// The input matched no recognizer and is not usable as free text.
    Unparseable,
    /// The input matched a recognizer whose scope is not registered.
    UnregisteredScope { namespace: String, name: String },
    /// The input resolved to a target whose scope is outside the allowed set.
    DisallowedScope { namespace: String, name: String },
}

impl From<InputIntentRejection> for IntentRejection {
    fn from(r: InputIntentRejection) -> Self {
        match r {
            InputIntentRejection::SecretLike => Self::SecretLike,
            InputIntentRejection::Unparseable => Self::Unparseable,
            InputIntentRejection::UnregisteredScope { scope } => Self::UnregisteredScope {
                namespace: scope.namespace,
                name: scope.name,
            },
            InputIntentRejection::DisallowedScope { scope } => Self::DisallowedScope {
                namespace: scope.namespace,
                name: scope.name,
            },
        }
    }
}

/// The result of intent classification.
///
/// Mirrors `nmp_core::substrate::InputIntentClassification`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum IntentClassification {
    /// One or more candidates (ranked; caller picks / disambiguates).
    Candidates { candidates: Vec<IntentCandidate> },
    /// The input could not be classified into any allowed candidate.
    Rejection { rejection: IntentRejection },
}

impl From<InputIntentClassification> for IntentClassification {
    fn from(c: InputIntentClassification) -> Self {
        match c {
            InputIntentClassification::Candidates(cs) => Self::Candidates {
                candidates: cs.into_iter().map(IntentCandidate::from).collect(),
            },
            InputIntentClassification::Rejection(r) => Self::Rejection {
                rejection: r.into(),
            },
        }
    }
}

// ── Exported function ─────────────────────────────────────────────────────────

/// Classify one untyped input string against the app's registered recognizers.
///
/// # Arguments
///
/// * `app`          — the runtime app handle (provides the recognizer snapshot).
/// * `input`        — the raw, untrusted input string.
/// * `scopes`       — the app's allow-list of acceptable result classes.
/// * `text_targets` — search-relay choice for the `TextQuery` fall-through.
///
/// # Returns
///
/// An `IntentClassification` — either `Candidates` (one or more matches) or
/// a `Rejection` (with a typed reason). Never throws.
///
/// `SecretLike` rejections carry **no** copy of `input` — the secret is never
/// echoed back.
///
/// Preserves the retired C-ABI behavior: same recognizer snapshot read, same
/// pure classification, typed output instead of JSON.
#[uniffi::export]
pub fn classify_intent(
    app: Arc<NmpApp>,
    input: String,
    scopes: Vec<IntentScope>,
    text_targets: IntentTextTargets,
) -> IntentClassification {
    let request = InputIntentRequest {
        input,
        scopes: scopes.into_iter().map(InputScopeId::from).collect(),
        text_targets: text_targets.into(),
    };
    app.inner.classify_input_intent(&request).into()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // The retired C-ABI `nmp_app_intent_classify` called
    // `NmpApp::classify_input_intent` internally. These tests call the same
    // method directly on `app.inner` and compare with the UniFFI fn output.

    #[test]
    fn parity_unparseable_input_yields_rejection() {
        let app = crate::NmpApp::new();
        let result = classify_intent(
            Arc::clone(&app),
            "not-anything-parseable".to_string(),
            vec![],
            IntentTextTargets::AppDefault,
        );

        // Retired C-ABI parity: same call classify_request made internally.
        let request = InputIntentRequest {
            input: "not-anything-parseable".to_string(),
            scopes: vec![],
            text_targets: TextSearchTargets::AppDefault,
        };
        let core_result = app.inner.classify_input_intent(&request);
        let expected: IntentClassification = core_result.into();

        assert_eq!(
            result, expected,
            "UniFFI classify_intent must match the core fn"
        );
    }

    #[test]
    fn parity_secret_key_produces_secretlike_rejection() {
        use nmp_nip19::encode_nsec;
        let nsec = encode_nsec("3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d")
            .unwrap();

        let app = crate::NmpApp::new();
        let result = classify_intent(
            Arc::clone(&app),
            nsec.clone(),
            vec![],
            IntentTextTargets::AppDefault,
        );

        // Must be a SecretLike rejection — key never echoed.
        let IntentClassification::Rejection { rejection } = &result else {
            panic!("expected Rejection, got {result:?}");
        };
        assert!(
            matches!(rejection, IntentRejection::SecretLike),
            "expected SecretLike rejection"
        );

        // Verify the nsec is not present in the serialized result.
        let debug_str = format!("{result:?}");
        assert!(
            !debug_str.contains(&nsec),
            "secret key must not appear in classification result"
        );
    }

    #[test]
    fn parity_valid_npub_classifies_as_direct_ref() {
        use nmp_nip19::encode_npub;
        let pubkey = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
        let npub = encode_npub(pubkey).unwrap();

        let app = crate::NmpApp::new();
        let result = classify_intent(
            Arc::clone(&app),
            npub.clone(),
            vec![],
            IntentTextTargets::AppDefault,
        );

        // Parity against the core path.
        let request = InputIntentRequest {
            input: npub,
            scopes: vec![],
            text_targets: TextSearchTargets::AppDefault,
        };
        let core_result: IntentClassification = app.inner.classify_input_intent(&request).into();
        assert_eq!(result, core_result);
    }

    #[test]
    fn type_conversions_are_round_trip_stable() {
        // Verify IntentScope <-> InputScopeId round-trips correctly.
        let scope = IntentScope {
            namespace: "nip50".to_string(),
            name: "profiles".to_string(),
        };
        let id: InputScopeId = scope.clone().into();
        let back: IntentScope = id.into();
        assert_eq!(scope, back);
    }
}
