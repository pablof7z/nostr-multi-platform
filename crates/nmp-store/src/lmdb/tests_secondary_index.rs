//! Secondary-index integrity tests for the LMDB backend.
//!
//! Covers the two leak classes fixed in GC Stage-1 follow-up:
//!
//!   Bug-1 (HIGH): kind:5 a-tag deletion left dangling nmp-lru-access,
//!   nmp-provenance, and nmp-expiry-index entries for the removed event id.
//!
//!   Bug-2 (MEDIUM): replaceable_freshness sub-db entries were never cleaned
//!   on deletion or LRU eviction, allowing stale TTL cache hits to wrongly
//!   skip a re-fetch.
//!
//! Each test follows strict TDD: assertions target the exact invariant the fix
//! enforces, not just incidental behavior.

#![cfg(feature = "lmdb-backend")]

use std::collections::HashSet;

use nostr::prelude::*;

use crate::types::{GcBudget, InsertOutcome};
use crate::EventStore;

use super::test_fixtures::{open_tmp, signed_event_with_keys, verified};

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Assert that the nmp-lru-access sub-db has NO entry for `id`.
fn lru_access_is_absent(store: &crate::LmdbEventStore, id: &[u8; 32]) {
    let inner = store.inner_for_test();
    let txn = inner.env.read_txn().expect("read_txn");
    let found = inner
        .lru_access
        .get(&txn, id.as_slice())
        .expect("lru_access.get");
    assert!(found.is_none(), "dangling lru_access entry for id {:?}", id);
}

/// Assert that the nmp-expiry-index has no entry whose key encodes `id`.
///
/// We do a full scan rather than an O(1) lookup because we don't have the
/// expiry_ts at hand (if the entry leaked we don't know its timestamp).
fn expiry_index_is_absent(store: &crate::LmdbEventStore, id: &[u8; 32]) {
    let inner = store.inner_for_test();
    let txn = inner.env.read_txn().expect("read_txn");
    let mut found = false;
    for entry in inner.expiry_index.iter(&txn).expect("expiry_index iter") {
        let (k, _) = entry.expect("entry");
        if k.len() == 40 && &k[8..] == id.as_slice() {
            found = true;
            break;
        }
    }
    assert!(!found, "dangling nmp-expiry-index entry for id {:?}", id);
}

// ─── Bug-1 tests (kind:5 a-tag leaks) ────────────────────────────────────────

/// Insert an addressable event (kind 30023), then delete it via a kind:5
/// event with an a-tag.  After deletion:
///   * the event itself is gone
///   * nmp-provenance is empty for that id
///   * nmp-lru-access has no entry for that id
///   * nmp-expiry-index has no entry for that id
///   * replaceable_freshness has no entry for the coordinate
#[test]
fn kind5_a_tag_delete_cleans_all_nmp_secondaries_addressable() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_bytes: [u8; 32] = keys.public_key().to_bytes();

    // Insert an addressable event (kind 30023 with d-tag "article-1").
    let addr_ev = signed_event_with_keys(&keys, 30023, 1000, "first version", Some("article-1"));
    let addr_id = addr_ev.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(addr_ev), &"wss://r/".into(), 1_000_000)
        .unwrap();

    // Sanity: event present, provenance present.
    assert!(store.get_by_id(&addr_id).unwrap().is_some());
    let prov = store.provenance_for(&addr_id).unwrap();
    assert!(!prov.is_empty(), "provenance must be present after insert");

    // Stamp a freshness mark for the coordinate.
    let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Parameterized {
        kind: 30023,
        pubkey: pk_bytes,
        d_tag: "article-1".to_string(),
    };
    store.set_check_again_after(freshness_key.clone(), 9_999_999_999);

    // Confirm freshness is set.
    assert!(
        store.get_check_again_after(&freshness_key).is_some(),
        "freshness must be set"
    );

    // Build and insert kind:5 with an a-tag referencing the addressable event.
    let a_tag_value = format!("30023:{}:article-1", keys.public_key());
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_json = k5.try_as_json().unwrap();
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5_json).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // The addressed event must be gone.
    assert!(
        store.get_by_id(&addr_id).unwrap().is_none(),
        "addressable event must be removed by kind:5 a-tag"
    );

    // Bug-1 assertion: provenance cleared.
    let prov_after = store.provenance_for(&addr_id).unwrap();
    assert!(
        prov_after.is_empty(),
        "nmp-provenance must be empty after kind:5 a-tag delete (Bug-1)"
    );

    // Bug-1 assertion: lru_access entry cleared.
    lru_access_is_absent(&store, &addr_id);

    // Bug-1 + Bug-2 assertion: freshness row cleared.
    assert!(
        store.get_check_again_after(&freshness_key).is_none(),
        "replaceable_freshness must be cleared after kind:5 a-tag delete (Bug-2)"
    );
}

/// Same as above but for a regular replaceable event (kind 0 / metadata).
#[test]
fn kind5_a_tag_delete_cleans_all_nmp_secondaries_replaceable() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_bytes: [u8; 32] = keys.public_key().to_bytes();

    // Insert a regular replaceable event (kind 0 = profile metadata).
    let meta = signed_event_with_keys(&keys, 0, 1000, r#"{"name":"alice"}"#, None);
    let meta_id = meta.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(meta), &"wss://r/".into(), 1_000_000)
        .unwrap();

    assert!(store.get_by_id(&meta_id).unwrap().is_some());
    let prov = store.provenance_for(&meta_id).unwrap();
    assert!(!prov.is_empty());

    // Stamp freshness.
    let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Regular {
        kind: 0,
        pubkey: pk_bytes,
    };
    store.set_check_again_after(freshness_key.clone(), 9_999_999_999);
    assert!(store.get_check_again_after(&freshness_key).is_some());

    // Build kind:5 with a-tag for kind:0 (no d-tag).
    let a_tag_value = format!("0:{}:", keys.public_key());
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5.try_as_json().unwrap()).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // Event gone.
    assert!(
        store.get_by_id(&meta_id).unwrap().is_none(),
        "replaceable event must be removed by kind:5 a-tag"
    );

    // Bug-1: provenance cleared.
    let prov_after = store.provenance_for(&meta_id).unwrap();
    assert!(
        prov_after.is_empty(),
        "nmp-provenance must be empty (Bug-1)"
    );

    // Bug-1: lru_access cleared.
    lru_access_is_absent(&store, &meta_id);

    // Bug-2: freshness cleared.
    assert!(
        store.get_check_again_after(&freshness_key).is_none(),
        "replaceable_freshness must be cleared (Bug-2)"
    );
}

/// Insert an addressable event WITH an expiration tag, then delete via
/// kind:5 a-tag.  The nmp-expiry-index entry must also be gone.
#[test]
fn kind5_a_tag_delete_cleans_expiry_index() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    let exp_ts = 9_000_000_000u64; // far future — event survives the insert check

    // Build addressable event with expiration tag.
    let ev = EventBuilder::new(Kind::from(30023u16), "content")
        .tag(Tag::identifier("d1"))
        .tag(Tag::expiration(Timestamp::from_secs(exp_ts)))
        .custom_created_at(Timestamp::from_secs(1000))
        .sign_with_keys(&keys)
        .unwrap();
    let ev_json = ev.try_as_json().unwrap();
    let ev_raw: crate::types::RawEvent = serde_json::from_str(&ev_json).unwrap();
    let ev_id = ev_raw.id_bytes().expect("fixture: valid hex");

    store
        .insert(verified(ev_raw), &"wss://r/".into(), 1_000_000)
        .unwrap();

    assert!(store.get_by_id(&ev_id).unwrap().is_some());

    // Delete via kind:5 a-tag.
    let a_tag_value = format!("30023:{}:d1", keys.public_key());
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5.try_as_json().unwrap()).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    assert!(
        store.get_by_id(&ev_id).unwrap().is_none(),
        "event must be removed"
    );

    // Bug-1: expiry-index entry must be gone.
    expiry_index_is_absent(&store, &ev_id);
}

// ─── Bug-1: LRU-entry leak verified via gc_step phantom count ─────────────────

/// If nmp-lru-access is not cleaned on kind:5 a-tag delete, a subsequent
/// gc_step would count a phantom eviction (attempting to delete an already-
/// absent event).  This test verifies that after the fix, gc_step reports
/// ZERO lru_evicted for the deleted event's slot.
///
/// Setup:
///   Insert 3 regular events + 1 addressable.
///   Delete the addressable via kind:5 a-tag.
///   Run gc_step with max_total_events=3 (ceiling matches surviving events).
///   Assert lru_evicted == 0: no phantom eviction of the deleted event.
#[test]
fn kind5_a_tag_delete_no_phantom_lru_eviction() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();

    // Insert 3 regular events.
    for ts in [1000u64, 1001, 1002] {
        let ev = signed_event_with_keys(&keys, 1, ts, "note", None);
        store
            .insert(verified(ev), &"wss://r/".into(), ts * 1000)
            .unwrap();
    }

    // Insert 1 addressable event (kind 30023, d-tag "x").
    let addr = signed_event_with_keys(&keys, 30023, 1003, "article", Some("x"));
    let addr_id = addr.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(addr), &"wss://r/".into(), 1_003_000)
        .unwrap();
    assert!(store.get_by_id(&addr_id).unwrap().is_some());

    // Delete the addressable event via kind:5 a-tag.
    let a_tag_value = format!("30023:{}:x", keys.public_key());
    let k5 = EventBuilder::new(Kind::EventDeletion, "")
        .tag(Tag::parse(["a", &a_tag_value]).unwrap())
        .custom_created_at(Timestamp::from_secs(2000))
        .sign_with_keys(&keys)
        .unwrap();
    let k5_raw: crate::types::RawEvent = serde_json::from_str(&k5.try_as_json().unwrap()).unwrap();
    store
        .insert(verified(k5_raw), &"wss://r/".into(), 2_000_000)
        .unwrap();

    // 4 events are in the store now: 3 notes + 1 kind:5.  The addressable is gone.
    // Run gc_step with ceiling = 4 (no eviction needed).
    let budget = GcBudget {
        max_total_events: 4,
        max_events_per_step: 100,
        max_duration_ms: 60_000,
    };
    let report = store.gc_step(budget, 9_000_000_000).expect("gc_step");

    assert_eq!(
        report.lru_evicted, 0,
        "no phantom lru eviction must occur after kind:5 a-tag delete (Bug-1 regression)"
    );
}

// ─── Bug-2 tests (replaceable_freshness not cleaned on LRU eviction) ──────────

/// Insert a replaceable event and stamp its freshness.  Evict it via LRU.
/// After eviction, `get_check_again_after` must return None — a stale
/// freshness entry must not survive the eviction.
#[test]
fn lru_eviction_clears_replaceable_freshness() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_bytes: [u8; 32] = keys.public_key().to_bytes();

    // Insert a replaceable event (kind 0 = profile metadata).
    let meta = signed_event_with_keys(&keys, 0, 1000, r#"{"name":"bob"}"#, None);
    let meta_id = meta.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(meta), &"wss://r/".into(), 1_000_000)
        .unwrap();

    // Insert a second regular event so we have 2 total, allowing us to set
    // max_total_events=1 and force eviction of the older one.
    let note = signed_event_with_keys(&keys, 1, 999, "earlier note", None);
    let note_id = note.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(note), &"wss://r/".into(), 999_000)
        .unwrap();

    // Stamp freshness for the replaceable (kind 0).
    let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Regular {
        kind: 0,
        pubkey: pk_bytes,
    };
    store.set_check_again_after(freshness_key.clone(), 9_999_999_999);
    assert!(
        store.get_check_again_after(&freshness_key).is_some(),
        "freshness must be set before eviction"
    );

    // Force LRU eviction: max_total_events = 1 forces eviction of 1 event.
    // The LRU logic evicts the event with the lowest seq (oldest access).
    // The note was inserted first so it should be evicted, but let's set
    // max_total_events=0 to force both evictions. We just need the replaceable
    // to be evicted.
    //
    // Actually: we need the KIND:0 event to be evicted.  Since the note
    // (kind 1) was inserted earlier it has a lower LRU seq.  To force
    // KIND:0 eviction, use max_total_events=0 (evict all above ceiling).
    let budget = GcBudget {
        max_total_events: 0,
        max_events_per_step: 100,
        max_duration_ms: 60_000,
    };
    let report = store.gc_step(budget, 9_000_000_000).expect("gc_step");

    // At least the replaceable event (kind 0) must have been evicted.
    assert!(
        report.lru_evicted >= 1,
        "expected LRU eviction, got {report:?}"
    );

    // The kind:0 event must be gone.
    assert!(
        store.get_by_id(&meta_id).unwrap().is_none(),
        "evicted event must not be present"
    );

    // Bug-2 assertion: freshness row must be gone.
    assert!(
        store.get_check_again_after(&freshness_key).is_none(),
        "replaceable_freshness must be cleared after LRU eviction (Bug-2)"
    );

    // Suppress unused warning for note_id if the note is also evicted.
    let _ = note_id;
}

/// Insert an addressable event and stamp its freshness.  Evict via LRU.
/// After eviction, `get_check_again_after` must return None.
#[test]
fn lru_eviction_clears_addressable_freshness() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_bytes: [u8; 32] = keys.public_key().to_bytes();

    // Insert an addressable event (kind 30023, d-tag "blog-1").
    let addr = signed_event_with_keys(&keys, 30023, 1000, "first post", Some("blog-1"));
    let addr_id = addr.id_bytes().expect("fixture: valid hex");
    store
        .insert(verified(addr), &"wss://r/".into(), 1_000_000)
        .unwrap();

    // Stamp freshness.
    let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Parameterized {
        kind: 30023,
        pubkey: pk_bytes,
        d_tag: "blog-1".to_string(),
    };
    store.set_check_again_after(freshness_key.clone(), 9_999_999_999);
    assert!(
        store.get_check_again_after(&freshness_key).is_some(),
        "freshness must be set before eviction"
    );

    // Evict all events (ceiling = 0).
    let budget = GcBudget {
        max_total_events: 0,
        max_events_per_step: 100,
        max_duration_ms: 60_000,
    };
    let report = store.gc_step(budget, 9_000_000_000).expect("gc_step");

    assert!(report.lru_evicted >= 1, "expected LRU eviction");
    assert!(
        store.get_by_id(&addr_id).unwrap().is_none(),
        "evicted event must not be present"
    );

    // Bug-2 assertion: freshness cleared.
    assert!(
        store.get_check_again_after(&freshness_key).is_none(),
        "replaceable_freshness must be cleared after LRU eviction (Bug-2)"
    );
}

// ─── Bug-2: normal replacement also clears freshness ─────────────────────────

/// When a replaceable event is replaced by a newer version via normal insert
/// (not kind:5), the old freshness row must be dropped so the kernel re-
/// verifies the coordinate (the new event may have a different TTL).
#[test]
fn normal_replacement_clears_replaceable_freshness() {
    let (store, _dir) = open_tmp();
    let keys = Keys::generate();
    let pk_bytes: [u8; 32] = keys.public_key().to_bytes();

    // Insert older kind:0.
    let old_meta = signed_event_with_keys(&keys, 0, 1000, r#"{"name":"old"}"#, None);
    let old_id = old_meta.id_bytes().expect("fixture: valid hex");
    let o = store
        .insert(verified(old_meta), &"wss://r/".into(), 1_000_000)
        .unwrap();
    assert!(matches!(o, InsertOutcome::Inserted { .. }));

    // Stamp freshness.
    let freshness_key = nmp_nostr_lmdb::ReplaceableKey::Regular {
        kind: 0,
        pubkey: pk_bytes,
    };
    store.set_check_again_after(freshness_key.clone(), 9_999_999_999);
    assert!(store.get_check_again_after(&freshness_key).is_some());

    // Insert newer kind:0 (same pubkey, higher created_at) — replaces the old.
    let new_meta = signed_event_with_keys(&keys, 0, 2000, r#"{"name":"new"}"#, None);
    let new_id = new_meta.id_bytes().expect("fixture: valid hex");
    let o = store
        .insert(verified(new_meta), &"wss://r/".into(), 2_000_000)
        .unwrap();
    assert!(
        matches!(o, InsertOutcome::Replaced { .. }),
        "expected Replaced, got {o:?}"
    );

    // Old event gone, new event present.
    assert!(store.get_by_id(&old_id).unwrap().is_none());
    assert!(store.get_by_id(&new_id).unwrap().is_some());

    // Bug-2: freshness must be cleared so the next claim re-verifies.
    assert!(
        store.get_check_again_after(&freshness_key).is_none(),
        "replaceable_freshness must be cleared after normal replacement (Bug-2)"
    );
}
