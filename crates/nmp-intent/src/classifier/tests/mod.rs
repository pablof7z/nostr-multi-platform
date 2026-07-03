//! Unit tests for the input-intent classifier (issue #1804 / S1), grouped by
//! precedence rung. Shared fixtures + assertion helpers live here; each rung's
//! cases live in its own submodule so no test file approaches the size cap.

use nmp_core::substrate::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentRequest,
    InputIntentTarget, InputScopeId, TextSearchTargets,
};
use nmp_nip50::SearchRequest;

use super::classify_impl;

mod ad;
mod nip05;
mod recognizer;
mod reference;
mod refusal;
mod relay;
mod secret;
mod text;

// ─── shared fixtures ─────────────────────────────────────────────────────────

pub(crate) const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
pub(crate) const EVID: &str = "5c83da77af1dec6d7289834998ad7aafbd9e2191396d75ec3cc27f5a77226f36";
pub(crate) const SK: &str = "67dea2ed018072d675f5415ecfaed7d2597555e202d85b3d65ea4e58d2d92ffa";

pub(crate) fn profiles_scope() -> InputScopeId {
    InputScopeId::new("nip50", "profiles")
}
pub(crate) fn notes_scope() -> InputScopeId {
    InputScopeId::new("nip50", "notes")
}
pub(crate) fn longform_scope() -> InputScopeId {
    InputScopeId::new("nip50", "longform")
}

/// Request with the given scopes and `UserPreferred` text targets.
pub(crate) fn req(input: &str, scopes: Vec<InputScopeId>) -> InputIntentRequest {
    InputIntentRequest {
        input: input.to_string(),
        scopes,
        text_targets: TextSearchTargets::UserPreferred,
    }
}

/// Classify with no registered recognizers.
pub(crate) fn classify_bare(req: &InputIntentRequest) -> InputIntentClassification {
    classify_impl(req, &[])
}

pub(crate) fn expect_single(c: InputIntentClassification) -> InputIntentCandidate {
    match c {
        InputIntentClassification::Candidates(mut v) => {
            assert_eq!(v.len(), 1, "expected exactly one candidate, got {v:?}");
            v.remove(0)
        }
        other => panic!("expected Candidates, got {other:?}"),
    }
}

pub(crate) fn expect_rejection(c: InputIntentClassification) -> InputIntentRejection {
    match c {
        InputIntentClassification::Rejection(r) => r,
        other => panic!("expected Rejection, got {other:?}"),
    }
}

pub(crate) fn decode_text_query(target: &InputIntentTarget) -> SearchRequest {
    match target {
        InputIntentTarget::TextQuery { request_json } => {
            serde_json::from_str(request_json).expect("valid SearchRequest json")
        }
        other => panic!("expected TextQuery, got {other:?}"),
    }
}
