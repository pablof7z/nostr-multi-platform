//! Bug 1 (D6 fail-loud) — `NmpApp::remove_account_forgetting_keyring`.
//!
//! The function forgets the app-scoped keyring secret, then removes the
//! account. Pre-fix it discarded the keyring forget result
//! (`let _ = dispatch_capability(..)`) and removed the account unconditionally.
//! If the OS keychain forget FAILED, the nsec was left orphaned in the keychain
//! (a security/privacy residue) while the account row vanished — a silent
//! failure the host could never observe.
//!
//! These tests drive a REAL local-key sign-in through the actor thread, install
//! a capability handler whose keyring `delete` outcome we control, then call
//! `remove_account_forgetting_keyring` and assert:
//!   * the returned `KeyringStatus` reflects the keychain result (the result is
//!     now CHECKED — the old `()` signature could not carry it); and
//!   * on `Error` the account is KEPT (the active slot still holds the pubkey),
//!     while on `Ok` the account is removed (the slot clears).

use crate::{nmp_app_free, nmp_app_new, nmp_app_start, NmpApp};
use nmp_core::substrate::KeyringStatus;
use nostr::prelude::*;
use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Linearise: these tests share the process-global update + force-error slots.
static SERIAL: Mutex<()> = Mutex::new(());

/// When `true`, the keyring `delete` capability reports `Error`; otherwise `Ok`.
static FORCE_KEYRING_ERROR: AtomicBool = AtomicBool::new(false);

/// Park the update-tick `Sender` for the (capture-free) `extern "C"` callback.
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

/// Keyring capability handler: echoes the request's `namespace`/`correlation_id`
/// and reports a `delete` outcome driven by `FORCE_KEYRING_ERROR`. A real
/// keychain would return the same envelope shape; here we control the verdict.
extern "C" fn keyring_handler(_ctx: *mut c_void, req: *const c_char) -> *mut c_char {
    let request_json = unsafe { CStr::from_ptr(req) }.to_string_lossy().into_owned();
    let v: serde_json::Value = serde_json::from_str(&request_json).unwrap_or_default();
    let namespace = v["namespace"].as_str().unwrap_or("").to_string();
    let correlation_id = v["correlation_id"].as_str().unwrap_or("").to_string();
    let result = if FORCE_KEYRING_ERROR.load(Ordering::SeqCst) {
        nmp_core::substrate::KeyringResult::error(-25300)
    } else {
        nmp_core::substrate::KeyringResult::ok(None)
    };
    let envelope = nmp_core::substrate::CapabilityEnvelope {
        namespace,
        correlation_id,
        result_json: serde_json::to_string(&result).unwrap(),
    };
    CString::new(serde_json::to_string(&envelope).unwrap())
        .unwrap()
        .into_raw()
}

fn hex_pubkey(nsec: &str) -> String {
    let sk = SecretKey::parse(nsec).expect("valid nsec");
    Keys::new(sk).public_key().to_hex()
}

/// Block until the active-account slot satisfies `pred`, draining update ticks.
/// `Err(())` on timeout so a hung actor is never mistaken for a real `None`.
fn wait_for_slot<F>(
    rx: &std::sync::mpsc::Receiver<()>,
    slot: &nmp_core::slots::ActiveAccountSlot,
    pred: F,
) -> Result<Option<String>, ()>
where
    F: Fn(&Option<String>) -> bool,
{
    {
        let guard = slot.lock().expect("slot lock");
        if pred(&guard) {
            return Ok(guard.clone());
        }
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {
                let guard = slot.lock().expect("slot lock");
                if pred(&guard) {
                    return Ok(guard.clone());
                }
            }
            Err(_) => return Err(()),
        }
    }
}

/// Sign in `TEST_NSEC` and return (app, active-account handle, update rx).
fn signed_in_app() -> (
    *mut NmpApp,
    nmp_core::slots::ActiveAccountSlot,
    std::sync::mpsc::Receiver<()>,
) {
    let rx = install_update_signal();
    let app = nmp_app_new();
    crate::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    // Install the keyring capability handler on this app's slot.
    crate::nmp_app_set_capability_callback(app, std::ptr::null_mut(), Some(keyring_handler));
    let handle = crate::app_ref(app).expect("app").active_account_handle();

    nmp_app_start(app, 256, 4);
    let secret = CString::new(TEST_NSEC).unwrap();
    crate::nmp_app_signin_nsec(app, secret.as_ptr(), 1);

    let expected = hex_pubkey(TEST_NSEC);
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.as_deref() == Some(expected.as_str())),
        Ok(Some(expected)),
        "precondition: the test account is signed in and active"
    );
    (app, handle, rx)
}

#[test]
fn keyring_forget_error_keeps_account() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    FORCE_KEYRING_ERROR.store(true, Ordering::SeqCst);

    let (app, handle, _rx) = signed_in_app();
    let pubkey = hex_pubkey(TEST_NSEC);

    // The active account's identity_id is its pubkey hex.
    let status = crate::app_ref(app)
        .expect("app")
        .remove_account_forgetting_keyring(&pubkey, pubkey.clone());

    // The result is now CHECKED and surfaced (pre-fix this returned `()`).
    assert_eq!(
        status,
        KeyringStatus::Error,
        "a failed keychain forget must be reported, not swallowed"
    );

    // The account must NOT have been removed — the secret is still in the
    // keychain, so dropping the account row would orphan it. The active slot
    // must still hold the pubkey.
    assert_eq!(
        handle.lock().unwrap().as_deref(),
        Some(pubkey.as_str()),
        "account must be KEPT when the keychain forget failed (no orphaned nsec)"
    );

    nmp_app_free(app);
    uninstall_update_signal();
    FORCE_KEYRING_ERROR.store(false, Ordering::SeqCst);
}

#[test]
fn keyring_forget_ok_removes_account() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    FORCE_KEYRING_ERROR.store(false, Ordering::SeqCst);

    let (app, handle, rx) = signed_in_app();
    let pubkey = hex_pubkey(TEST_NSEC);

    let status = crate::app_ref(app)
        .expect("app")
        .remove_account_forgetting_keyring(&pubkey, pubkey.clone());

    assert_eq!(
        status,
        KeyringStatus::Ok,
        "a successful keychain forget reports Ok"
    );

    // The account WAS removed: the only account is gone, so the active slot
    // drains to None.
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.is_none()),
        Ok(None),
        "account must be removed once the keychain forget succeeded"
    );

    nmp_app_free(app);
    uninstall_update_signal();
}
