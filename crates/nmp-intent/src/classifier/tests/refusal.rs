//! Rung 7 — refusals: `UnregisteredScope` (requested scope with no recognizer
//! and no built-in) and `Unparseable` (no structural match, no servable text).

use nmp_core::substrate::{InputIntentRejection, InputScopeId};

use super::{classify_bare, expect_rejection, profiles_scope, req};

#[test]
fn free_text_with_unregistered_custom_scope_is_unregistered() {
    // App requests a custom scope with no registered recognizer and no built-in.
    let custom = InputScopeId::new("myapp", "widgets");
    let r = req("hello", vec![custom.clone()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::UnregisteredScope { scope: custom }
    );
}

#[test]
fn empty_input_with_only_nip50_scope_is_unparseable() {
    // Whitespace-only input: no structural match, the free-text rung is skipped
    // (empty), and `nip50.*` scopes are serviceable → not UnregisteredScope → so
    // the final fall-through is Unparseable.
    let r = req("   ", vec![profiles_scope()]);
    assert_eq!(
        expect_rejection(classify_bare(&r)),
        InputIntentRejection::Unparseable
    );
}
