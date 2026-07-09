//! Referrer scope resolution (thread/referrer feeds) — split out of
//! `resolve_tests.rs` for file-size discipline (pre-#3086-merge polish).

use nmp_core::substrate::{EventId, KernelEvent};
use nmp_planner::InterestScope;

use super::resolve_static::resolve_referrer;

#[test]
fn referrer_scope_fails_closed_on_empty_id() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let result = resolve_referrer("", &kinds);
    assert!(
        result.is_err(),
        "referrer scope with empty event_id must fail closed"
    );
}

#[test]
fn referrer_scope_fails_closed_on_no_kinds() {
    let kinds = std::collections::BTreeSet::new();
    let result = resolve_referrer("root123", &kinds);
    assert!(
        result.is_err(),
        "referrer scope with no acquisition kinds must fail closed"
    );
}

#[test]
fn referrer_scope_admits_root_by_id() {
    use super::resolve_static::resolve_referrer;
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    // The root event itself (by id)
    let root = KernelEvent {
        id: EventId::from("root123"),
        author: "alice".to_string(),
        kind: 1,
        created_at: 100,
        tags: Vec::new(),
        content: "root note".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(
        (resolved.admission)(&root),
        "admission must admit the root by id"
    );
}

#[test]
fn referrer_scope_opens_etag_tail_and_root_id_interests() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    assert_eq!(
        resolved.interests.len(),
        2,
        "thread scope opens #e reply-tail plus root-by-id acquisition"
    );
    for interest in &resolved.interests {
        assert_eq!(
            interest.scope,
            InterestScope::Global,
            "thread acquisition is Global/account-agnostic"
        );
    }
    let shapes: Vec<_> = resolved
        .interests
        .iter()
        .map(|interest| &interest.shape)
        .collect();
    assert!(
        shapes.iter().any(|shape| {
            shape
                .tags
                .get("e")
                .is_some_and(|values| values.contains("root123"))
                && shape.kinds == kinds
        }),
        "#e reply-tail interest must be fixed at open"
    );
    assert!(
        shapes
            .iter()
            .any(|shape| shape.event_ids.contains("root123") && shape.kinds == kinds),
        "root-by-id interest must be kind-gated and fixed at open"
    );

    let live = (resolved.live_shape)().expect("live pull shape");
    assert!(
        live.tags
            .get("e")
            .is_some_and(|values| values.contains("root123")),
        "load_older pulls the reply tail"
    );
    assert!(
        live.event_ids.is_empty(),
        "root-by-id replay is fixed acquisition, not the load_older tail"
    );
}

#[test]
fn referrer_scope_admits_events_referencing_root_via_etag() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    // A reply that references the root via #e tag
    let reply = KernelEvent {
        id: EventId::from("reply456"),
        author: "bob".to_string(),
        kind: 1,
        created_at: 101,
        tags: vec![vec!["e".to_string(), "root123".to_string()]],
        content: "a reply".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(
        (resolved.admission)(&reply),
        "admission must admit events with #e referencing the root"
    );
}

#[test]
fn referrer_scope_rejects_non_primary_kind_root_and_etag_rows() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    let wrong_kind_root = KernelEvent {
        id: EventId::from("root123"),
        author: "alice".to_string(),
        kind: 30_023,
        created_at: 100,
        tags: Vec::new(),
        content: "longform root".to_string(),
        relay_provenance: Vec::new(),
    };
    let wrong_kind_reply = KernelEvent {
        id: EventId::from("reply789"),
        author: "bob".to_string(),
        kind: 30_023,
        created_at: 101,
        tags: vec![vec!["e".to_string(), "root123".to_string()]],
        content: "longform reply".to_string(),
        relay_provenance: Vec::new(),
    };

    assert!(
        !(resolved.admission)(&wrong_kind_root),
        "root-by-id admission must still require the compiled kinds"
    );
    assert!(
        !(resolved.admission)(&wrong_kind_reply),
        "#e admission must reject events outside the compiled kinds"
    );
}

#[test]
fn referrer_scope_rejects_unrelated_events() {
    let kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    let resolved = resolve_referrer("root123", &kinds).expect("valid referrer scope");

    // An unrelated event
    let unrelated = KernelEvent {
        id: EventId::from("other789"),
        author: "charlie".to_string(),
        kind: 1,
        created_at: 102,
        tags: vec![vec!["e".to_string(), "other_root".to_string()]],
        content: "unrelated note".to_string(),
        relay_provenance: Vec::new(),
    };
    assert!(
        !(resolved.admission)(&unrelated),
        "admission must reject events that don't reference the root"
    );
}
