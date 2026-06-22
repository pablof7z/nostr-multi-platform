//! Unit tests for [`HandleCorrelationIndex`] (S7, #1754).

use super::{HandleCorrelationIndex, MAX_HANDLE_CORRELATION_ENTRIES};

#[test]
fn resolves_correlation_id_to_handle_and_back() {
    let mut idx = HandleCorrelationIndex::new();
    idx.record("event-handle-1", Some("op-corr-1"));

    // Resolve by correlation_id → (handle, original correlation_id).
    let (handle, corr) = idx
        .resolve("op-corr-1")
        .expect("known correlation resolves");
    assert_eq!(handle, "event-handle-1");
    assert_eq!(corr, "op-corr-1");

    // Resolve by the raw handle → SAME pair (the original correlation_id).
    let (handle2, corr2) = idx
        .resolve("event-handle-1")
        .expect("known handle resolves");
    assert_eq!(handle2, "event-handle-1");
    assert_eq!(
        corr2, "op-corr-1",
        "a handle lookup still recovers the ORIGINAL correlation_id (PD-036)"
    );
}

#[test]
fn none_correlation_self_maps_handle() {
    // An internal publish with no distinct dispatch correlation_id maps the
    // handle to itself, so cancel-by-handle still resolves.
    let mut idx = HandleCorrelationIndex::new();
    idx.record("internal-handle", None);

    let (handle, corr) = idx
        .resolve("internal-handle")
        .expect("self-mapped resolves");
    assert_eq!(handle, "internal-handle");
    assert_eq!(corr, "internal-handle");
}

#[test]
fn unknown_id_resolves_to_none() {
    let idx = HandleCorrelationIndex::new();
    assert!(
        idx.resolve("never-seen").is_none(),
        "an unknown id resolves to None so the caller falls back to id-as-both"
    );
}

#[test]
fn forget_clears_both_directions() {
    let mut idx = HandleCorrelationIndex::new();
    idx.record("h1", Some("c1"));
    idx.forget("h1");

    assert!(idx.resolve("c1").is_none(), "correlation lookup cleared");
    assert!(idx.resolve("h1").is_none(), "handle lookup cleared");
    assert_eq!(idx.len(), 0);
}

#[test]
fn re_record_same_handle_does_not_duplicate_order() {
    let mut idx = HandleCorrelationIndex::new();
    idx.record("h1", Some("c1"));
    idx.record("h1", Some("c1"));
    assert_eq!(idx.len(), 1, "re-recording the same handle keeps one entry");
}

#[test]
fn global_cap_evicts_oldest() {
    let mut idx = HandleCorrelationIndex::new();
    for i in 0..MAX_HANDLE_CORRELATION_ENTRIES {
        idx.record(&format!("h{i}"), Some(&format!("c{i}")));
    }
    assert_eq!(idx.len(), MAX_HANDLE_CORRELATION_ENTRIES);

    // One past the cap evicts the oldest (h0/c0) and keeps the cardinality
    // bounded (D8).
    idx.record("h-overflow", Some("c-overflow"));
    assert_eq!(idx.len(), MAX_HANDLE_CORRELATION_ENTRIES);
    assert!(
        idx.resolve("c0").is_none(),
        "the oldest pair is evicted at the cap"
    );
    assert!(
        idx.resolve("c-overflow").is_some(),
        "the newest pair survives"
    );
}
