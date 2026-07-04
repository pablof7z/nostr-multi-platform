//! Request / target / classification value types handed across the
//! recognizer boundary. All noun-free: protocol specifics ride as opaque JSON
//! or already-typed generic fields.

use serde::{Deserialize, Serialize};

use super::id::InputScopeId;
use super::recognizer::TextSearchTargets;

/// The classification request: one raw input string, the app's set of requested
/// scopes (which classes of result the calling surface is willing to accept),
/// and the free-text search-target choice to use when the input falls through to
/// a text query.
///
/// `scopes` is the app's allow-list. The synthetic [`InputScopeId::nostr_ref`]
/// is always implicitly allowed for a *valid* reference, but a ref whose target
/// class is not in `scopes` is refused with
/// [`InputIntentRejection::DisallowedScope`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputIntentRequest {
    /// The raw, untrusted input. NOTE: a secret (`nsec…`) detected here is
    /// rejected and never copied into the rejection — see
    /// [`InputIntentRejection::SecretLike`].
    pub input: String,
    /// App-requested scopes (the allow-list of acceptable result classes).
    pub scopes: Vec<InputScopeId>,
    /// Free-text search-target choice used for the `TextQuery` fall-through.
    pub text_targets: TextSearchTargets,
}

/// A single resolved intent target. Noun-free: protocol-specific recognizers
/// produce the `Registered` variant carrying opaque JSON; the generic core
/// parsers produce the typed variants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputIntentTarget {
    /// A NIP-19/21 reference the app should open directly (open-uri). `uri` is
    /// the canonical `nostr:`-form URI that [`crate::resolve_open_uri`] accepts.
    DirectRef { uri: String },
    /// A NIP-05-shaped identifier (`name@domain`). SHAPE ONLY — the dispatch
    /// layer performs the HTTP `.well-known/nostr.json` reverse lookup; classify
    /// never does IO.
    Nip05 { identifier: String },
    /// A NIP-AD candidate: an ordinary `http(s)://<domain>/<path>` web URL that
    /// may double as a pointer to Nostr events (#2927). SHAPE ONLY — classify
    /// never does IO; the dispatch layer performs the
    /// `.well-known/nostr.json?ad=<path>` fetch. Emitted ALONGSIDE the free-text
    /// candidates for the same input so an app can search in parallel (D1) while
    /// an AD resolution is attempted.
    AdCandidate { url: String },
    /// A normalized relay URL (`ws://` / `wss://`).
    RelayUrl { url: String },
    /// Free-text search. `request_json` is an opaque, already-serialized
    /// `nmp_nip50::SearchRequest` so core stays noun-free (it never imports
    /// nmp-nip50).
    TextQuery { request_json: String },
    /// A recognizer-specific target. `payload_json` is opaque to core; the
    /// dispatch layer routes it back to the producing recognizer's scope.
    Registered { payload_json: String },
}

/// A classification candidate: which scope matched, and the target it produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputIntentCandidate {
    pub scope: InputScopeId,
    pub target: InputIntentTarget,
}

/// Why an input could not be classified into any allowed candidate.
///
/// `SecretLike` deliberately carries **no** copy of the input — a detected
/// secret is never logged, stored, or echoed back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputIntentRejection {
    /// The input is (or contains) a secret key (`nsec` / `nostr:nsec` /
    /// `ncryptsec`). Carries nothing.
    SecretLike,
    /// The input matched no recognizer and is not usable as free text.
    Unparseable,
    /// The input matched a recognizer whose scope is not registered with the
    /// kernel.
    UnregisteredScope { scope: InputScopeId },
    /// The input resolved to a valid target whose scope is not in the app's
    /// requested `scopes` allow-list (e.g. a valid `naddr` ref when the app only
    /// requested profile scopes).
    DisallowedScope { scope: InputScopeId },
}

/// The outcome of [`nmp_intent::classify`]: either one or more candidates (the
/// caller picks / disambiguates) or a single rejection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputIntentClassification {
    Candidates(Vec<InputIntentCandidate>),
    Rejection(InputIntentRejection),
}
