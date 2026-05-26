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

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_snapshot, nmp_app_chirp_snapshot_free,
    nmp_app_chirp_snapshot_window, nmp_app_chirp_unregister, ChirpTimelineSnapshot,
};
use nmp_core::store::{RawEvent, VerifiedEvent};
use nmp_core::ActorCommand;
use nmp_ffi::{nmp_app_free, nmp_app_new, nmp_app_start};
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

fn snapshot_for(handle: *mut nmp_app_chirp::ChirpHandle) -> ChirpTimelineSnapshot {
    let ptr = nmp_app_chirp_snapshot(handle);
    assert!(!ptr.is_null(), "snapshot returned null");
    // SAFETY: ptr is a valid C string from our own CString.
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("snapshot JSON is utf8")
        .to_owned();
    nmp_app_chirp_snapshot_free(ptr);
    serde_json::from_str(&json).expect("snapshot deserializes")
}

fn window_snapshot_for(
    handle: *mut nmp_app_chirp::ChirpHandle,
    request_json: &str,
) -> ChirpTimelineSnapshot {
    let request = CString::new(request_json).expect("request contains no nul");
    let ptr = nmp_app_chirp_snapshot_window(handle, request.as_ptr());
    assert!(!ptr.is_null(), "window snapshot returned null");
    // SAFETY: ptr is a valid C string from our own CString.
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("snapshot JSON is utf8")
        .to_owned();
    nmp_app_chirp_snapshot_free(ptr);
    serde_json::from_str(&json).expect("snapshot deserializes")
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
fn root_plus_reply_round_trip_through_ffi_snapshot() {
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

    let snap = snapshot_for(handle);
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

    let snap = snapshot_for(handle);
    assert_eq!(snap.blocks.len(), 1);
    assert!(matches!(snap.blocks[0], TimelineBlock::Standalone(_)));
    assert_eq!(snap.cards.len(), 1);

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn window_snapshot_returns_bounded_newest_blocks() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    nmp_app_start(app, 0, 80, 4);
    let handle = nmp_app_chirp_register(app, std::ptr::null());

    let author = "c".repeat(64);
    let old_id = "4".repeat(64);
    let new_id = "5".repeat(64);
    let old = VerifiedEvent::from_raw_unchecked(raw_note(&old_id, &author, 1, vec![], "old"));
    let new = VerifiedEvent::from_raw_unchecked(raw_note(&new_id, &author, 2, vec![], "new"));
    inject(app, vec![new, old]);
    std::thread::sleep(Duration::from_millis(500));

    let snap = window_snapshot_for(handle, r#"{"limit":1}"#);
    assert_eq!(snap.blocks.len(), 1);
    assert!(matches!(&snap.blocks[0], TimelineBlock::Standalone(id) if id == &new_id));
    assert_eq!(snap.cards.len(), 1);
    let page = snap.page.expect("window snapshot carries page metadata");
    assert!(page.has_more);
    assert_eq!(page.total_blocks, 2);
    assert_eq!(page.next_cursor.expect("cursor").id, new_id);

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
