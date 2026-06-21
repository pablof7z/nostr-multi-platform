//! #1740 step 3 — the compiled perspective predicate gates ROOTS, not just
//! reply attribution.
//!
//! These drive the real engine through `with_root_admission` and assert that a
//! root whose event the perspective does not admit NEVER enters the feed. This
//! is the engine-level proof of the admission fix; the per-scope predicate truth
//! tables live in `nmp-testing`'s `feed_perspective_admission_matrix`.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::KernelEvent;

use crate::{AdmitExpr, EventGate, RootAdmission};

use super::support::{root_event, Harness};

/// A root-admission predicate that admits only the given author set.
fn members(ids: &[&str]) -> RootAdmission {
    let set: BTreeSet<String> = ids.iter().map(|s| (*s).to_string()).collect();
    AdmitExpr::Authors(set).to_root_admission()
}

fn allow_all_gate() -> EventGate {
    Arc::new(|_event: &KernelEvent| true)
}

#[test]
fn contact_list_admits_only_member_roots() {
    // ContactList/ListMembers/Wot all compile to an author-membership predicate.
    // A member's root enters the feed; a non-member's root is EXCLUDED.
    let h = Harness::with_root_admission(&[], allow_all_gate(), members(&["bob"]));

    h.ingest(&root_event("op-member", "bob", 10, "from a member"));
    h.ingest(&root_event("op-stranger", "mallory", 11, "from a stranger"));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1, "only the member root is admitted");
    assert_eq!(snap.cards[0].card.root_id, "op-member");
}

#[test]
fn empty_member_set_admits_no_roots_fail_closed() {
    // Fail-closed: an empty resolved set (e.g. a list before it arrives) admits
    // NOBODY as a root — never everyone.
    let h = Harness::with_root_admission(&[], allow_all_gate(), members(&[]));
    h.ingest(&root_event("op1", "bob", 10, "hi"));
    assert!(h.snapshot().cards.is_empty());
}

#[test]
fn difference_excludes_right_side_members_as_roots() {
    // Difference(A, B): a root authored by a B-side member is excluded from the
    // feed even though it is a root (the perspective filters the feed itself).
    // A = {alice, bob}, B = {bob} → only alice's root survives.
    let left = AdmitExpr::Authors(["alice".to_string(), "bob".to_string()].into_iter().collect());
    let right = AdmitExpr::Authors(["bob".to_string()].into_iter().collect());
    let admission = AdmitExpr::AndNot(Box::new(left), Box::new(right)).to_root_admission();

    let h = Harness::with_root_admission(&[], allow_all_gate(), admission);
    h.ingest(&root_event("op-alice", "alice", 10, "left-only"));
    h.ingest(&root_event("op-bob", "bob", 11, "right-side member"));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1, "right-side member excluded as a root");
    assert_eq!(snap.cards[0].card.root_id, "op-alice");
}

#[test]
fn tag_scope_admits_only_tagged_roots() {
    // A `#t` tag perspective admits a root iff it carries the tag — the
    // event-aware admission gates the rendered feed.
    let admission = AdmitExpr::Tag("rust".to_string()).to_root_admission();
    let h = Harness::with_root_admission(&[], allow_all_gate(), admission);

    let mut tagged = root_event("op-tagged", "anyone", 10, "about rust");
    tagged.tags = vec![vec!["t".to_string(), "rust".to_string()]];
    let untagged = root_event("op-untagged", "anyone", 11, "off topic");

    h.ingest(&tagged);
    h.ingest(&untagged);

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1, "only the #t-tagged root is admitted");
    assert_eq!(snap.cards[0].card.root_id, "op-tagged");
}
