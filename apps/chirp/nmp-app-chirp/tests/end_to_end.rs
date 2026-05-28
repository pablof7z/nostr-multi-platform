//! End-to-end: register Chirp through the FFI, drive synthetic kind:1
//! events through the actor's `IngestPreVerifiedEvents` channel, decode the
//! snapshot, assert the modular blocks land.
//!
//! Mirrors the production flow `KernelBridge.swift` will take: open an
//! `NmpApp`, register the projection, watch ingest fan-out feed the
//! grouper, render the resulting blocks. Bypasses the relay layer by
//! pushing pre-verified events directly into the actor command channel —
//! `nmp-core`'s public `actor_sender()` exposes this for cross-crate tests
//! (the production wire path makes the same `ActorCommand` enqueue).

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
use nmp_threading::TimelineBlock;

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
fn root_plus_reply_round_trip_through_feed_projection() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    nmp_app_start(app, 0, 80, 4);

    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null(), "register returned null");

    // Root + one reply with NIP-10 marked root/reply tags pointing at the
    // root. The grouper should fold the two events into one Module.
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
    assert_eq!(snap.blocks.len(), 1, "expected one block, got {snap:?}");
    match &snap.blocks[0] {
        TimelineBlock::Module { events, .. } => {
            assert_eq!(events.len(), 2, "module must contain both events");
            assert!(events.contains(&root_id), "module must contain root id");
            assert!(events.contains(&reply_id), "module must contain reply id");
        }
        other => panic!("expected Module, got {other:?}"),
    }
    assert_eq!(snap.cards.len(), 2, "two cards (root + reply)");

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn standalone_note_renders_as_standalone_block() {
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
    assert_eq!(snap.blocks.len(), 1);
    assert!(matches!(snap.blocks[0], TimelineBlock::Standalone { .. }));
    assert_eq!(snap.cards.len(), 1);

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn snapshot_returns_default_window_and_load_older_expands_it() {
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
    assert_eq!(snap.blocks.len(), DEFAULT_TIMELINE_WINDOW_LIMIT);
    let page = snap.page.expect("window snapshot carries page metadata");
    assert!(snap.metrics.is_some(), "window snapshot carries metrics");
    assert!(page.has_more);
    assert_eq!(page.total_blocks, total);

    let key = CString::new("nmp.feed.home").expect("static key has no nul");
    nmp_app_load_older_feed(app, key.as_ptr());
    let expanded = feed_projection_for(app);
    assert_eq!(expanded.blocks.len(), total);
    assert!(!expanded.page.expect("expanded page").has_more);

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
