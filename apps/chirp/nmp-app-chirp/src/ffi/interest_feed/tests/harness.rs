//! Shared actor-harness helpers for interest_feed tests.
//!
//! Tests that need events to appear in the kernel read-cache (so the ADR-0062
//! replay path can deliver them to a newly-opened feed) use the ACTOR harness:
//!
//!   1. `nmp_app_new()` — allocate a fresh app.
//!   2. `nmp_app_set_update_callback(app, ctx, Some(cb))` — wire up a signal
//!      channel so we can block until the actor has processed a command.
//!   3. `nmp_app_start(app, 80, 4)` — start the actor thread (no relays,
//!      visible limit 80, 4 Hz).
//!   4. Inject signed events via `nmp_app_inject_signed_event_json(app, json)`.
//!   5. Block on `recv_timeout` until `app.event_by_id(id).is_some()`.
//!   6. Open the feed → the kernel replay delivers cached events.
//!   7. Block on `recv_timeout` until the typed sidecar carries the expected ids.
//!
//! This is the same pattern used by `nmp-ffi/src/pull_tests.rs` and
//! `nmp-ffi/src/event_by_id_tests.rs`.

use super::super::*;
use std::ffi::{CString, c_void};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use nmp_ffi::{nmp_app_new, nmp_app_inject_signed_event_json};
use nmp_ffi::{nmp_app_set_update_callback, nmp_app_start};

pub(super) extern "C" fn update_signal(ctx: *mut c_void, _ptr: *const u8, _len: usize) {
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
pub(super) fn start_app() -> (*mut NmpApp, Receiver<()>, Box<Sender<()>>) {
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
pub(super) fn inject_and_wait(app: *mut NmpApp, json: &str, id: &str, rx: &Receiver<()>) {
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
pub(super) fn wait_for_feed_cards(
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
pub(super) fn read_typed_card_ids(app: *mut NmpApp, key: &str) -> Option<Vec<String>> {
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
pub(super) fn typed_projection_is_gone(app: *mut NmpApp, key: &str) -> bool {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    projections
        .iter()
        .all(|p| p.key != key || p.payload.is_empty())
}
