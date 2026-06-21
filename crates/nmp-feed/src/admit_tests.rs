//! Truth-table tests for the compiled, EVENT-AWARE admission predicate
//! (#1740 step 3).

use super::AdmitExpr;
use nmp_core::substrate::KernelEvent;
use std::collections::BTreeSet;

fn authors(ids: &[&str]) -> AdmitExpr {
    AdmitExpr::Authors(ids.iter().map(|s| (*s).to_string()).collect())
}

const A: &str = "aaaa";
const B: &str = "bbbb";
const C: &str = "cccc";

/// A minimal root event by `author`, carrying the given `#t` tag terms.
fn event(author: &str, t_tags: &[&str]) -> KernelEvent {
    KernelEvent {
        id: format!("id-{author}"),
        author: author.to_string(),
        kind: 1,
        created_at: 0,
        tags: t_tags
            .iter()
            .map(|t| vec!["t".to_string(), (*t).to_string()])
            .collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn any_admits_everyone() {
    assert!(AdmitExpr::Any.matches(&event(A, &[])));
    assert!(AdmitExpr::Any.matches(&event("anything", &[])));
}

#[test]
fn authors_admits_only_members() {
    let admit = authors(&[A, B]);
    assert!(admit.matches(&event(A, &[])));
    assert!(admit.matches(&event(B, &[])));
    // Fail-closed: a non-member is NOT admitted.
    assert!(!admit.matches(&event(C, &[])));
}

#[test]
fn empty_author_set_admits_nobody() {
    let admit = AdmitExpr::Authors(BTreeSet::new());
    assert!(!admit.matches(&event(A, &[])));
    assert!(!admit.matches(&event(B, &[])));
}

#[test]
fn tag_admits_only_matching_t_tag() {
    let admit = AdmitExpr::Tag("rust".to_string());
    assert!(admit.matches(&event(A, &["rust"])));
    assert!(admit.matches(&event(A, &["nostr", "rust"])));
    // No matching #t tag → not admitted (regardless of author).
    assert!(!admit.matches(&event(A, &["nostr"])));
    assert!(!admit.matches(&event(A, &[])));
}

#[test]
fn union_is_or() {
    let admit = AdmitExpr::Or(Box::new(authors(&[A])), Box::new(authors(&[B])));
    assert!(admit.matches(&event(A, &[])));
    assert!(admit.matches(&event(B, &[])));
    assert!(!admit.matches(&event(C, &[])));
}

#[test]
fn intersection_is_and() {
    let admit = AdmitExpr::And(Box::new(authors(&[A, B])), Box::new(authors(&[B, C])));
    // Only B is in BOTH sets.
    assert!(!admit.matches(&event(A, &[])));
    assert!(admit.matches(&event(B, &[])));
    assert!(!admit.matches(&event(C, &[])));
}

#[test]
fn difference_is_and_not() {
    let admit = AdmitExpr::AndNot(Box::new(authors(&[A, B])), Box::new(authors(&[B])));
    // A is in left and not right → admitted; B is excluded by the right set.
    assert!(admit.matches(&event(A, &[])));
    assert!(!admit.matches(&event(B, &[])));
    assert!(!admit.matches(&event(C, &[])));
}

#[test]
fn and_with_any_collapses_to_other() {
    // Intersection with `Any` admits exactly the other side.
    let admit = AdmitExpr::And(Box::new(AdmitExpr::Any), Box::new(authors(&[A])));
    assert!(admit.matches(&event(A, &[])));
    assert!(!admit.matches(&event(B, &[])));
}

#[test]
fn mixed_tag_and_author_intersection_checks_both() {
    // #1740 step 3 (mixed tag+author algebra): Intersection(Tag, ContactList)
    // must check BOTH the event's #t tag AND its author membership — NOT treat
    // the tag scope as `Any` (which would silently admit a member's untagged
    // note).
    let admit = AdmitExpr::And(
        Box::new(AdmitExpr::Tag("rust".to_string())),
        Box::new(authors(&[A, B])),
    );
    // Member A with the #t tag → admitted.
    assert!(admit.matches(&event(A, &["rust"])));
    // Member A WITHOUT the tag → NOT admitted (the faithful AND, not Any).
    assert!(!admit.matches(&event(A, &["nostr"])));
    // Non-member C with the tag → NOT admitted (author side fails).
    assert!(!admit.matches(&event(C, &["rust"])));
}

#[test]
fn mixed_tag_difference_excludes_tagged_member() {
    // Difference(ContactList, Tag): a member's note is admitted UNLESS it carries
    // the excluded #t tag.
    let admit = AdmitExpr::AndNot(
        Box::new(authors(&[A, B])),
        Box::new(AdmitExpr::Tag("spoiler".to_string())),
    );
    assert!(admit.matches(&event(A, &["nostr"])));
    // Member A but tagged #spoiler → excluded by the right side.
    assert!(!admit.matches(&event(A, &["spoiler"])));
    // Non-member never admitted.
    assert!(!admit.matches(&event(C, &[])));
}

#[test]
fn to_root_admission_matches_data() {
    let pred = authors(&[A, B]).to_root_admission();
    assert!(pred(&event(A, &[])));
    assert!(pred(&event(B, &[])));
    assert!(!pred(&event(C, &[])));
}

#[test]
fn empty_predicate_fails_closed() {
    let pred = AdmitExpr::Authors(BTreeSet::new()).to_root_admission();
    assert!(!pred(&event(A, &[])));
}
