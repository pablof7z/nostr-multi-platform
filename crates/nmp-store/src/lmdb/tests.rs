//! LMDB-backend parity tests against `MemEventStore`.
//!
//! Mirrors `mem/tests/` — same scenarios, same expected outcomes. The
//! per-test fixture uses `tempfile::tempdir()` to spin up a fresh
//! `LmdbEventStore` so each test owns its own LMDB env.
//!
//! Sibling files (split to stay under the 500-LOC hard cap):
//!   tests_kind5.rs          — kind:5 deletion scenarios
//!   tests_authors_kind.rs   — `StoreQuery::AuthorsKind` multi-author query parity
//!   tests_addr_tombstone.rs — addr-tombstone GC (S-2 audit fix)

#![cfg(feature = "lmdb-backend")]

use std::ops::ControlFlow;
use std::sync::Mutex;

use crate::types::{InsertOutcome, RawEvent, StoreQuery};
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event, signed_event_with_keys, verified};

static QUERY_VISIT_SERIAL: Mutex<()> = Mutex::new(());

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
    let _guard = QUERY_VISIT_SERIAL
        .lock()
        .expect("query_visit tests must serialize the global conversion counter seam");
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

// ─── AuthorsKind multi-author query tests — see tests_authors_kind.rs ────────

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

// ─── Streaming query_visit tests (#1516) ─────────────────────────────────────

/// Proves that materialization is lazy: breaking at the 10th event must not
/// pay more than 10+1 conversions (the +1 accounts for one possible tie-group
/// boundary read).
#[test]
fn streaming_visit_does_not_over_materialize() {
    let _guard = QUERY_VISIT_SERIAL
        .lock()
        .expect("query_visit tests must serialize the global conversion counter seam");
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    // 1 000 events, each with a distinct decreasing created_at so no ties.
    for i in 0..1000u64 {
        let ev = EventBuilder::text_note(format!("n={i}"))
            .custom_created_at(Timestamp::from_secs(2_000_000 - i))
            .sign_with_keys(&keys)
            .unwrap();
        let json = ev.try_as_json().unwrap();
        let raw: RawEvent = serde_json::from_str(&json).unwrap();
        store
            .insert(verified(raw), &"wss://r/".into(), 2_000_000 - i)
            .unwrap();
    }

    super::query::reset_conversion_count();

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

    assert_eq!(visited, 10, "must visit exactly 10 events");
    let conversions = super::query::conversion_count();
    assert!(
        conversions <= 11,
        "streaming must convert at most 11 rows for a break-at-10 query (got {conversions})"
    );
}

/// Events sharing the same `created_at` must be delivered id-ascending,
/// matching the MemEventStore tie-break contract.
#[test]
fn tie_break_order_matches_mem() {
    let _guard = QUERY_VISIT_SERIAL
        .lock()
        .expect("query_visit tests must serialize the global conversion counter seam");
    use nostr::prelude::*;

    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    // Four events sharing the same created_at → ordering is id-asc only.
    let mut expected_ids: Vec<String> = Vec::new();
    for i in 0..4u32 {
        let ev = EventBuilder::new(Kind::from(1u16), format!("tie-{i}"))
            .custom_created_at(Timestamp::from_secs(5_000_000))
            .sign_with_keys(&keys)
            .unwrap();
        expected_ids.push(ev.id.to_hex());
        let json = ev.try_as_json().unwrap();
        let raw: RawEvent = serde_json::from_str(&json).unwrap();
        store
            .insert(verified(raw), &"wss://r/".into(), 5_000_000)
            .unwrap();
    }
    expected_ids.sort(); // id-asc is the tie-break

    let q = StoreQuery::KindTime {
        kinds: vec![1],
        since: None,
        until: None,
    };
    let mut got_ids: Vec<String> = Vec::new();
    store
        .query_visit(&q, 100, &mut |ev| {
            got_ids.push(ev.raw.id.clone());
            ControlFlow::Continue(())
        })
        .unwrap();

    assert_eq!(
        got_ids, expected_ids,
        "equal-created_at events must be delivered id-ascending (MemEventStore tie-break parity)"
    );
}

// ─── addr_tombstone GC tests (S-2 fix) — see tests_addr_tombstone.rs ────────
