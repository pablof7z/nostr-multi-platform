//! LMDB-backend parity tests against `MemEventStore`.
//!
//! Mirrors `mem/tests.rs` — same scenarios, same expected outcomes. The
//! per-test fixture uses `tempfile::tempdir()` to spin up a fresh
//! `LmdbEventStore` so each test owns its own LMDB env.
//!
//! Kind:5 deletion scenarios live in the sibling `tests_kind5.rs` so this
//! file stays under the 500-LOC hard cap.

#![cfg(feature = "lmdb-backend")]

use std::ops::ControlFlow;

use crate::types::{InsertOutcome, RawEvent, StoreQuery};
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

// ─── Insert / outcome parity ─────────────────────────────────────────────────

#[test]
fn insert_then_duplicate_id_returns_duplicate() {
    let (store, _dir) = open_tmp();
    let raw = signed_event(1, 1000, "first", None);

    let o1 = store
        .insert(verified(raw.clone()), &"wss://r1/".into(), 1_000_000)
        .expect("insert");
    assert!(
        matches!(o1, InsertOutcome::Inserted { .. }),
        "first must be Inserted, got {o1:?}"
    );

    let o2 = store
        .insert(verified(raw), &"wss://r2/".into(), 2_000_000)
        .expect("insert dup");
    assert!(
        matches!(o2, InsertOutcome::Duplicate { .. }),
        "re-delivery must be Duplicate, got {o2:?}"
    );
}

#[test]
fn replaceable_supersession_emits_replaced_then_superseded() {
    use nostr::Keys;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    let old = signed_event_with_keys(&keys, 0, 1000, "old", None);
    let new = signed_event_with_keys(&keys, 0, 2000, "new", None);
    let older = signed_event_with_keys(&keys, 0, 500, "older", None);

    let o1 = store
        .insert(verified(old), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(matches!(o1, InsertOutcome::Inserted { .. }), "1: {o1:?}");

    let o2 = store
        .insert(verified(new), &"wss://r/".into(), 2_000_000)
        .unwrap();
    assert!(matches!(o2, InsertOutcome::Replaced { .. }), "2: {o2:?}");

    let o3 = store
        .insert(verified(older), &"wss://r/".into(), 3_000_000)
        .unwrap();
    assert!(
        matches!(o3, InsertOutcome::Superseded { .. }),
        "3 (older): {o3:?}"
    );
}

#[test]
fn replaceable_dup_id_merges_provenance() {
    use nostr::Keys;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let raw = signed_event_with_keys(&keys, 0, 1000, "m", None);

    let id = raw.id_bytes().expect("fixture: valid hex");
    let o1 = store
        .insert(verified(raw.clone()), &"wss://r1/".into(), 1_000_000)
        .unwrap();
    assert!(matches!(o1, InsertOutcome::Inserted { .. }));

    let o2 = store
        .insert(verified(raw), &"wss://r2/".into(), 2_000_000)
        .unwrap();
    assert!(
        matches!(o2, InsertOutcome::Duplicate { .. }),
        "dup expected, got {o2:?}"
    );

    let prov = store.provenance_for(&id).unwrap();
    assert_eq!(prov.len(), 2, "both relays must be in provenance");
}

#[test]
fn ephemeral_kind_is_not_stored() {
    let (store, _dir) = open_tmp();
    let raw = signed_event(20_000, 1000, "ephemeral", None);
    let id = raw.id_bytes().expect("fixture: valid hex");
    let o = store
        .insert(verified(raw), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(matches!(o, InsertOutcome::Ephemeral { .. }), "got {o:?}");
    assert!(
        store.get_by_id(&id).unwrap().is_none(),
        "must not store ephemeral"
    );
}

#[test]
fn nip40_expired_on_arrival_rejected() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    // expiration tag at t=500 with received_at_ms => received_secs=1000.
    let ev = EventBuilder::text_note("expired")
        .tag(Tag::expiration(Timestamp::from_secs(500)))
        .custom_created_at(Timestamp::from_secs(100))
        .sign_with_keys(&keys)
        .unwrap();
    let json = ev.try_as_json().unwrap();
    let raw: RawEvent = serde_json::from_str(&json).unwrap();
    let id = raw.id_bytes().expect("fixture: valid hex");
    let o = store
        .insert(verified(raw), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(matches!(o, InsertOutcome::Rejected { .. }), "got {o:?}");
    assert!(
        store.get_by_id(&id).unwrap().is_none(),
        "expired not stored"
    );
}

// ─── query_visit parity ──────────────────────────────────────────────────────

#[test]
fn query_visit_early_stop_after_10() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    for i in 0..100u64 {
        let ev = EventBuilder::text_note(format!("n={i}"))
            .custom_created_at(Timestamp::from_secs(1_000_000 + i))
            .sign_with_keys(&keys)
            .unwrap();
        let json = ev.try_as_json().unwrap();
        let raw: RawEvent = serde_json::from_str(&json).unwrap();
        store
            .insert(verified(raw), &"wss://r/".into(), 1_000_000 + i)
            .unwrap();
    }
    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let mut visited = 0usize;
    store
        .query_visit(&q, 1000, &mut |_ev| {
            visited += 1;
            if visited >= 10 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        })
        .unwrap();
    assert_eq!(visited, 10, "visitor must stop after exactly 10 events");
}

#[test]
fn query_wrapper_newest_first_ordered() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(keys.public_key().to_bytes().as_slice());

    for i in 0..50u64 {
        let ev = EventBuilder::new(Kind::from(7u16), format!("r={i}"))
            .custom_created_at(Timestamp::from_secs(2_000_000 + i))
            .sign_with_keys(&keys)
            .unwrap();
        let json = ev.try_as_json().unwrap();
        let raw: RawEvent = serde_json::from_str(&json).unwrap();
        store
            .insert(verified(raw), &"wss://r/".into(), 2_000_000 + i)
            .unwrap();
    }
    let q = StoreQuery::AuthorKind {
        author: pk_bytes,
        kinds: vec![7],
        since: None,
        until: None,
    };
    let v = store.query(&q, 5).unwrap();
    assert_eq!(v.len(), 5, "limit must cap");
    for w in v.windows(2) {
        assert!(w[0].raw.created_at >= w[1].raw.created_at, "newest-first");
    }
    assert_eq!(v[0].raw.created_at, 2_000_049, "first must be newest");
}

// ─── Domain rows parity ──────────────────────────────────────────────────────

#[test]
fn domain_put_get_delete_scan_prefix() {
    let (store, _dir) = open_tmp();
    let h = store.domain_open("drafts").unwrap();
    h.put(b"key1", b"v1").unwrap();
    h.put(b"key2", b"v2").unwrap();
    h.put(b"other", b"v3").unwrap();

    assert_eq!(h.get(b"key1").unwrap().as_deref(), Some(&b"v1"[..]));
    assert_eq!(h.get(b"missing").unwrap(), None);

    // Prefix scan.
    let rows: Vec<(Vec<u8>, Vec<u8>)> =
        h.scan_prefix(b"key").unwrap().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 2, "prefix scan must find both `key*`");

    assert!(h.delete(b"key1").unwrap());
    assert_eq!(h.get(b"key1").unwrap(), None);
    assert!(!h.delete(b"key1").unwrap(), "second delete returns false");
}

#[test]
fn domain_isolation_two_namespaces() {
    let (store, _dir) = open_tmp();
    let a = store.domain_open("a").unwrap();
    let b = store.domain_open("b").unwrap();
    a.put(b"k", b"in-a").unwrap();
    b.put(b"k", b"in-b").unwrap();
    assert_eq!(a.get(b"k").unwrap().as_deref(), Some(&b"in-a"[..]));
    assert_eq!(b.get(b"k").unwrap().as_deref(), Some(&b"in-b"[..]));
}

// ─── Tombstone max-merge parity (Mem P2 test) ────────────────────────────────

#[test]
fn tombstone_max_merge_takes_newer_deleted_at() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let target = signed_event_with_keys(&keys, 1, 50, "doomed", None);
    let target_id = target.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(target), &"wss://r/".into(), 50_000)
        .unwrap();

    let k5a = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(100))
        .sign_with_keys(&keys)
        .unwrap();
    let r1: RawEvent = serde_json::from_str(&k5a.try_as_json().unwrap()).unwrap();
    store
        .insert(verified(r1), &"wss://r1/".into(), 100_000)
        .unwrap();

    let k5b = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(200))
        .sign_with_keys(&keys)
        .unwrap();
    let r2: RawEvent = serde_json::from_str(&k5b.try_as_json().unwrap()).unwrap();
    store
        .insert(verified(r2), &"wss://r2/".into(), 200_000)
        .unwrap();

    let tombs = store.tombstones_for(&target_id).unwrap();
    let tomb = tombs.first().expect("tombstone present");
    assert_eq!(tomb.deleted_at, 200, "max-merge must take newer deleted_at");
    assert!(tomb.sources.contains(&"wss://r1/".to_string()), "union r1");
    assert!(tomb.sources.contains(&"wss://r2/".to_string()), "union r2");
}

// ─── addr_tombstone GC tests (S-2 fix) ───────────────────────────────────────

/// Insert a kind:5 with an `a`-tag to create an addr tombstone, then run
/// gc_step far into the future — the stale addr tombstone must be purged.
///
/// This is the RED → GREEN proof for the S-2 audit finding.
#[test]
fn lmdb_stale_addr_tombstone_is_purged_by_gc() {
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_hex = keys.public_key().to_hex();

    // A parameterized-replaceable event (kind 30023) that gets deleted.
    let target = signed_event_with_keys(&keys, 30023, 1000, "article", Some("my-slug"));
    store
        .insert(verified(target), &"wss://r/".into(), 1_000_000)
        .unwrap();

    // kind:5 with an `a`-tag deleting the coordinate.
    let a_tag_value = format!("30023:{pk_hex}:my-slug");
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: RawEvent = serde_json::from_str(&k5_json).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // Addr tombstone must exist after the kind:5 insert.
    let count_before = store.addr_tombstone_count().unwrap();
    assert!(
        count_before >= 1,
        "addr tombstone must be written by kind:5 a-tag insert"
    );

    // GC with now_secs = deleted_at + TOMBSTONE_MAX_AGE_SECS + 1.
    // deleted_at = 2000 (kind5.created_at); age window = 90 * 24 * 3600.
    const MAX_AGE: u64 = 90 * 24 * 3600;
    let now_secs = 2000 + MAX_AGE + 1;
    let budget = crate::types::GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 10_000,
        max_total_events: usize::MAX,
    };
    let report = store.gc_step(budget, now_secs).unwrap();

    assert_eq!(
        store.addr_tombstone_count().unwrap(),
        0,
        "stale addr_tombstone must be purged by gc_step"
    );
    assert_eq!(
        report.addr_tombstones_purged, 1,
        "report must count purged addr_tombstone"
    );
}

/// A fresh addr tombstone (well within TOMBSTONE_MAX_AGE_SECS) must NOT be purged.
#[test]
fn lmdb_fresh_addr_tombstone_is_retained_by_gc() {
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_hex = keys.public_key().to_hex();

    let target = signed_event_with_keys(&keys, 30023, 1000, "article", Some("keep-slug"));
    store
        .insert(verified(target), &"wss://r/".into(), 1_000_000)
        .unwrap();

    let a_tag_value = format!("30023:{pk_hex}:keep-slug");
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: RawEvent = serde_json::from_str(&k5_json).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // GC with now_secs = deleted_at + 1 (far below the 90-day threshold).
    let budget = crate::types::GcBudget {
        max_events_per_step: 1000,
        max_duration_ms: 10_000,
        max_total_events: usize::MAX,
    };
    let report = store.gc_step(budget, 2001).unwrap();

    assert!(
        store.addr_tombstone_count().unwrap() >= 1,
        "fresh addr_tombstone must NOT be purged"
    );
    assert_eq!(
        report.addr_tombstones_purged, 0,
        "report must not count fresh addr_tombstone as purged"
    );
}
