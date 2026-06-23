//! D13 sign-and-return — end-to-end FFI integration for
//! `nmp_app_sign_event_for_return`.
//!
//! These drive a REAL sign-in and a REAL sign-and-return through the actor
//! thread, then read the signed event back out of the `signed_events` snapshot
//! projection — the exact path the podcast player's Blossom upload / feedback
//! flows take. They prove the host never needs raw private key bytes to obtain
//! a signed auth event (D13): the kernel signs with its own key material and
//! hands the flat NIP-01 JSON back through the projection.
//!
//! The local-nsec path resolves synchronously in the dispatch arm, so a single
//! update tick carries the result (no NIP-46 broker is wired in-process; the
//! `PendingSignReturn` idle-loop path is covered by the `pending_sign` unit
//! tests in `nmp-core`).

use super::*;
use crate::{nmp_app_free, nmp_app_new, nmp_app_start};
use std::ffi::c_void;
use nmp_core::decode_snapshot_typed_projections;
use nmp_core::typed_projections::{decode_signed_events, SignedEventRow, SIGNED_EVENTS_SCHEMA_ID};
use nostr::prelude::*;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Linearise: these tests share a process-global frame-capture channel.
static SERIAL: Mutex<()> = Mutex::new(());
/// `extern "C"` callbacks cannot capture, so park the frame `Sender` in a
/// static and forward every emitted snapshot frame's raw bytes through it.
static FRAME_TX: OnceLock<Mutex<Option<Sender<Vec<u8>>>>> = OnceLock::new();

extern "C" fn capture_frame_callback(_ctx: *mut c_void, ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    // SAFETY: the actor hands a valid (ptr, len) for the duration of the call;
    // we copy the bytes out immediately and never retain the pointer.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    if let Some(slot) = FRAME_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(bytes);
            }
        }
    }
}

fn install_frame_capture() -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = channel::<Vec<u8>>();
    let slot = FRAME_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_frame_capture() {
    if let Some(slot) = FRAME_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

fn hex_pubkey(nsec: &str) -> String {
    let sk = SecretKey::parse(nsec).expect("valid nsec");
    Keys::new(sk).public_key().to_hex()
}

/// Drain emitted frames until the typed `signed_events` sidecar carries an
/// entry for `correlation_id`, returning that typed row. Errors on timeout so
/// a hung actor is not mistaken for a legitimately-absent key.
///
/// PR-B (#991/#979): reads the typed FlatBuffers sidecar via
/// `decode_signed_events` — the generic JSON payload no longer exists.
fn wait_for_signed_event(
    rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    correlation_id: &str,
) -> Result<SignedEventRow, ()> {
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(bytes) => {
                let Ok(typed) = decode_snapshot_typed_projections(&bytes) else {
                    continue;
                };
                let Some(sidecar) = typed.iter().find(|t| t.key == SIGNED_EVENTS_SCHEMA_ID) else {
                    continue;
                };
                let Ok(model) = decode_signed_events(&sidecar.payload) else {
                    continue;
                };
                if let Some((_, row)) =
                    model.entries.into_iter().find(|(key, _)| key == correlation_id)
                {
                    return Ok(row);
                }
            }
            Err(_) => return Err(()),
        }
    }
}

#[test]
fn sign_event_for_return_signs_with_active_local_key_and_returns_flat_json() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_frame_capture();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_frame_callback));
    nmp_app_start(app, 256, 4);

    // Sign in a local nsec — the active account that will sign the draft.
    let secret = std::ffi::CString::new(TEST_NSEC).unwrap();
    super::nmp_app_signin_nsec(app, secret.as_ptr(), 1);

    // Request a kind:24242 Blossom auth event signed by the active account
    // (empty pubkey = active). The draft carries no pubkey; the kernel fills it.
    let draft = r#"{"kind":24242,"content":"Upload image","tags":[["t","upload"],["x","deadbeef"]],"created_at":0}"#;
    let empty = std::ffi::CString::new("").unwrap();
    let draft_c = std::ffi::CString::new(draft).unwrap();
    let cid_ptr =
        super::identity::nmp_app_sign_event_for_return(app, empty.as_ptr(), draft_c.as_ptr());
    assert!(!cid_ptr.is_null(), "a correlation_id C string is returned");
    let correlation_id = unsafe { std::ffi::CStr::from_ptr(cid_ptr) }
        .to_str()
        .expect("utf-8 correlation_id")
        .to_string();
    assert!(!correlation_id.is_empty(), "the correlation_id is non-empty");
    super::free::nmp_free_string(cid_ptr);

    let entry =
        wait_for_signed_event(&rx, &correlation_id).expect("the signed event must surface in time");

    assert!(entry.ok, "a local-key sign succeeds");
    let signed_json = entry.signed_json.as_deref().expect("signed_json present on success");
    let event: serde_json::Value =
        serde_json::from_str(signed_json).expect("signed_json is valid JSON");

    // Flat NIP-01 shape — the host base64-encodes this for a Blossom header.
    assert_eq!(event.get("kind").and_then(serde_json::Value::as_u64), Some(24242));
    assert_eq!(
        event.get("pubkey").and_then(serde_json::Value::as_str),
        Some(hex_pubkey(TEST_NSEC).as_str()),
        "the kernel filled the active account's pubkey"
    );
    assert!(
        event.get("created_at").and_then(serde_json::Value::as_u64).unwrap_or(0) > 0,
        "created_at is re-stamped from the kernel clock (D7), not the draft's 0"
    );
    assert!(
        event.get("id").and_then(serde_json::Value::as_str).is_some(),
        "the event carries a computed id"
    );
    assert!(
        event.get("sig").and_then(serde_json::Value::as_str).is_some(),
        "the event carries a signature"
    );
    let tags = event.get("tags").and_then(serde_json::Value::as_array).expect("tags array");
    assert_eq!(tags.len(), 2, "the draft's tags are carried through verbatim");

    nmp_app_free(app);
    uninstall_frame_capture();
}

#[test]
fn sign_event_for_return_without_account_returns_error_verdict() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_frame_capture();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_frame_callback));
    nmp_app_start(app, 256, 4);

    // No sign-in: the active account is empty, so the sign must fail with an
    // observable error verdict (never a hang, never a crash — D6).
    let draft = r#"{"kind":1,"content":"x","tags":[]}"#;
    let empty = std::ffi::CString::new("").unwrap();
    let draft_c = std::ffi::CString::new(draft).unwrap();
    let cid_ptr =
        super::identity::nmp_app_sign_event_for_return(app, empty.as_ptr(), draft_c.as_ptr());
    let correlation_id = unsafe { std::ffi::CStr::from_ptr(cid_ptr) }
        .to_str()
        .unwrap()
        .to_string();
    super::free::nmp_free_string(cid_ptr);

    let entry = wait_for_signed_event(&rx, &correlation_id)
        .expect("the error verdict must surface in time");
    assert!(!entry.ok, "signing with no active account fails");
    assert!(
        entry.error.is_some(),
        "a human-readable error reason is carried for the host's continuation to throw"
    );

    nmp_app_free(app);
    uninstall_frame_capture();
}

#[test]
fn sign_by_explicit_pubkey_uses_named_signer() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_frame_capture();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_frame_callback));
    nmp_app_start(app, 256, 4);

    // Sign in with the test nsec (nmp_app_signin_nsec is the one registration
    // path — no separate "register without activating" FFI is needed).
    let nsec = std::ffi::CString::new(TEST_NSEC).unwrap();
    super::identity::nmp_app_signin_nsec(app, nsec.as_ptr(), 1);

    let pubkey = hex_pubkey(TEST_NSEC);
    let draft = r#"{"kind":24242,"content":"Upload image","tags":[["x","abc"]]}"#;
    let pubkey_c = std::ffi::CString::new(pubkey.clone()).unwrap();
    let draft_c = std::ffi::CString::new(draft).unwrap();
    let cid_ptr =
        super::identity::nmp_app_sign_event_for_return(app, pubkey_c.as_ptr(), draft_c.as_ptr());
    let correlation_id = unsafe { std::ffi::CStr::from_ptr(cid_ptr) }
        .to_str()
        .unwrap()
        .to_string();
    super::free::nmp_free_string(cid_ptr);

    let entry = wait_for_signed_event(&rx, &correlation_id)
        .expect("signing by explicit pubkey must produce a result");
    assert!(entry.ok, "sign_event_for_return must succeed when the named account is registered");
    let signed_json = entry.signed_json.as_deref().unwrap();
    let event: serde_json::Value = serde_json::from_str(signed_json).unwrap();
    assert_eq!(
        event.get("pubkey").and_then(serde_json::Value::as_str),
        Some(pubkey.as_str()),
        "the event is signed by the named (non-active) account"
    );

    nmp_app_free(app);
    uninstall_frame_capture();
}
