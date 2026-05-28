//! V-83 — `NmpApp::event_by_id()` exposes the kernel's `EventStore` as a
//! synchronous, host-thread event-by-id read seam (the OP-feed engine's repost
//! L-2/L-5 backward-hydration paths).
//!
//! These tests drive a REAL event ingest through the actor thread — not a direct
//! store poke — so they prove the store the host reads through is the very store
//! the actor publishes after kernel construction. A `Reset` rebuilds the kernel
//! (and hence the store); the publish-back slot is re-populated so the same
//! `NmpApp` handle keeps reading the live store, never an orphaned one.
//!
//! Unlike the V-82 `Arc::as_ptr` identity check (the slot the host holds IS the
//! slot the kernel writes), V-83's slot is empty until the actor publishes the
//! kernel-built store into it, so the meaningful proof is behavioural: ingest an
//! event, read it back by id across the boundary, and confirm `Reset` does not
//! strand the reader.

use super::*;
use crate::{nmp_app_free, nmp_app_new, nmp_app_start};
use nmp_core::ActorCommand;
use nostr::prelude::*;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Linearise the tests: they share a process-global update-signal channel.
static SERIAL: Mutex<()> = Mutex::new(());
/// `extern "C"` callbacks cannot capture; park the signal `Sender` in a static.
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

fn install_update_signal() -> std::sync::mpsc::Receiver<()> {
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

/// Build a real Schnorr-signed kind:1 event with a fixed key + timestamp and
/// return its `(hex_id, NIP-01 JSON)`. The id is the canonical event id the
/// store keys by, so the test can read it back via `event_by_id`.
fn signed_note(content: &str, created_at: u64) -> (String, String) {
    let keys = Keys::generate();
    let event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(&keys)
        .expect("sign note");
    (event.id.to_hex(), event.as_json())
}

/// Inject a signed event (NIP-01 JSON) through the actor's
/// `IngestPreVerifiedEvents` path, then block on `id` becoming readable through
/// `event_by_id`. Every ingest sets `changed_since_emit`, so the actor produces
/// ≥1 update frame; we re-check the store on each tick (the store insert
/// precedes the observer fan-out + emit, so a delivered tick means the event is
/// already readable). Panics on timeout — a hung actor, not a missing event.
fn inject_and_wait(app: *mut NmpApp, id: &str, json: &str, rx: &std::sync::mpsc::Receiver<()>) {
    let json_c = std::ffi::CString::new(json).expect("event json");
    let ok = super::nmp_app_inject_signed_event_json(app, json_c.as_ptr());
    assert!(ok, "signed event must pass verification + inject");
    let app_ref = super::app_ref(app).expect("app");
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
            Err(_) => panic!("actor never made the ingested event readable in time"),
        }
    }
}

#[test]
fn event_by_id_reads_ingested_event_across_the_actor_boundary() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));

    // Pre-start: the slot is empty → reads return None (the cold-start state,
    // matching the prior no-op the OP-feed composition root relied on).
    {
        let app_ref = super::app_ref(app).expect("app");
        let (id, _json) = signed_note("pre-start", 1_700_000_000);
        assert!(
            app_ref.event_by_id(&id).is_none(),
            "no store published before nmp_app_start → None"
        );
    }

    nmp_app_start(app, 0, 256, 4);

    let (id, json) = signed_note("a real ingested note", 1_700_000_100);
    inject_and_wait(app, &id, &json, &rx);

    let app_ref = super::app_ref(app).expect("app");
    let event = app_ref
        .event_by_id(&id)
        .expect("event_by_id resolves the just-ingested event across the actor boundary");
    assert_eq!(event.id, id, "id round-trips through the store read");
    assert_eq!(event.kind, 1, "kind preserved");
    assert_eq!(event.content, "a real ingested note", "content preserved");
    assert_eq!(event.created_at, 1_700_000_100, "created_at preserved");

    // An unknown id reads as None (and a malformed id too — no panic).
    let unknown = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    assert!(
        app_ref.event_by_id(unknown).is_none(),
        "unknown id → None"
    );
    assert!(
        app_ref.event_by_id("not-hex").is_none(),
        "malformed id → None (never panics across the FFI boundary)"
    );

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn event_by_id_survives_reset() {
    // The Reset trap (same shape as V-82): a rebuilt kernel mints a fresh store.
    // Without the Reset re-publish, the slot would retain a handle to the
    // discarded kernel's store and reads would go stale. This proves the SAME
    // `NmpApp` handle reads a post-Reset ingest.
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));

    nmp_app_start(app, 0, 256, 4);

    // Ingest pre-Reset event.
    let (id_before, json_before) = signed_note("before reset", 1_700_000_200);
    inject_and_wait(app, &id_before, &json_before, &rx);
    assert!(
        super::app_ref(app).expect("app").event_by_id(&id_before).is_some(),
        "pre-Reset event readable"
    );

    // Reset wipes all kernel state (and rebuilds the store with a fresh
    // in-memory backend, since no storage path is set).
    super::app_ref(app)
        .expect("app")
        .send_cmd(ActorCommand::Reset);

    // A post-Reset ingest is readable through the SAME `NmpApp` handle → the
    // slot was re-published against the rebuilt kernel's store (the load-bearing
    // V-83 property: the handle is NOT stranded on the discarded kernel's
    // store). `inject_and_wait` blocks until the new store has the event, which
    // also guarantees the Reset + re-publish has completed before the
    // state-wipe assertion below.
    let (id_after, json_after) = signed_note("after reset", 1_700_000_300);
    inject_and_wait(app, &id_after, &json_after, &rx);
    let event = super::app_ref(app)
        .expect("app")
        .event_by_id(&id_after)
        .expect("post-Reset event readable via the re-published store handle");
    assert_eq!(event.content, "after reset");

    // And the pre-Reset event is gone from the fresh store (state wipe). By now
    // the re-publish has definitely landed (the post-Reset event above resolved
    // through it), so reading the OLD id against the NEW store is a clean check
    // that the handle points at the rebuilt store, not the orphaned one.
    assert!(
        super::app_ref(app)
            .expect("app")
            .event_by_id(&id_before)
            .is_none(),
        "Reset wiped the store → the pre-Reset event is gone (handle reads the rebuilt store)"
    );

    nmp_app_free(app);
    uninstall_update_signal();
}
