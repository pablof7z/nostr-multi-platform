//! Unit-level key/shape tests (no actor needed).
//!
//! These tests verify compile-time invariants: key formatting and filter JSON
//! parsing. They are cheap and do not require the actor harness.

use super::super::*;

#[test]
fn keys_are_namespaced_per_consumer() {
    assert_eq!(author_feed_key("abc"), "nmp.feed.author.abc");
    assert_eq!(thread_feed_key("def"), "nmp.feed.thread.def");
    assert_eq!(author_consumer("abc"), "author-abc");
    assert_eq!(thread_consumer("def"), "thread-def");
}

#[test]
fn filter_json_carries_derived_acquisition_kinds_and_dimension() {
    assert_eq!(FEED_PRIMARY_KINDS, [1]);
    let acquisition = feed_acquisition_kinds().expect("primary kind derives acquisition");

    let author_json = feed_filter_json("authors", "abc").expect("author filter");
    let author_shape = nmp_planner::InterestShape::from_filter_json(&author_json).unwrap();
    assert_eq!(
        author_shape.kinds,
        acquisition
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(
        author_shape.authors,
        std::collections::BTreeSet::from(["abc".to_string()])
    );

    let thread_json = feed_filter_json("#e", "root1").expect("thread filter");
    let thread_shape = nmp_planner::InterestShape::from_filter_json(&thread_json).unwrap();
    assert_eq!(
        thread_shape.kinds,
        acquisition
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(
        thread_shape.tags.get("e"),
        Some(&std::collections::BTreeSet::from(["root1".to_string()]))
    );
}

#[test]
fn feed_filter_json_parses_as_a_valid_interest_shape() {
    for json in [
        feed_filter_json("authors", "abc").expect("valid author filter"),
        feed_filter_json("#e", "root1").expect("valid thread filter"),
    ] {
        assert!(
            nmp_planner::InterestShape::from_filter_json(&json).is_some(),
            "filter must parse: {json}"
        );
    }
}
