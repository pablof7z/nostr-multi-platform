//! Tests for the Chirp per-open author / thread flat-feed registration
//! (ADR-0042 §5.1, ADR-0058 §8 6B viewport grow wiring, ADR-0062 observer
//! catch-up).
//!
//! ## Harness
//!
//! Tests that need events to appear in the kernel read-cache (so the ADR-0062
//! replay path can deliver them to a newly-opened feed) use the ACTOR harness:
//!
//!   1. `nmp_app_new()` — allocate a fresh app.
//!   2. `nmp_app_set_update_callback(app, ctx, Some(cb))` — wire up a signal
//!      channel so we can block until the actor has processed a command.
//!   3. `nmp_app_start(app, 0, 80, 4)` — start the actor thread (no relays,
//!      visible limit 80, 4 Hz).
//!   4. Inject signed events via `nmp_app_inject_signed_event_json(app, json)`.
//!   5. Block on `recv_timeout` until `app.event_by_id(id).is_some()`.
//!   6. Open the feed → the kernel replay delivers cached events.
//!   7. Block on `recv_timeout` until the typed sidecar carries the expected ids.
//!
//! This is the same pattern used by `nmp-ffi/src/pull_tests.rs` and
//! `nmp-ffi/src/event_by_id_tests.rs`.
//!
//! Tests that only check compile-time invariants (key formatting, filter JSON
//! parsing) remain cheap and do not need the actor.

use super::*;
use std::ffi::{CString, c_void};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_inject_signed_event_json};
use nmp_ffi::{nmp_app_set_update_callback, nmp_app_start};
use nostr::{EventBuilder, Keys, Tag, Timestamp};
use nostr::prelude::JsonUtil;

// ─── Actor harness ────────────────────────────────────────────────────────────

extern "C" fn update_signal(ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    // ctx is a *mut Sender<()> (boxed, kept alive by the test).
    if ctx.is_null() {
        return;
    }
    let tx: &Sender<()> = unsafe { &*(ctx as *const Sender<()>) };
    let _ = tx.send(());
}

/// Start a fresh `NmpApp` with a signal channel. Returns `(app, rx, tx_box)`.
/// The caller must keep `tx_box` alive for the duration of the test (the
/// `set_update_callback` ctx pointer points into it).
fn start_app() -> (*mut NmpApp, Receiver<()>, Box<Sender<()>>) {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must succeed");
    let (tx, rx) = channel::<()>();
    let tx_box = Box::new(tx);
    let ctx = tx_box.as_ref() as *const Sender<()> as *mut c_void;
    nmp_app_set_update_callback(app, ctx, Some(update_signal));
    nmp_app_start(app, 80, 4);
    (app, rx, tx_box)
}

/// Inject a real Schnorr-signed event and block until the actor has made it
/// readable (i.e. it's in the kernel read-cache so the replay path can find it).
fn inject_and_wait(app: *mut NmpApp, json: &str, id: &str, rx: &Receiver<()>) {
    let json_c = CString::new(json).expect("event JSON");
    let ok = nmp_app_inject_signed_event_json(app, json_c.as_ptr());
    assert!(ok, "inject_signed_event_json must succeed for: {json}");
    let app_ref: &NmpApp = unsafe { &*app };
    if app_ref.event_by_id(id).is_some() {
        return;
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {
                if app_ref.event_by_id(id).is_some() {
                    return;
                }
            }
            Err(_) => panic!(
                "actor timed out making event {} readable",
                &id[..16.min(id.len())]
            ),
        }
    }
}

/// Block until the typed sidecar for `key` carries the expected number of
/// cards (or timeout). Returns the decoded card ids.
fn wait_for_feed_cards(
    app: *mut NmpApp,
    key: &str,
    expected_count: usize,
    rx: &Receiver<()>,
) -> Vec<String> {
    let app_ref: &NmpApp = unsafe { &*app };
    // Quick path: sidecar might already be populated.
    if let Some(ids) = read_typed_card_ids(app, key) {
        if ids.len() >= expected_count {
            return ids;
        }
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {
                if let Some(ids) = read_typed_card_ids(app, key) {
                    if ids.len() >= expected_count {
                        return ids;
                    }
                }
                let _ = app_ref; // suppress unused warning
            }
            Err(_) => {
                let ids = read_typed_card_ids(app, key).unwrap_or_default();
                panic!(
                    "timed out waiting for {} cards in feed {key} (got {})",
                    expected_count,
                    ids.len()
                );
            }
        }
    }
}

/// Read typed op-feed card ids for `key`.
fn read_typed_card_ids(app: *mut NmpApp, key: &str) -> Option<Vec<String>> {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    let entry = projections
        .iter()
        .find(|p| p.key == key && !p.payload.is_empty())?;
    let snapshot = nmp_nip01::op_feed::decode_op_feed_snapshot(&entry.payload).ok()?;
    let ids: Vec<String> = snapshot.cards.iter().map(|c| c.card.id.clone()).collect();
    if ids.is_empty() {
        None
    } else {
        Some(ids)
    }
}

/// Return `true` when the typed sidecar for `key` is absent or cleared.
fn typed_projection_is_gone(app: *mut NmpApp, key: &str) -> bool {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    projections
        .iter()
        .all(|p| p.key != key || p.payload.is_empty())
}

// ─── Unit-level key/shape tests (no actor needed) ────────────────────────────

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

// ─── ADR-0062 regression: inject-then-open → feed populates ─────────────────
//
// These are the critical regressions. Before ADR-0062, opening an author or
// thread feed AFTER the events were already cached would result in an empty
// feed (the global fan-out had already fired). After ADR-0062, the kernel
// replays `self.events` to the muted observer during `OpenObservedInterest`.

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

// ─── Existing tests migrated to actor harness ────────────────────────────────

#[test]
fn author_feed_open_seeds_kind1_and_close_removes_projection() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    let e1 = EventBuilder::text_note("older")
        .custom_created_at(Timestamp::from(10u64))
        .sign_with_keys(&keys)
        .expect("sign e1");
    let e2 = EventBuilder::text_note("newer")
        .custom_created_at(Timestamp::from(20u64))
        .sign_with_keys(&keys)
        .expect("sign e2");

    inject_and_wait(app, &e1.as_json(), &e1.id.to_hex(), &rx);
    inject_and_wait(app, &e2.as_json(), &e2.id.to_hex(), &rx);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);
    let ids = wait_for_feed_cards(app, &key, 2, &rx);
    assert_eq!(ids.len(), 2, "both events present after open");

    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    assert!(
        typed_projection_is_gone(app, &key),
        "author feed projection must be gone after close"
    );
    nmp_app_free(app);
}

#[test]
fn thread_feed_open_seeds_cached_root_and_replies() {
    let (app, rx, _tx_box) = start_app();

    let root_keys = Keys::generate();
    let reply_keys = Keys::generate();

    let root_ev = EventBuilder::text_note("root")
        .custom_created_at(Timestamp::from(10u64))
        .sign_with_keys(&root_keys)
        .expect("sign root");
    let root_id = root_ev.id.to_hex();

    let reply_ev = EventBuilder::text_note("reply")
        .tags([Tag::parse(["e", &root_id]).expect("e tag")])
        .custom_created_at(Timestamp::from(20u64))
        .sign_with_keys(&reply_keys)
        .expect("sign reply");

    inject_and_wait(app, &root_ev.as_json(), &root_id, &rx);
    inject_and_wait(app, &reply_ev.as_json(), &reply_ev.id.to_hex(), &rx);

    let root_c = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());
    let key = thread_feed_key(&root_id);
    let ids = wait_for_feed_cards(app, &key, 2, &rx);
    assert!(ids.contains(&root_id), "root present");
    assert!(ids.contains(&reply_ev.id.to_hex()), "reply present");

    let root_c2 = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_close_thread_feed(app, root_c2.as_ptr());
    assert!(
        typed_projection_is_gone(app, &key),
        "thread feed projection must be gone after close"
    );
    nmp_app_free(app);
}

// ─── ADR-0063 D7 (#1671 Lane H) — structural feed-author auto-resolve ─────────
//
// The coverage hole: a dynamic author/thread feed used to register ONLY a typed
// sidecar and NO feed-author provider, so its rendered authors never
// auto-resolved (blank avatars). The structural-pairing fix routes BOTH lanes
// through `register_feed_render_source`, so a sidecar can't exist without its
// provider. These tests prove the provider is present while the feed is open and
// gone after close (no leak), for BOTH the author and thread feeds.

/// Consumer id the kernel auto-resolves a feed's authors under.
fn feed_author_consumer(feed_key: &str) -> String {
    format!("feed-author:{feed_key}")
}

/// **Author feed structural pairing**: opening an author feed registers BOTH its
/// typed sidecar AND its feed-author provider under the same key; closing
/// removes both. The provider returns the feed's visible author (the injected
/// note's author), proving the rows auto-resolve through `resolve_ref`.
#[test]
fn author_feed_registers_author_provider_structurally_and_releases_on_close() {
    let (app, rx, _tx_box) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();
    let ev = EventBuilder::text_note("hi")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&keys)
        .expect("sign");
    inject_and_wait(app, &ev.as_json(), &ev.id.to_hex(), &rx);

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);
    // Wait until the card is actually in the feed window (so the provider has an
    // author to surface).
    let _ = wait_for_feed_cards(app, &key, 1, &rx);

    // STRUCTURAL: both lanes are registered under the SAME key.
    assert!(
        app_ref.registered_typed_projection_keys().contains(&key),
        "author feed typed sidecar must be registered"
    );
    let providers = app_ref.registered_feed_author_provider_keys();
    assert!(
        providers.contains(&key),
        "author feed MUST have a structurally-paired author provider (was the coverage hole)"
    );

    // The provider surfaces the feed's visible author → it auto-resolves.
    assert_eq!(
        app_ref.run_feed_author_provider_for_test(&key),
        vec![pubkey.clone()],
        "the author provider returns the visible author (auto-resolved via resolve_ref)"
    );
    // The consumer id is the documented feed-author scheme.
    assert_eq!(feed_author_consumer(&key), format!("feed-author:{key}"));

    // Close → BOTH lanes gone (no leak).
    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    assert!(
        !app_ref.registered_typed_projection_keys().contains(&key),
        "typed sidecar gone after close"
    );
    assert!(
        !app_ref.registered_feed_author_provider_keys().contains(&key),
        "author provider released after close (no leak)"
    );
    nmp_app_free(app);
}

/// **Thread feed structural pairing**: same guarantee for a thread feed — its
/// rendered authors (root author + reply author) auto-resolve via a
/// structurally-paired provider, released on close.
#[test]
fn thread_feed_registers_author_provider_structurally_and_releases_on_close() {
    let (app, rx, _tx_box) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };

    let root_keys = Keys::generate();
    let reply_keys = Keys::generate();
    let root_pubkey = root_keys.public_key().to_hex();
    let reply_pubkey = reply_keys.public_key().to_hex();

    let root_ev = EventBuilder::text_note("root")
        .custom_created_at(Timestamp::from(1_000u64))
        .sign_with_keys(&root_keys)
        .expect("sign root");
    let root_id = root_ev.id.to_hex();
    let reply_ev = EventBuilder::text_note("reply")
        .tags([Tag::parse(["e", &root_id]).expect("e tag")])
        .custom_created_at(Timestamp::from(2_000u64))
        .sign_with_keys(&reply_keys)
        .expect("sign reply");

    inject_and_wait(app, &root_ev.as_json(), &root_id, &rx);
    inject_and_wait(app, &reply_ev.as_json(), &reply_ev.id.to_hex(), &rx);

    let root_c = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_open_thread_feed(app, root_c.as_ptr());
    let key = thread_feed_key(&root_id);
    let _ = wait_for_feed_cards(app, &key, 2, &rx);

    // STRUCTURAL pairing present.
    assert!(app_ref.registered_typed_projection_keys().contains(&key));
    assert!(
        app_ref.registered_feed_author_provider_keys().contains(&key),
        "thread feed MUST have a structurally-paired author provider"
    );

    // Both rendered authors (root + reply) are surfaced for auto-resolve.
    let authors = app_ref.run_feed_author_provider_for_test(&key);
    assert!(
        authors.contains(&root_pubkey),
        "thread root author auto-resolves (got {authors:?})"
    );
    assert!(
        authors.contains(&reply_pubkey),
        "thread reply author auto-resolves (got {authors:?})"
    );

    // Close → released.
    let root_c2 = CString::new(root_id.clone()).unwrap();
    nmp_app_chirp_close_thread_feed(app, root_c2.as_ptr());
    assert!(!app_ref.registered_typed_projection_keys().contains(&key));
    assert!(
        !app_ref.registered_feed_author_provider_keys().contains(&key),
        "thread author provider released after close (no leak)"
    );
    nmp_app_free(app);
}

/// `load_older` drains a page over the pull substrate and grows the viewport.
#[test]
fn author_feed_load_older_grows_visible_window_past_first_page() {
    let (app, rx, _tx_box) = start_app();

    let keys = Keys::generate();
    let pubkey = keys.public_key().to_hex();

    let total = nmp_feed::DEFAULT_FEED_WINDOW_LIMIT + 20;
    let mut ids_in_order: Vec<String> = Vec::new();
    for i in 0..total {
        let ev = EventBuilder::text_note(format!("n{i}"))
            .custom_created_at(Timestamp::from(1_000u64 + i as u64))
            .sign_with_keys(&keys)
            .expect("sign");
        let id = ev.id.to_hex();
        inject_and_wait(app, &ev.as_json(), &id, &rx);
        ids_in_order.push(id);
    }

    let pubkey_c = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_open_author_feed(app, pubkey_c.as_ptr());
    let key = author_feed_key(&pubkey);

    // First page: DEFAULT_FEED_WINDOW_LIMIT events.
    let first = wait_for_feed_cards(app, &key, nmp_feed::DEFAULT_FEED_WINDOW_LIMIT, &rx);
    assert_eq!(
        first.len(),
        nmp_feed::DEFAULT_FEED_WINDOW_LIMIT,
        "sidecar emits only the first page before load_older"
    );

    let app_ref: &NmpApp = unsafe { &*app };
    assert!(
        app_ref.load_older_feed(&key),
        "load_older must drain + grow the viewport"
    );
    let grown = wait_for_feed_cards(app, &key, total, &rx);
    assert!(
        grown.len() > first.len(),
        "the emitted projection grew after load_older"
    );

    let pubkey_c2 = CString::new(pubkey.clone()).unwrap();
    nmp_app_chirp_close_author_feed(app, pubkey_c2.as_ptr());
    nmp_app_free(app);
}
