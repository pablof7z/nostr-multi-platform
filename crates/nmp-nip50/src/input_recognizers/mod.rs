//! NIP-50 [`InputScopeRecognizer`] implementations (issue #1804, S4).
//!
//! Provides three recognizers that cover the three public FTS scopes:
//!
//! - `ProfilesInputRecognizer` — scope `nip50.profiles` → `SearchScope::Users`
//! - `NotesInputRecognizer`    — scope `nip50.notes`    → `SearchScope::Kinds([1])`
//! - `LongFormInputRecognizer` — scope `nip50.longform` → `SearchScope::LongForm`
//!
//! All recognizers are **free-text-only**: `recognize()` always returns `None`
//! (NIP-19/21 references, relay URLs, and NIP-05 shapes are handled upstream by
//! the generic precedence cascade in `nmp-intent`; no pattern-claiming here).
//! `text_candidate()` builds the correct `SearchRequest` and returns a
//! `TextQuery` carrying the opaque JSON.
//!
//! Registration entry-point: [`register_input_scopes`].

use std::sync::Arc;

use nmp_core::substrate::{
    InputIntentTarget, InputScopeId, InputScopeRecognizer, InputScopeRegistrar, ResolvedInput,
    TextSearchTargets,
};
use serde_json;

use crate::request::{SearchRequest, SearchScope, SearchTargets};
use crate::scopes::{SCOPE_LABEL_LONGFORM, SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES};

// ─── helpers ────────────────────────────────────────────────────────────────

/// Translate the noun-free `TextSearchTargets` from core into the concrete
/// `SearchTargets` from this crate. The two types are structurally identical;
/// core can never import nmp-nip50, so the translation lives here.
fn translate_targets(t: &TextSearchTargets) -> SearchTargets {
    match t {
        TextSearchTargets::UserPreferred => SearchTargets::UserPreferred,
        TextSearchTargets::AppDefault => SearchTargets::AppDefault,
        TextSearchTargets::Explicit(relays) => SearchTargets::Explicit(relays.clone()),
    }
}

/// Serialize a `SearchRequest` into a `TextQuery` target, returning `None` if
/// the query is empty / rejected by `SearchRequest::new`.
fn make_text_query(
    free_text: &str,
    scope: SearchScope,
    targets: &TextSearchTargets,
) -> Option<InputIntentTarget> {
    let request = SearchRequest::new(free_text, scope, translate_targets(targets), None)?;
    let request_json = serde_json::to_string(&request).ok()?;
    Some(InputIntentTarget::TextQuery { request_json })
}

// ─── profiles recognizer ────────────────────────────────────────────────────

/// Recognizer for the `nip50.profiles` input scope.
///
/// Scope id: `InputScopeId::new("nip50", "profiles")` (== `SCOPE_LABEL_PROFILES`).
/// `recognize()` returns `None` — only free-text queries target this scope.
/// `text_candidate()` wraps the query in a `SearchRequest{ scope: Users, … }`.
pub struct ProfilesInputRecognizer;

impl ProfilesInputRecognizer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProfilesInputRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl InputScopeRecognizer for ProfilesInputRecognizer {
    fn scope(&self) -> InputScopeId {
        // "nip50.profiles" — must match SCOPE_LABEL_PROFILES
        debug_assert_eq!(
            InputScopeId::new("nip50", "profiles").label(),
            SCOPE_LABEL_PROFILES
        );
        InputScopeId::new("nip50", "profiles")
    }

    fn recognize(&self, _input: &ResolvedInput) -> Option<InputIntentTarget> {
        // Free-text scope only — no pattern to claim.
        None
    }

    fn text_candidate(
        &self,
        free_text: &str,
        targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget> {
        make_text_query(free_text, SearchScope::Users, targets)
    }
}

// ─── notes recognizer ───────────────────────────────────────────────────────

/// Recognizer for the `nip50.notes` input scope.
///
/// Scope id: `InputScopeId::new("nip50", "notes")` (== `SCOPE_LABEL_NOTES`).
/// `recognize()` returns `None`.
/// `text_candidate()` wraps the query in a `SearchRequest{ scope: Kinds([1]), … }`.
pub struct NotesInputRecognizer;

impl NotesInputRecognizer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NotesInputRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl InputScopeRecognizer for NotesInputRecognizer {
    fn scope(&self) -> InputScopeId {
        debug_assert_eq!(
            InputScopeId::new("nip50", "notes").label(),
            SCOPE_LABEL_NOTES
        );
        InputScopeId::new("nip50", "notes")
    }

    fn recognize(&self, _input: &ResolvedInput) -> Option<InputIntentTarget> {
        None
    }

    fn text_candidate(
        &self,
        free_text: &str,
        targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget> {
        use std::collections::BTreeSet;
        make_text_query(
            free_text,
            SearchScope::Kinds(BTreeSet::from([1u32])),
            targets,
        )
    }
}

// ─── long-form recognizer ────────────────────────────────────────────────────

/// Recognizer for the `nip50.longform` input scope.
///
/// Scope id: `InputScopeId::new("nip50", "longform")` (== `SCOPE_LABEL_LONGFORM`).
/// `recognize()` returns `None`.
/// `text_candidate()` wraps the query in a `SearchRequest{ scope: LongForm, … }`.
pub struct LongFormInputRecognizer;

impl LongFormInputRecognizer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LongFormInputRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl InputScopeRecognizer for LongFormInputRecognizer {
    fn scope(&self) -> InputScopeId {
        debug_assert_eq!(
            InputScopeId::new("nip50", "longform").label(),
            SCOPE_LABEL_LONGFORM
        );
        InputScopeId::new("nip50", "longform")
    }

    fn recognize(&self, _input: &ResolvedInput) -> Option<InputIntentTarget> {
        None
    }

    fn text_candidate(
        &self,
        free_text: &str,
        targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget> {
        make_text_query(free_text, SearchScope::LongForm, targets)
    }
}

// ─── registration ────────────────────────────────────────────────────────────

/// Register the three NIP-50 input-scope recognizers (profiles, notes,
/// long-form) against `host`. Narrow surface (D6): takes `&impl
/// InputScopeRegistrar`, never the whole `AppHost`. Called from the
/// composition root after [`crate::scopes::register_search_scopes`].
pub fn register_input_scopes(host: &impl InputScopeRegistrar) {
    host.register_input_scope(Arc::new(ProfilesInputRecognizer::new()));
    host.register_input_scope(Arc::new(NotesInputRecognizer::new()));
    host.register_input_scope(Arc::new(LongFormInputRecognizer::new()));
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
