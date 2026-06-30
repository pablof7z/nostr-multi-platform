//! Integration tests for the generic reference-counter sidecar (#2512, was #1519).
//!
//! These tests exercise the STORE's write-path maintenance of the generic,
//! noun-free counter (increment on insert, decrement on kind:5 deletion, GC, and
//! `delete_by_filter`). They install a SYNTHETIC classifier — buckets are picked
//! from a `c` tag, the target from the first `e` tag — so this crate stays free
//! of any protocol kind literal or NIP-10 semantics (those live in
//! `nmp-relations`, which has its own end-to-end engagement tests).

#![cfg(all(test, feature = "lmdb-backend"))]

use std::sync::Arc;

use tempfile::tempdir;

use crate::reference_counts::{ReferenceBucketId, ReferenceClassifyFn};
use crate::types::{GcBudget, RawEvent, VerifiedEvent};
use crate::{EventStore, LmdbEventStore};

// ─── Synthetic buckets ─────────────────────────────────────────────────────────

const ALPHA: ReferenceBucketId = ReferenceBucketId::new(1, "alpha");
const BETA: ReferenceBucketId = ReferenceBucketId::new(2, "beta");

/// A protocol-noun-free test classifier: an event counts iff it carries a `c`
/// tag (the bucket discriminant) AND an `e` tag (the target). Decoupled from any
/// kind — the point is to test the store's generic maintenance, not protocol
/// classification.
fn test_classifier() -> Arc<ReferenceClassifyFn> {
    Arc::new(|_kind, tags| {
        let bucket = tags
            .iter()
            .find(|t| t.first().map(|s| s == "c").unwrap_or(false))
            .and_then(|t| t.get(1))
            .and_then(|s| s.parse::<u8>().ok())?;
        let target = tags
            .iter()
            .find(|t| t.first().map(|s| s == "e").unwrap_or(false))
            .and_then(|t| t.get(1))
            .cloned()?;
        Some((ReferenceBucketId::new(bucket, "test"), target))
    })
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn open_tmp() -> (LmdbEventStore, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let store = LmdbEventStore::open(dir.path()).expect("open");
    (store, dir)
}

const PHANTOM_TARGET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

fn phantom_target_id() -> crate::types::EventId {
    crate::types::hex_to_event_id(PHANTOM_TARGET).expect("valid hex")
}

const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn verified(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

/// Build a raw event directly (integration-harness path — no signing). `tags`
/// carry whatever the synthetic classifier reads.
fn raw(id_hex: &str, pubkey_hex: &str, kind: u32, tags: Vec<Vec<String>>, created_at: u64) -> RawEvent {
    RawEvent {
        id: id_hex.to_string(),
        pubkey: pubkey_hex.to_string(),
        created_at,
        kind,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    }
}

fn etag(target_hex: &str) -> Vec<String> {
    vec!["e".to_string(), target_hex.to_string()]
}
fn ctag(bucket: u8) -> Vec<String> {
    vec!["c".to_string(), bucket.to_string()]
}

/// A counted reference: e-tags `target`, bucketed by `bucket`. The kind is an
/// arbitrary regular kind — the synthetic classifier keys on the `c` tag, never
/// the kind, so the store stays free of any engagement-kind vocabulary.
fn counted(id_hex: &str, target_hex: &str, bucket: u8, created_at: u64) -> RawEvent {
    raw(id_hex, ALICE, 2, vec![etag(target_hex), ctag(bucket)], created_at)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Without an installed classifier the sidecar stays inert — every insert is a
/// no-op and reads return empty.
#[test]
fn no_classifier_installed_yields_empty() {
    let (store, _dir) = open_tmp();
    store
        .insert(verified(counted(&"11".repeat(32), PHANTOM_TARGET, 1, 1000)), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(store.reference_counts(&phantom_target_id()).unwrap().is_empty());
}

/// Inserting one counted reference increments its bucket.
#[test]
fn insert_increments_bucket() {
    let (store, _dir) = open_tmp();
    store.install_reference_counter_classifier(test_classifier());
    store
        .insert(verified(counted(&"11".repeat(32), PHANTOM_TARGET, 1, 1000)), &"wss://r/".into(), 1_000_000)
        .unwrap();
    let counts = store.reference_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts.get(ALPHA), 1);
    assert_eq!(counts.get(BETA), 0);
}

/// Distinct buckets accumulate independently.
#[test]
fn buckets_accumulate_independently() {
    let (store, _dir) = open_tmp();
    store.install_reference_counter_classifier(test_classifier());
    let relay = "wss://r/".to_string();
    for i in 0..3u64 {
        let id = format!("a{:063x}", i);
        store.insert(verified(counted(&id, PHANTOM_TARGET, 1, 1000 + i)), &relay, 1_000_000).unwrap();
    }
    for i in 0..2u64 {
        let id = format!("b{:063x}", i);
        store.insert(verified(counted(&id, PHANTOM_TARGET, 2, 2000 + i)), &relay, 2_000_000).unwrap();
    }
    let counts = store.reference_counts(&phantom_target_id()).unwrap();
    assert_eq!(counts.get(ALPHA), 3);
    assert_eq!(counts.get(BETA), 2);
}

/// An event the classifier does not count leaves the sidecar untouched.
#[test]
fn uncounted_event_ignored() {
    let (store, _dir) = open_tmp();
    store.install_reference_counter_classifier(test_classifier());
    // No `c` tag → classifier returns None.
    let ev = raw(&"22".repeat(32), ALICE, 2, vec![etag(PHANTOM_TARGET)], 1000);
    store.insert(verified(ev), &"wss://r/".into(), 1_000_000).unwrap();
    assert!(store.reference_counts(&phantom_target_id()).unwrap().is_empty());
}

/// kind:5 deletion of a counted event decrements its bucket.
#[test]
fn kind5_delete_decrements() {
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();
    store.install_reference_counter_classifier(test_classifier());

    let ev_id = "11".repeat(32);
    store.insert(verified(counted(&ev_id, PHANTOM_TARGET, 1, 1000)), &relay, 1_000_000).unwrap();
    assert_eq!(store.reference_counts(&phantom_target_id()).unwrap().get(ALPHA), 1);

    // Same-author kind:5 e-tagging the counted event removes it.
    let del = raw(&"55".repeat(32), ALICE, 5, vec![etag(&ev_id)], 2000);
    store.insert(verified(del), &relay, 2_000_000).unwrap();

    assert_eq!(store.reference_counts(&phantom_target_id()).unwrap().get(ALPHA), 0);
}

/// GC (NIP-40 expiry eviction) decrements the bucket for an evicted counted event.
#[test]
fn gc_expiry_decrements() {
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();
    store.install_reference_counter_classifier(test_classifier());

    let mut ev = counted(&"11".repeat(32), PHANTOM_TARGET, 1, 1000);
    ev.tags.push(vec!["expiration".to_string(), "5000".to_string()]);
    store.insert(verified(ev), &relay, 1_000_000).unwrap();
    assert_eq!(store.reference_counts(&phantom_target_id()).unwrap().get(ALPHA), 1);

    let budget = GcBudget {
        max_events_per_step: 100,
        max_duration_ms: 60_000,
        max_total_events: usize::MAX,
    };
    store.gc_step(budget, 6000).unwrap(); // now(6000) > expiry(5000)

    assert_eq!(
        store.reference_counts(&phantom_target_id()).unwrap().get(ALPHA),
        0,
        "GC expiry must decrement the counter"
    );
}

/// `delete_by_filter` decrements the bucket for each deleted counted event.
#[test]
fn delete_by_filter_decrements() {
    use crate::types::DeleteFilter;
    let (store, _dir) = open_tmp();
    let relay = "wss://r/".to_string();
    store.install_reference_counter_classifier(test_classifier());

    let counted_ev = counted(&"11".repeat(32), PHANTOM_TARGET, 1, 1000);
    let ev_id = counted_ev.id_bytes().unwrap();
    store.insert(verified(counted_ev), &relay, 1_000_000).unwrap();
    assert_eq!(store.reference_counts(&phantom_target_id()).unwrap().get(ALPHA), 1);

    store.delete_by_filter(DeleteFilter::ByIds(vec![ev_id])).unwrap();

    assert_eq!(
        store.reference_counts(&phantom_target_id()).unwrap().get(ALPHA),
        0,
        "delete_by_filter must decrement the counter"
    );
}
