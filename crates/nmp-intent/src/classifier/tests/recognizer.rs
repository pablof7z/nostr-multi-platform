//! Rungs 5/6 — registered-recognizer integration: a requested recognizer's
//! `text_candidate` is served; a recognizer the app did not request is skipped
//! (and the built-in NIP-50 fall-through serves the requested scope instead).

use std::sync::Arc;

use nmp_core::substrate::{
    InputIntentTarget, InputScopeId, InputScopeRecognizer, ResolvedInput, TextSearchTargets,
};

use super::{classify_impl, expect_single, profiles_scope, req};

struct StubRecognizer {
    scope: InputScopeId,
}

impl InputScopeRecognizer for StubRecognizer {
    fn scope(&self) -> InputScopeId {
        self.scope.clone()
    }
    fn recognize(&self, _input: &ResolvedInput) -> Option<InputIntentTarget> {
        None
    }
    fn text_candidate(
        &self,
        free_text: &str,
        _targets: &TextSearchTargets,
    ) -> Option<InputIntentTarget> {
        Some(InputIntentTarget::Registered {
            payload_json: format!("{{\"q\":\"{free_text}\"}}"),
        })
    }
}

#[test]
fn registered_recognizer_serves_its_requested_scope_as_text_candidate() {
    let scope = InputScopeId::new("myapp", "widgets");
    let recog: Arc<dyn InputScopeRecognizer> = Arc::new(StubRecognizer {
        scope: scope.clone(),
    });
    let r = req("hello", vec![scope.clone()]);
    let cand = expect_single(classify_impl(&r, &[recog]));
    assert_eq!(cand.scope, scope);
    assert_eq!(
        cand.target,
        InputIntentTarget::Registered {
            payload_json: "{\"q\":\"hello\"}".to_string()
        }
    );
}

#[test]
fn registered_recognizer_not_requested_is_skipped() {
    let scope = InputScopeId::new("myapp", "widgets");
    let recog: Arc<dyn InputScopeRecognizer> = Arc::new(StubRecognizer {
        scope: scope.clone(),
    });
    // App requests only profiles; the widgets recognizer must not fire, and the
    // built-in profiles search serves the text.
    let r = req("hello", vec![profiles_scope()]);
    let cand = expect_single(classify_impl(&r, &[recog]));
    assert_eq!(cand.scope, profiles_scope());
    assert!(matches!(cand.target, InputIntentTarget::TextQuery { .. }));
}
