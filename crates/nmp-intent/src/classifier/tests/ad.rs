//! Rung 4.5 — NIP-AD candidate (#2927): an `http(s)://` URL classifies as an
//! `AdCandidate` emitted alongside the free-text search candidates (D1).

use nmp_core::substrate::{InputIntentClassification, InputIntentTarget, InputScopeId};

use super::{classify_bare, profiles_scope, req};

#[test]
fn https_url_is_ad_candidate_plus_free_text() {
    let r = req("https://trellis.rs/legible", vec![profiles_scope()]);
    let InputIntentClassification::Candidates(candidates) = classify_bare(&r) else {
        panic!("expected candidates");
    };
    // First candidate is the AD candidate under the synthetic ref scope.
    assert_eq!(candidates[0].scope, InputScopeId::nostr_ref());
    match &candidates[0].target {
        InputIntentTarget::AdCandidate { url } => {
            assert_eq!(url, "https://trellis.rs/legible");
        }
        other => panic!("expected AdCandidate first, got {other:?}"),
    }
    // D1: the free-text search candidate for the same input is co-emitted so the
    // app can search in parallel while resolving the AD URL.
    assert!(
        candidates
            .iter()
            .any(|c| matches!(c.target, InputIntentTarget::TextQuery { .. })),
        "expected a co-emitted free-text TextQuery candidate, got {candidates:?}"
    );
}

#[test]
fn ad_candidate_emitted_even_without_text_scopes() {
    // No nip50 scopes requested → no free-text candidate, but the URL is still a
    // first-class AD candidate (moment-2 is never scope-gated).
    let r = req("https://example.com/path", vec![]);
    let InputIntentClassification::Candidates(candidates) = classify_bare(&r) else {
        panic!("expected candidates");
    };
    assert_eq!(candidates.len(), 1);
    assert!(matches!(
        candidates[0].target,
        InputIntentTarget::AdCandidate { .. }
    ));
}

#[test]
fn non_url_does_not_become_ad_candidate() {
    // A bare domain is a NIP-05-shape miss AND not a URL → free text, not AD.
    let r = req("just some text", vec![profiles_scope()]);
    if let InputIntentClassification::Candidates(candidates) = classify_bare(&r) {
        assert!(
            !candidates
                .iter()
                .any(|c| matches!(c.target, InputIntentTarget::AdCandidate { .. })),
            "free text must not classify as an AD candidate"
        );
    }
}
