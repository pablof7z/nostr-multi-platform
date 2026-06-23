//! Rung 4 — NIP-05 SHAPE detection (positive shapes vs. non-shapes that fall
//! through to free text).

use nmp_core::substrate::{InputIntentClassification, InputIntentTarget, InputScopeId};

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

#[test]
fn uppercase_local_part_is_not_nip05_shape() {
    // `parse_nip05` rejects uppercase local parts (the key must match the
    // `names` map verbatim). Before #1882 the classifier accepted them and the
    // candidate silently no-op'd on dispatch; now the classifier defers to
    // `parse_nip05`, so it agrees and the input falls through to free text.
    let r = req("Alice@example.com", vec![profiles_scope()]);
    let cand = expect_single(classify_bare(&r));
    assert!(
        matches!(cand.target, InputIntentTarget::TextQuery { .. }),
        "uppercase local part must fall through to free text, got {:?}",
        cand.target
    );
}

#[test]
fn every_classified_nip05_is_accepted_by_parse_nip05() {
    // The agreement invariant (#1882): any identifier the classifier labels
    // `Nip05` MUST be accepted by the canonical `parse_nip05`, else dispatch
    // would silently no-op. Spans uppercase/lowercase local parts, the root
    // identifier, sub-domains, non-shapes, and free-text.
    let inputs = [
        "alice@example.com",
        "_@example.com",
        "a.b-c_d@sub.example.com",
        "Alice@example.com",
        "ALICE@EXAMPLE.COM",
        "MixedCase@Example.COM",
        "bob@localhost",
        "a@b@example.com",
        "plainword",
        "foo@bar.co",
        "ali ce@example.com",
    ];
    for input in inputs {
        let r = req(input, vec![profiles_scope()]);
        if let InputIntentClassification::Candidates(cands) = classify_bare(&r) {
            for cand in cands {
                if let InputIntentTarget::Nip05 { identifier } = &cand.target {
                    assert!(
                        nmp_nip05::parse_nip05(identifier).is_some(),
                        "classifier labeled `{identifier}` as Nip05 but parse_nip05 rejects it (silent no-op)"
                    );
                }
            }
        }
    }
}
