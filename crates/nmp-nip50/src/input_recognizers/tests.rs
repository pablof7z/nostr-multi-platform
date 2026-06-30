//! Unit tests for the NIP-50 [`InputScopeRecognizer`] implementations.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{InputIntentTarget, ResolvedInput, TextSearchTargets};

use super::{
    register_input_scopes, LongFormInputRecognizer, NotesInputRecognizer, ProfilesInputRecognizer,
};
use crate::request::{SearchRequest, SearchScope, SearchTargets, DEFAULT_MAX_SEARCH_HITS};
use crate::scopes::{SCOPE_LABEL_LONGFORM, SCOPE_LABEL_NOTES, SCOPE_LABEL_PROFILES};
use nmp_core::substrate::InputScopeRecognizer;

fn decode_request(target: Option<InputIntentTarget>) -> SearchRequest {
    let Some(InputIntentTarget::TextQuery { request_json }) = target else {
        panic!("expected TextQuery, got {:?}", target);
    };
    serde_json::from_str::<SearchRequest>(&request_json).expect("valid SearchRequest JSON")
}

// ── profiles ──

#[test]
fn profiles_text_candidate_users_scope_user_preferred() {
    let r = ProfilesInputRecognizer::new();
    let target = r.text_candidate("alice", &TextSearchTargets::UserPreferred);
    let req = decode_request(target);
    assert_eq!(req.query, "alice");
    assert_eq!(req.scope, SearchScope::Users);
    assert_eq!(req.targets, SearchTargets::UserPreferred);
    assert_eq!(req.max_hits, DEFAULT_MAX_SEARCH_HITS);
}

#[test]
fn profiles_text_candidate_users_scope_app_default() {
    let r = ProfilesInputRecognizer::new();
    let target = r.text_candidate("bob", &TextSearchTargets::AppDefault);
    let req = decode_request(target);
    assert_eq!(req.scope, SearchScope::Users);
    assert_eq!(req.targets, SearchTargets::AppDefault);
}

#[test]
fn profiles_text_candidate_users_scope_explicit_relays() {
    let r = ProfilesInputRecognizer::new();
    let relays = vec!["wss://relay.example.com".to_string()];
    let target = r.text_candidate("carol", &TextSearchTargets::Explicit(relays.clone()));
    let req = decode_request(target);
    assert_eq!(req.scope, SearchScope::Users);
    assert_eq!(req.targets, SearchTargets::Explicit(relays));
}

#[test]
fn profiles_recognize_always_none() {
    let r = ProfilesInputRecognizer::new();
    let input = ResolvedInput {
        raw: "alice".to_string(),
        kind: nmp_core::substrate::ResolvedInputKind::FreeText {
            text: "alice".to_string(),
        },
    };
    assert!(r.recognize(&input).is_none());
}

#[test]
fn profiles_empty_query_returns_none() {
    let r = ProfilesInputRecognizer::new();
    assert!(r
        .text_candidate("   ", &TextSearchTargets::UserPreferred)
        .is_none());
}

#[test]
fn profiles_scope_id_matches_label_const() {
    let r = ProfilesInputRecognizer::new();
    assert_eq!(r.scope().label(), SCOPE_LABEL_PROFILES);
}

// ── notes ──

#[test]
fn notes_text_candidate_kinds_scope() {
    let r = NotesInputRecognizer::new();
    let target = r.text_candidate("hello world", &TextSearchTargets::UserPreferred);
    let req = decode_request(target);
    assert_eq!(req.query, "hello world");
    assert_eq!(req.scope, SearchScope::Kinds(BTreeSet::from([1u32])));
    assert_eq!(req.targets, SearchTargets::UserPreferred);
}

#[test]
fn notes_recognize_always_none() {
    let r = NotesInputRecognizer::new();
    let input = ResolvedInput {
        raw: "hello".to_string(),
        kind: nmp_core::substrate::ResolvedInputKind::FreeText {
            text: "hello".to_string(),
        },
    };
    assert!(r.recognize(&input).is_none());
}

#[test]
fn notes_scope_id_matches_label_const() {
    let r = NotesInputRecognizer::new();
    assert_eq!(r.scope().label(), SCOPE_LABEL_NOTES);
}

// ── long-form ──

#[test]
fn longform_text_candidate_longform_scope() {
    let r = LongFormInputRecognizer::new();
    let target = r.text_candidate("rust programming", &TextSearchTargets::AppDefault);
    let req = decode_request(target);
    assert_eq!(req.query, "rust programming");
    assert_eq!(req.scope, SearchScope::LongForm);
    assert_eq!(req.targets, SearchTargets::AppDefault);
}

#[test]
fn longform_recognize_always_none() {
    let r = LongFormInputRecognizer::new();
    let input = ResolvedInput {
        raw: "rust".to_string(),
        kind: nmp_core::substrate::ResolvedInputKind::FreeText {
            text: "rust".to_string(),
        },
    };
    assert!(r.recognize(&input).is_none());
}

#[test]
fn longform_scope_id_matches_label_const() {
    let r = LongFormInputRecognizer::new();
    assert_eq!(r.scope().label(), SCOPE_LABEL_LONGFORM);
}

// ── registration ──

#[test]
fn register_input_scopes_installs_three_recognizers() {
    use nmp_core::substrate::InputScopeRegistry;

    let registry = InputScopeRegistry::new();
    register_input_scopes(&registry);
    assert_eq!(registry.len(), 3);

    let recognizers = registry.recognizers();
    let scope_labels: Vec<String> = recognizers.iter().map(|r| r.scope().label()).collect();
    assert!(scope_labels.contains(&SCOPE_LABEL_PROFILES.to_string()));
    assert!(scope_labels.contains(&SCOPE_LABEL_NOTES.to_string()));
    assert!(scope_labels.contains(&SCOPE_LABEL_LONGFORM.to_string()));
}

#[test]
fn register_input_scopes_yields_on_duplicate() {
    use nmp_core::substrate::{InputScopeDisposition, InputScopeRegistry};

    let registry = InputScopeRegistry::new();
    // First call installs all three.
    register_input_scopes(&registry);
    assert_eq!(registry.len(), 3);
    // Second call: all three scope ids already claimed → yielded, count stays 3.
    let disposition = registry.register(Arc::new(ProfilesInputRecognizer::new()));
    assert_eq!(disposition, InputScopeDisposition::YieldedToExisting);
    assert_eq!(registry.len(), 3);
}
