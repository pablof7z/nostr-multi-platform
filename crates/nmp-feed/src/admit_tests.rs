//! Truth-table tests for the compiled admission predicate (#1740 step 3).

use super::AdmitExpr;
use std::collections::BTreeSet;

fn authors(ids: &[&str]) -> AdmitExpr {
    AdmitExpr::Authors(ids.iter().map(|s| (*s).to_string()).collect())
}

const A: &str = "aaaa";
const B: &str = "bbbb";
const C: &str = "cccc";

#[test]
fn any_admits_everyone() {
    assert!(AdmitExpr::Any.matches(A));
    assert!(AdmitExpr::Any.matches("anything"));
}

#[test]
fn authors_admits_only_members() {
    let admit = authors(&[A, B]);
    assert!(admit.matches(A));
    assert!(admit.matches(B));
    // Fail-closed: a non-member is NOT admitted.
    assert!(!admit.matches(C));
}

#[test]
fn empty_author_set_admits_nobody() {
    let admit = AdmitExpr::Authors(BTreeSet::new());
    assert!(!admit.matches(A));
    assert!(!admit.matches(B));
}

#[test]
fn union_is_or() {
    let admit = AdmitExpr::Or(Box::new(authors(&[A])), Box::new(authors(&[B])));
    assert!(admit.matches(A));
    assert!(admit.matches(B));
    assert!(!admit.matches(C));
}

#[test]
fn intersection_is_and() {
    let admit = AdmitExpr::And(Box::new(authors(&[A, B])), Box::new(authors(&[B, C])));
    // Only B is in BOTH sets.
    assert!(!admit.matches(A));
    assert!(admit.matches(B));
    assert!(!admit.matches(C));
}

#[test]
fn difference_is_and_not() {
    let admit = AdmitExpr::AndNot(Box::new(authors(&[A, B])), Box::new(authors(&[B])));
    // A is in left and not right → admitted; B is excluded by the right set.
    assert!(admit.matches(A));
    assert!(!admit.matches(B));
    assert!(!admit.matches(C));
}

#[test]
fn and_with_any_collapses_to_other() {
    // Intersection with `Any` admits exactly the other side.
    let admit = AdmitExpr::And(Box::new(AdmitExpr::Any), Box::new(authors(&[A])));
    assert!(admit.matches(A));
    assert!(!admit.matches(B));
}

#[test]
fn to_follow_predicate_matches_data() {
    let pred = authors(&[A, B]).to_follow_predicate();
    assert!(pred(A));
    assert!(pred(B));
    assert!(!pred(C));
}

#[test]
fn empty_predicate_fails_closed() {
    let pred = AdmitExpr::Authors(BTreeSet::new()).to_follow_predicate();
    assert!(!pred(A));
}
