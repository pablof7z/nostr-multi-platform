//! LMDB-backend parity tests against `MemEventStore`.
//!
//! Mirrors `mem/tests.rs` — same scenarios, same expected outcomes. The
//! per-test fixture uses `tempfile::tempdir()` to spin up a fresh
//! `LmdbEventStore` so each test owns its own LMDB env.

#![cfg(feature = "lmdb-backend")]

use std::ops::ControlFlow;

use tempfile::tempdir;

use crate::store::types::{ClaimerId, InsertOutcome, RawEvent, StoreQuery, VerifiedEvent};
use crate::store::{EventStore, LmdbEventStore, StoreError};

fn open_tmp() -> (LmdbEventStore, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let store = LmdbEventStore::open(dir.path()).expect("open");
    (store, dir)
}

fn signed_event(kind: u32, created_at: u64, content: &str, d_tag: Option<&str>) -> RawEvent {
    use nostr::prelude::*;
    let keys = Keys::generate();
    let mut b = EventBuilder::new(Kind::from(kind as u16), content)
        .custom_created_at(Timestamp::from_secs(created_at));
    if let Some(d) = d_tag {
        b = b.tag(Tag::identifier(d));
    }
    let ev = b.sign_with_keys(&keys).expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

fn signed_event_with_keys(
    keys: &nostr::Keys,
    kind: u32,
    created_at: u64,
    content: &str,
    d_tag: Option<&str>,
) -> RawEvent {
    use nostr::prelude::*;
    let mut b = EventBuilder::new(Kind::from(kind as u16), content)
        .custom_created_at(Timestamp::from_secs(created_at));
    if let Some(d) = d_tag {
        b = b.tag(Tag::identifier(d));
    }
    let ev = b.sign_with_keys(keys).expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

fn verified(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}

// ─── Insert / outcome parity ─────────────────────────────────────────────────

#[test]
fn insert_then_duplicate_id_returns_duplicate() {
    let (store, _dir) = open_tmp();
    let raw = signed_event(1, 1000, "first", None);

    let o1 = store
        .insert(verified(raw.clone()), &"wss://r1/".into(), 1_000_000)
        .expect("insert");
    assert!(matches!(o1, InsertOutcome::Inserted { .. }), "first must be Inserted, got {o1:?}");

    let o2 = store
        .insert(verified(raw), &"wss://r2/".into(), 2_000_000)
        .expect("insert dup");
    assert!(matches!(o2, InsertOutcome::Duplicate { .. }), "re-delivery must be Duplicate, got {o2:?}");
}

#[test]
fn replaceable_supersession_emits_replaced_then_superseded() {
    use nostr::Keys;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    let old = signed_event_with_keys(&keys, 0, 1000, "old", None);
    let new = signed_event_with_keys(&keys, 0, 2000, "new", None);
    let older = signed_event_with_keys(&keys, 0, 500, "older", None);

    let o1 = store.insert(verified(old), &"wss://r/".into(), 1_000_000).unwrap();
    assert!(matches!(o1, InsertOutcome::Inserted { .. }), "1: {o1:?}");

    let o2 = store.insert(verified(new), &"wss://r/".into(), 2_000_000).unwrap();
    assert!(matches!(o2, InsertOutcome::Replaced { .. }), "2: {o2:?}");

    let o3 = store.insert(verified(older), &"wss://r/".into(), 3_000_000).unwrap();
    assert!(matches!(o3, InsertOutcome::Superseded { .. }), "3 (older): {o3:?}");
}

#[test]
fn replaceable_dup_id_merges_provenance() {
    use nostr::Keys;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let raw = signed_event_with_keys(&keys, 0, 1000, "m", None);

    let id = raw.id_bytes();
    let o1 = store.insert(verified(raw.clone()), &"wss://r1/".into(), 1_000_000).unwrap();
    assert!(matches!(o1, InsertOutcome::Inserted { .. }));

    let o2 = store.insert(verified(raw), &"wss://r2/".into(), 2_000_000).unwrap();
    assert!(matches!(o2, InsertOutcome::Duplicate { .. }), "dup expected, got {o2:?}");

    let prov = store.provenance_for(&id).unwrap();
    assert_eq!(prov.len(), 2, "both relays must be in provenance");
}

#[test]
fn ephemeral_kind_is_not_stored() {
    let (store, _dir) = open_tmp();
    let raw = signed_event(20_000, 1000, "ephemeral", None);
    let id = raw.id_bytes();
    let o = store.insert(verified(raw), &"wss://r/".into(), 1_000_000).unwrap();
    assert!(matches!(o, InsertOutcome::Ephemeral { .. }), "got {o:?}");
    assert!(store.get_by_id(&id).unwrap().is_none(), "must not store ephemeral");
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
    let id = raw.id_bytes();
    let o = store
        .insert(verified(raw), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(matches!(o, InsertOutcome::Rejected { .. }), "got {o:?}");
    assert!(store.get_by_id(&id).unwrap().is_none(), "expired not stored");
}

// ─── kind:5 deletion parity ──────────────────────────────────────────────────

#[test]
fn kind5_self_delete_e_tag_writes_tombstone() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    let target = signed_event_with_keys(&keys, 1, 1000, "doomed", None);
    let target_id = target.id_bytes();
    store.insert(verified(target.clone()), &"wss://r/".into(), 1_000_000).unwrap();

    // kind:5 referencing target.
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(
            nostr::EventId::from_slice(&target_id).unwrap(),
        ))
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: RawEvent = serde_json::from_str(&k5_json).unwrap();
    store.insert(verified(k5_raw), &"wss://r/".into(), 2_000_000).unwrap();

    // Tombstone present, target gone.
    let tombs = store.tombstones_for(&target_id).unwrap();
    assert!(!tombs.is_empty(), "tombstone must be recorded");
    assert!(store.get_by_id(&target_id).unwrap().is_none(), "target purged");

    // Re-delivery of the same target_id must surface as Tombstoned.
    let o = store.insert(verified(target), &"wss://r/".into(), 3_000_000).unwrap();
    assert!(matches!(o, InsertOutcome::Tombstoned { .. }), "got {o:?}");
}

#[test]
fn kind5_foreign_target_silently_skipped() {
    use nostr::prelude::*;
    let (store, _dir) = open_tmp();
    let alice = Keys::generate();
    let bob = Keys::generate();

    let alice_event = signed_event_with_keys(&alice, 1, 1000, "alice's note", None);
    let alice_id = alice_event.id_bytes();
    store.insert(verified(alice_event.clone()), &"wss://r/".into(), 1_000_000).unwrap();

    // Bob tries to delete Alice's event — must be silently skipped, NOT
    // rejected as InvalidDelete (parity with mem/insert.rs:271 continue).
    let foreign_k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&alice_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&bob)
        .unwrap();
    let json = foreign_k5.try_as_json().unwrap();
    let raw: RawEvent = serde_json::from_str(&json).unwrap();
    let o = store.insert(verified(raw), &"wss://r/".into(), 2_000_000).unwrap();
    // Bob's kind:5 itself is stored (it's a valid event of his), but the
    // foreign target is not deleted.
    assert!(
        matches!(o, InsertOutcome::Inserted { .. }),
        "foreign kind:5 must be stored, got {o:?}"
    );
    assert!(store.get_by_id(&alice_id).unwrap().is_some(),
        "alice's event must survive bob's foreign deletion attempt");
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
        store.insert(verified(raw), &"wss://r/".into(), 1_000_000 + i).unwrap();
    }
    let q = StoreQuery::KindTime { kinds: vec![1], since: None, until: None };
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
        store.insert(verified(raw), &"wss://r/".into(), 2_000_000 + i).unwrap();
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
        assert!(
            w[0].raw.created_at >= w[1].raw.created_at,
            "newest-first"
        );
    }
    assert_eq!(v[0].raw.created_at, 2_000_049, "first must be newest");
}

// ─── Claims parity ───────────────────────────────────────────────────────────

#[test]
fn claim_idempotent_reclaim_does_not_count() {
    let (store, _dir) = open_tmp();
    let c = ClaimerId(1);
    store.register_view_cover(c, 5).unwrap();
    let mut id = [0u8; 32];
    id[0] = 1;
    store.claim(c, &[id]).unwrap();
    store.claim(c, &[id]).unwrap();
    // No direct count exposure, so we exercise the budget: claim 5 unique ids
    // (4 fresh) — must succeed.
    let mut others = Vec::new();
    for b in 2..6u8 {
        let mut o = [0u8; 32];
        o[0] = b;
        others.push(o);
    }
    store.claim(c, &others).unwrap();
}

#[test]
fn claim_over_per_view_ceiling_returns_err() {
    let (store, _dir) = open_tmp();
    let c = ClaimerId(2);
    store.register_view_cover(c, 2).unwrap();
    let id1 = {
        let mut i = [0u8; 32];
        i[0] = 1;
        i
    };
    let id2 = {
        let mut i = [0u8; 32];
        i[0] = 2;
        i
    };
    let id3 = {
        let mut i = [0u8; 32];
        i[0] = 3;
        i
    };
    store.claim(c, &[id1, id2]).unwrap();
    let res = store.claim(c, &[id3]);
    assert!(
        matches!(res, Err(StoreError::OverPinned { .. })),
        "expected OverPinned, got {res:?}"
    );
}

#[test]
fn release_clears_claimer() {
    let (store, _dir) = open_tmp();
    let c = ClaimerId(3);
    store.register_view_cover(c, 100).unwrap();
    let mut id = [0u8; 32];
    id[0] = 7;
    store.claim(c, &[id]).unwrap();
    store.release(c).unwrap();
    // After release, the slot is free — re-registering and claiming a fresh
    // id must succeed without OverPinned.
    store.register_view_cover(c, 1).unwrap();
    store.claim(c, &[id]).unwrap();
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
    let rows: Vec<(Vec<u8>, Vec<u8>)> = h
        .scan_prefix(b"key")
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
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
    let target_id = target.id_bytes();
    store.insert(verified(target), &"wss://r/".into(), 50_000).unwrap();

    let k5a = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(100))
        .sign_with_keys(&keys)
        .unwrap();
    let r1: RawEvent = serde_json::from_str(&k5a.try_as_json().unwrap()).unwrap();
    store.insert(verified(r1), &"wss://r1/".into(), 100_000).unwrap();

    let k5b = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::event(nostr::EventId::from_slice(&target_id).unwrap()))
        .custom_created_at(Timestamp::from_secs(200))
        .sign_with_keys(&keys)
        .unwrap();
    let r2: RawEvent = serde_json::from_str(&k5b.try_as_json().unwrap()).unwrap();
    store.insert(verified(r2), &"wss://r2/".into(), 200_000).unwrap();

    let tombs = store.tombstones_for(&target_id).unwrap();
    let tomb = tombs.first().expect("tombstone present");
    assert_eq!(tomb.deleted_at, 200, "max-merge must take newer deleted_at");
    assert!(tomb.sources.contains(&"wss://r1/".to_string()), "union r1");
    assert!(tomb.sources.contains(&"wss://r2/".to_string()), "union r2");
}

// ─── Watermark round-trip ────────────────────────────────────────────────────

#[test]
fn watermark_round_trip() {
    use crate::store::types::{Coverage, SyncMethod, WatermarkKey, WatermarkRow};
    let (store, _dir) = open_tmp();
    let key = WatermarkKey {
        filter_hash: [0xab; 32],
        relay_url: "wss://r/".into(),
    };
    assert!(matches!(store.coverage(&key).unwrap(), Coverage::Unknown));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let row = WatermarkRow {
        key: key.clone(),
        synced_up_to: 12345,
        last_sync_method: SyncMethod::Negentropy,
        last_negentropy_state: Some(vec![1, 2, 3]),
        bytes_saved_vs_req: 1000,
        updated_at: now,
    };
    store.write_watermark(row.clone()).unwrap();
    let got = store.read_watermark(&key).unwrap().unwrap();
    assert_eq!(got.synced_up_to, 12345);
    assert_eq!(got.last_negentropy_state.as_deref(), Some(&[1u8, 2, 3][..]));
    assert!(matches!(
        store.coverage(&key).unwrap(),
        Coverage::CompleteAsOf(_)
    ));

    let listed = store
        .list_watermarks_for_relay("wss://r/")
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(listed.len(), 1);
}
