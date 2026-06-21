//! ADR-0062 regression tests: inject-then-open → feed populates.
//!
//! Before ADR-0062, opening an author or thread feed AFTER the events were
//! already cached would result in an empty feed (the global fan-out had already
//! fired). After ADR-0062, the kernel replays `self.events` to the muted
//! observer during `OpenObservedInterest`.

use super::super::*;
use super::harness::*;
use std::ffi::CString;

use nmp_ffi::nmp_app_free;
use nostr::{EventBuilder, Keys, Tag, Timestamp};
use nostr::prelude::JsonUtil;

/// **Inject-then-open (author)**: events injected BEFORE `open_author_feed`
/// must appear in the feed via the kernel read-model catch-up path.
#[test]
fn author_inject_then_open_populates_feed() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    // Inject two events BEFORE opening the feed.
    let e1 = EventBuilder::text_note("older")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign e1");
    let e2 = EventBuilder::text_note("newer")
        .custom_created_at(Timestamp::from(2_000u64))
        .sign_with_keys(&keys)
        .expect("sign e2");

    inject_and_wait(app, &e1.as_json(), &e1.id.to_hex(), &rx);
    inject_and_wait(app, &e2.as_json(), &e2.id.to_hex(), &rx);

    // THEN open the feed. The kernel must replay both cached events.
    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());

    let key = author_feed_key(&pubkey);
    let ids = wait_for_feed_cards(app, &key, 2, &rx);

    assert_eq!(ids.len(), 2, "author feed must contain both injected events");
    assert!(ids.contains(&e1.id.to_hex()), "e1 must appear");
    assert!(ids.contains(&e2.id.to_hex()), "e2 must appear");

    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    nmp_app_free(app);
}

/// **Inject-then-open (thread)**: root and reply events cached BEFORE opening
/// the thread feed must appear in the feed via replay.
#[test]
fn thread_inject_then_open_populates_cached_root_and_replies() {
    let (app, rx, _tx_box) = start_app();

    let root_keys = Keys::generate();
    let reply_keys = Keys::generate();

    // Inject root.
    let root_ev = EventBuilder::text_note("root note")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&root_keys)
        .expect("sign root");
    let root_id = root_ev.id.to_hex();

    // Inject reply with #e tag referencing the root.
    let reply_ev = EventBuilder::text_note("reply")
        .tags([Tag::parse(["e", &root_id]).expect("e tag")])
        .custom_created_at(Timestamp::from(2_000u64))
        .sign_with_keys(&reply_keys)
        .expect("sign reply");

    inject_and_wait(app, &root_ev.as_json(), &root_id, &rx);
    inject_and_wait(app, &reply_ev.as_json(), &reply_ev.id.to_hex(), &rx);

    // Open thread feed AFTER injection.
    let root_c = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());

    let key = thread_feed_key(&root_id);
    let ids = wait_for_feed_cards(app, &key, 2, &rx);

    assert!(ids.contains(&root_id), "root must appear in thread feed");
    assert!(ids.contains(&reply_ev.id.to_hex()), "reply must appear in thread feed");

    let root_c2 = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_close_thread_feed(app, root_c2.as_ptr());
    nmp_app_free(app);
}

/// **Multi-owner**: two observers for the same author shape. The second open
/// (changed:false from `EnsureAbsent`) still replays the cached events.
#[test]
fn multi_owner_second_observer_hydrates_despite_changed_false() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    let ev = EventBuilder::text_note("note")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign");
    inject_and_wait(app, &ev.as_json(), &ev.id.to_hex(), &rx);

    // First open.
    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);
    wait_for_feed_cards(app, &key, 1, &rx);

    // Close and re-open (simulates a screen navigate-away/back).
    // On re-open, EnsureAbsent returns changed:false (slot still exists),
    // but the new observer must still replay.
    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());

    let pubkey_c3 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c3.as_ptr());
    let ids = wait_for_feed_cards(app, &key, 1, &rx);
    assert!(!ids.is_empty(), "re-opened feed must hydrate despite changed:false");

    let pubkey_c4 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c4.as_ptr());
    nmp_app_free(app);
}

/// **No double-delivery**: an event replayed at open time must not arrive
/// again when the feed is already open and a NEW event is ingested live.
#[test]
fn no_double_delivery_when_event_replayed_then_arrives_live() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    // Inject e1 BEFORE opening.
    let e1 = EventBuilder::text_note("pre-existing")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign e1");
    inject_and_wait(app, &e1.as_json(), &e1.id.to_hex(), &rx);

    // Open feed → e1 is replayed.
    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);
    let ids_before = wait_for_feed_cards(app, &key, 1, &rx);
    assert_eq!(ids_before.len(), 1, "exactly 1 event from replay");

    // Inject a NEW event AFTER open → arrives via live global fan-out.
    let e2 = EventBuilder::text_note("live")
        .custom_created_at(Timestamp::from(2_000u64))
        .sign_with_keys(&keys)
        .expect("sign e2");
    inject_and_wait(app, &e2.as_json(), &e2.id.to_hex(), &rx);
    let ids_after = wait_for_feed_cards(app, &key, 2, &rx);
    assert_eq!(ids_after.len(), 2, "2 events total — no double-delivery of e1");

    // e1 must appear exactly once.
    let e1_count = ids_after.iter().filter(|&id| id == &e1.id.to_hex()).count();
    assert_eq!(e1_count, 1, "e1 must appear exactly once (no double-delivery)");

    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    nmp_app_free(app);
}
