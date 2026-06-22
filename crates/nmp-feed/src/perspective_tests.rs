//! Unit proofs for the closed-data custom-perspective registry (#1740 step 4).

use super::*;
use crate::params::{CustomPerspectiveId, FeedRanking, FeedScope, TagTerm};

fn id(s: &str) -> CustomPerspectiveId {
    CustomPerspectiveId(s.to_string())
}

fn tag_scope(term: &str) -> FeedScope {
    FeedScope::Tag {
        term: TagTerm(term.to_string()),
    }
}

#[test]
fn unregistered_id_resolves_to_none_fail_closed() {
    let reg = PerspectiveRegistry::default();
    // The fail-closed contract: an id that was never registered has no
    // definition, so the compiler keying on `get(..) == None` rejects the open.
    assert!(reg.get(&id("missing")).is_none());
    assert!(!reg.is_registered(&id("missing")));
    assert!(reg.is_empty());
}

#[test]
fn registered_id_resolves_to_its_definition() {
    let reg = PerspectiveRegistry::default();
    let def = CustomPerspectiveDef::new(tag_scope("rust"));
    assert!(reg.register(id("eng"), def.clone()), "first register succeeds");

    assert!(reg.is_registered(&id("eng")));
    assert_eq!(reg.get(&id("eng")), Some(def));
    assert_eq!(reg.len(), 1);
    // A DIFFERENT id is still unregistered → fail closed.
    assert!(reg.get(&id("other")).is_none());
}

#[test]
fn register_is_register_once_immutable_no_overwrite() {
    // Immutability is a fail-CLOSED safety property: a live session captured the
    // FIRST definition's compiled admission. A second register under the same id
    // must NOT overwrite it (overwriting a narrower gate in would leave open
    // feeds admitting under the stale WIDER policy — a fail-OPEN leak).
    let reg = PerspectiveRegistry::default();
    assert!(reg.register(id("eng"), CustomPerspectiveDef::new(tag_scope("rust"))));
    assert!(
        !reg.register(id("eng"), CustomPerspectiveDef::new(tag_scope("nostr"))),
        "re-register under a live id is rejected"
    );
    assert_eq!(reg.len(), 1, "no append");
    assert_eq!(
        reg.get(&id("eng")).map(|d| d.acquisition),
        Some(tag_scope("rust")),
        "the FIRST definition stands (immutable, no overwrite)",
    );
}

#[test]
fn definition_carries_ranking() {
    let def = CustomPerspectiveDef::new(tag_scope("rust"))
        .with_ranking(FeedRanking::ChronologicalAsc);
    assert_eq!(def.ranking, FeedRanking::ChronologicalAsc);
    // Default ranking is chronological-descending (the engine-honored order).
    assert_eq!(
        CustomPerspectiveDef::new(tag_scope("rust")).ranking,
        FeedRanking::ChronologicalDesc,
    );
}
