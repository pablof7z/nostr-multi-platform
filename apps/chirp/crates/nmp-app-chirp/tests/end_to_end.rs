//! End-to-end: register Chirp through the FFI, drive synthetic kind:1
//! events through the actor's `IngestPreVerifiedEvents` channel, decode the
//! `"nmp.feed.home"` projection, and assert the OP-centric `RootFeedSnapshot`
//! lands.
//!
//! V-80 rung 7 — the home feed is now thread-ROOTS-only, produced by the
//! `nmp-nip01` OP-feed engine (via `nmp_app_template::register_op_feed_defaults`
//! wired in `nmp_app_chirp_register`). Replies no longer appear as their own
//! rows; a followed author's reply attributes back to its root. These tests run
//! with NO signed-in account, so the follow set is empty and the follow
//! predicate is universally false: every injected reply is dropped (no
//! attribution, no row), while every root-shaped event surfaces as a card.
//!
//! Mirrors the production flow `KernelBridge.swift` takes: open an `NmpApp`,
//! register the projection, watch ingest fan-out feed the engine, read the
//! snapshot. Bypasses the relay layer by pushing pre-verified events directly
//! into the actor command channel (`IngestPreVerifiedEvents` routes through
//! `kernel.ingest_pre_verified_event`, which fans out to observers without the
//! `timeline_authors` store gate — so the engine sees every injected event).

use std::ffi::{CStr, CString};
use std::sync::Mutex;
use std::time::Duration;

use nmp_app_chirp::{nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpTimelineSnapshot};
use nmp_core::store::{RawEvent, VerifiedEvent};
use nmp_core::ActorCommand;
use nmp_ffi::{
    nmp_app_free, nmp_app_free_string, nmp_app_load_older_feed, nmp_app_new,
    nmp_app_read_projection_json, nmp_app_start,
};
use nmp_nip01::DEFAULT_TIMELINE_WINDOW_LIMIT;

// Serialize tests because `NmpApp` initialisation spawns process-global
// actor threads; staggering avoids cross-test interference.
static SERIAL: Mutex<()> = Mutex::new(());

fn raw_note(id: &str, author: &str, ts: u64, tags: Vec<Vec<String>>, content: &str) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: ts,
        kind: 1,
        tags,
        content: content.to_string(),
        sig: "a".repeat(128),
    }
}

fn feed_projection_for(app: *mut nmp_ffi::NmpApp) -> ChirpTimelineSnapshot {
    let key = CString::new("nmp.feed.home").expect("static key has no nul");
    let ptr = nmp_app_read_projection_json(app, key.as_ptr());
    assert!(!ptr.is_null(), "home feed projection returned null");
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("projection JSON is utf8")
        .to_owned();
    nmp_app_free_string(ptr);
    serde_json::from_str(&json).expect("projection deserializes")
}

fn inject(app: *mut nmp_ffi::NmpApp, events: Vec<VerifiedEvent>) {
    // SAFETY: `app` is a valid `*mut NmpApp` for the duration of this call
    // — caller passes the same handle they got from `nmp_app_new`.
    let app_ref = unsafe { &*app };
    let tx = app_ref.actor_sender();
    tx.send(ActorCommand::IngestPreVerifiedEvents(events))
        .expect("actor command channel open");
}

#[test]
fn root_surfaces_and_unfollowed_reply_is_dropped() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    nmp_app_start(app, 0, 80, 4);

    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null(), "register returned null");

    // Root + one NIP-10-marked reply pointing at the root. With no signed-in
    // account the follow predicate is universally false, so the reply is
    // dropped: the feed shows ONLY the root, with no attribution.
    let root_id = "1".repeat(64);
    let reply_id = "2".repeat(64);
    let author = "a".repeat(64);
    let root = VerifiedEvent::from_raw_unchecked(raw_note(&root_id, &author, 1, vec![], "root"));
    let reply = VerifiedEvent::from_raw_unchecked(raw_note(
        &reply_id,
        &author,
        2,
        vec![
            vec!["e".into(), root_id.clone(), "".into(), "root".into()],
            vec!["e".into(), root_id.clone(), "".into(), "reply".into()],
        ],
        "reply",
    ));

    inject(app, vec![root, reply]);

    // Wait for the actor to drain the injection. Two events are tiny; the
    // 250ms idle tick should be plenty even on a loaded CI machine.
    std::thread::sleep(Duration::from_millis(500));

    let snap = feed_projection_for(app);
    assert_eq!(
        snap.cards.len(),
        1,
        "feed is roots-only: the root surfaces, the reply does not get a row; got {snap:?}"
    );
    assert_eq!(snap.cards[0].card.id, root_id, "the surfaced card is the root");
    assert!(
        snap.cards[0].attribution.is_empty(),
        "the reply is from a non-followed author → no attribution attaches"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn standalone_note_renders_as_root_card() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    nmp_app_start(app, 0, 80, 4);
    let handle = nmp_app_chirp_register(app, std::ptr::null());

    let id = "3".repeat(64);
    let author = "b".repeat(64);
    let note = VerifiedEvent::from_raw_unchecked(raw_note(&id, &author, 1, vec![], "lone note"));
    inject(app, vec![note]);
    std::thread::sleep(Duration::from_millis(500));

    let snap = feed_projection_for(app);
    assert_eq!(snap.cards.len(), 1, "one root card");
    assert_eq!(snap.cards[0].card.id, id);
    assert!(snap.cards[0].attribution.is_empty());

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn snapshot_returns_default_window() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    nmp_app_start(app, 0, 80, 4);
    let handle = nmp_app_chirp_register(app, std::ptr::null());

    let author = "c".repeat(64);
    let total = DEFAULT_TIMELINE_WINDOW_LIMIT + 2;
    let events = (0..total)
        .map(|idx| {
            let id = format!("{:064x}", idx + 1);
            VerifiedEvent::from_raw_unchecked(raw_note(
                &id,
                &author,
                (idx + 1) as u64,
                vec![],
                "note",
            ))
        })
        .collect::<Vec<_>>();
    inject(app, events);
    std::thread::sleep(Duration::from_millis(500));

    let snap = feed_projection_for(app);
    assert_eq!(
        snap.cards.len(),
        DEFAULT_TIMELINE_WINDOW_LIMIT,
        "the default window caps the visible root cards"
    );
    let page = snap.page.expect("window snapshot carries page metadata");
    // The engine's RootFeedSnapshot does not emit per-tick timing metrics (V-80
    // §3-G removed them from this surface); it is always `None` here.
    assert!(snap.metrics.is_none(), "engine snapshot carries no metrics");
    assert!(page.has_more);
    assert_eq!(page.total_blocks, total);

    // `nmp_app_load_older_feed` is a no-op for the OP engine: its
    // `FeedController::load_older` returns `false` and `snapshot_json` always
    // serializes the default window (the engine holds every root bounded by D5
    // but does not yet grow the request limit on load-older). The previous
    // `ModularTimelineProjection` grew its window here; window-growth for the
    // OP engine is a separate follow-up. Assert the no-op so the behaviour is
    // pinned, not silently regressed.
    let key = CString::new("nmp.feed.home").expect("static key has no nul");
    nmp_app_load_older_feed(app, key.as_ptr());
    let after = feed_projection_for(app);
    assert_eq!(
        after.cards.len(),
        DEFAULT_TIMELINE_WINDOW_LIMIT,
        "window stays at the default after a no-op load-older"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
