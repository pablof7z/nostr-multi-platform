//! The recognizer trait + the generic [`ResolvedInput`] the orchestrator hands
//! it (so recognizers never re-parse), plus the noun-free mirror of the
//! free-text target choice.

use serde::{Deserialize, Serialize};

use super::id::InputScopeId;
use super::target::InputIntentTarget;

/// Noun-free mirror of the NIP-50 `SearchTargets` choice. Core never imports
/// `nmp-nip50`; the orchestrator translates this into the real
/// `nmp_nip50::SearchTargets` when it builds the opaque `TextQuery` JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextSearchTargets {
    /// The active account's preferred (published) search relays.
    UserPreferred,
    /// The app-declared default search relays.
    AppDefault,
    /// An explicit caller-provided relay list.
    Explicit(Vec<String>),
}

/// The already-decoded class of an input, produced by the orchestrator's
/// generic parsing pass and handed to recognizers so they do not re-parse.
///
/// Noun-free: each variant is a generic field, not a protocol concept. A
/// recognizer inspects this to decide whether the input belongs to its scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolvedInputKind {
    /// A NIP-19/21 reference that the generic core parser already validated and
    /// canonicalized to a `nostr:`-form URI (via [`crate::resolve_open_uri`]'s
    /// decoder). `entity_class` is a generic discriminant the producing parser
    /// fills (e.g. `"profile"`, `"event"`, `"address"`) so a recognizer can
    /// scope-match without re-decoding.
    Reference { uri: String, entity_class: String },
    /// A normalized relay URL (`ws://` / `wss://`).
    RelayUrl { url: String },
    /// A NIP-05-shaped identifier (`name@domain`), SHAPE-validated only (no IO).
    Nip05Shape { identifier: String },
    /// Free text — nothing structural matched.
    FreeText { text: String },
}

/// The orchestrator's parse output handed to every recognizer. Carries the raw
/// input (trimmed) plus its already-decoded [`ResolvedInputKind`], so a
/// recognizer's [`InputScopeRecognizer::recognize`] is a pure inspection, never
/// a re-parse.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedInput {
    /// The trimmed raw input.
    pub raw: String,
    /// The decoded class of the input.
    pub kind: ResolvedInputKind,
}

/// A crate-registered input recognizer. Protocol / app crates implement this to
/// claim one [`InputScopeId`] and turn already-decoded generic input into a
/// scope-specific [`InputIntentTarget`].
///
/// All three methods are PURE + SYNC + side-effect-free (no IO): they run inside
/// `classify`. IO (NIP-05 HTTP, search REQs) happens only in the dispatch layer.
pub trait InputScopeRecognizer: Send + Sync {
    /// The scope this recognizer claims.
    fn scope(&self) -> InputScopeId;

    /// Inspect already-decoded input. Return `Some(target)` if this recognizer
    /// claims it for its scope, else `None`. Pure — never re-parses, never does
    /// IO.
    fn recognize(&self, input: &ResolvedInput) -> Option<InputIntentTarget>;

    /// Offer a target for free text (the search fall-through). Recognizers that
    /// can turn a bare query into their scope (e.g. text-search) return
    /// `Some(target)`; others return `None`. Pure — no IO.
    fn text_candidate(
        &self,
        free_text: &str,
        targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget>;
}
