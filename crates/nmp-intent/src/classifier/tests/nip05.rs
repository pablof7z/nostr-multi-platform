//! Rung 4 — NIP-05 SHAPE detection (positive shapes vs. non-shapes that fall
//! through to free text).

use nmp_core::substrate::{InputIntentTarget, InputScopeId};

use super::{classify_bare, expect_single, profiles_scope, req};

#[test]
fn nip05_identifier_routes_to_nip05_shape() {
    let r = req("alice@example.com", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert_eq!(cand.scope, InputScopeId::nostr_ref());
    assert_eq!(
        cand.target,
        InputIntentTarget::Nip05 {
            identifier: "alice@example.com".to_string()
        }
    );
}

#[test]
fn nip05_root_identifier_underscore_is_recognized() {
    let r = req("_@example.com", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert!(matches!(cand.target, InputIntentTarget::Nip05 { .. }));
}

#[test]
fn email_without_tld_is_not_nip05_shape() {
    // `bob@localhost` has no dot in the domain → not a NIP-05 shape; falls
    // through to free text.
    let r = req("bob@localhost", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert!(
        matches!(cand.target, InputIntentTarget::TextQuery { .. }),
        "expected free-text fall-through, got {:?}",
        cand.target
    );
}

#[test]
fn double_at_is_not_nip05_shape() {
    let r = req("a@b@example.com", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    // split_once('@') gives local="a", domain="b@example.com"; '@' is not a
    // valid domain-label char → falls through to free text.
    assert!(matches!(cand.target, InputIntentTarget::TextQuery { .. }));
}
