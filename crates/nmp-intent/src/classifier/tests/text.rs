//! Rung 6 — free-text → `SearchRequest` json bridge: correct scope + targets,
//! and one candidate per requested `nip50.*` scope.

use nmp_core::substrate::TextSearchTargets;
use nmp_nip50::{SearchScope, SearchTargets};

use nmp_core::substrate::InputIntentClassification;

use super::{
    classify_bare, decode_text_query, expect_single, longform_scope, notes_scope, profiles_scope,
    req,
};

#[test]
fn free_text_under_profiles_builds_users_search_request() {
    let r = req("alice", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.scope, profiles_scope());
    let request = decode_text_query(&cand.target);
    assert_eq!(request.query, "alice");
    assert_eq!(request.scope, SearchScope::Users);
    assert_eq!(request.targets, SearchTargets::UserPreferred);
}

#[test]
fn free_text_under_longform_builds_longform_search_request() {
    let r = req("nostr protocol", vec![longform_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.scope, longform_scope());
    let request = decode_text_query(&cand.target);
    assert_eq!(request.scope, SearchScope::LongForm);
    assert_eq!(request.query, "nostr protocol");
}

#[test]
fn free_text_honors_explicit_targets() {
    let mut r = req("query", vec![profiles_scope()]);
    r.text_targets = TextSearchTargets::Explicit(vec!["wss://search.example".to_string()]);
    let cand = expect_single(classify_bare(&r));
    let request = decode_text_query(&cand.target);
    assert_eq!(
        request.targets,
        SearchTargets::Explicit(vec!["wss://search.example".to_string()])
    );
}

#[test]
fn free_text_app_default_targets() {
    let mut r = req("query", vec![notes_scope()]);
    r.text_targets = TextSearchTargets::AppDefault;
    let cand = expect_single(classify_bare(&r));
    let request = decode_text_query(&cand.target);
    assert_eq!(request.targets, SearchTargets::AppDefault);
    assert!(matches!(request.scope, SearchScope::Kinds(_)));
}

#[test]
fn free_text_emits_one_candidate_per_requested_nip50_scope() {
    let r = req("alice", vec![profiles_scope(), longform_scope()]);
    match classify_bare(&r) {
        InputIntentClassification::Candidates(v) => {
            assert_eq!(v.len(), 2);
            assert!(v.iter().any(|c| c.scope == profiles_scope()));
            assert!(v.iter().any(|c| c.scope == longform_scope()));
        }
        other => panic!("expected 2 candidates, got {other:?}"),
    }
}
