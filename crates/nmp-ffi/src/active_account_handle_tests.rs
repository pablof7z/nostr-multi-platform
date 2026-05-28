//! V-82 — `NmpApp::active_account_handle()` exposes the kernel's authoritative
//! active-account slot as a single shared source of truth.
//!
//! These tests drive a REAL sign-in (and account-switch / logout / Reset)
//! through the actor thread — not a direct `slot.lock()` poke — so they prove
//! the slot the host reads is the very `Arc` the kernel writes on every
//! identity mutation (`Kernel::set_accounts`). A test that merely set the slot
//! then read it back would not rule out two divergent slots both happening to
//! hold the right value; the `Arc::as_ptr` identity check below does.

use super::*;
use crate::{nmp_app_free, nmp_app_new, nmp_app_start};
use nostr::prelude::*;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// Reference key the in-tree identity tests use (`actor::commands::tests`).
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// A second, distinct nsec (bech32) for the switch test, derived
/// deterministically from a fixed 32-byte secret so we never hard-code a
/// possibly-malformed literal.
fn second_nsec() -> String {
    let sk = SecretKey::from_slice(&[2u8; 32]).expect("valid 32-byte secret");
    Keys::new(sk)
        .secret_key()
        .to_bech32()
        .expect("nsec bech32")
}

/// Linearise the tests: they share a process-global update-signal channel.
static SERIAL: Mutex<()> = Mutex::new(());
/// `extern "C"` callbacks cannot capture, so park the signal `Sender` in a
/// static and forward a tick through it on every update frame the actor emits.
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

/// Install the update recorder; returns the receiver of update ticks.
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

/// Compute the raw hex pubkey an nsec resolves to — the exact value the actor
/// stores in the slot (`keys.public_key().to_hex()`), derived independently so
/// the assertion is self-checking rather than a hard-coded constant.
fn hex_pubkey(nsec: &str) -> String {
    let sk = SecretKey::parse(nsec).expect("valid nsec");
    Keys::new(sk).public_key().to_hex()
}

/// Block until the slot's locked value satisfies `pred`, draining update ticks
/// emitted by the actor. The actor sets `changed_since_emit` on every identity
/// mutation, so a sign-in / switch / logout produces at least one update frame;
/// we re-check the slot on each tick. A generous timeout guards against a hung
/// actor without polling-sleep loops in the steady state.
///
/// Returns `Ok(value)` when `pred` is satisfied (the matching slot value), or
/// `Err(())` on timeout (the actor never produced a state the predicate
/// accepts). The `Result` is DELIBERATE: a plain `Option` would make a timeout
/// indistinguishable from a legitimate `None` match (e.g. logout), turning a
/// hung actor into a silent false-positive in `…_is_none()` assertions.
fn wait_for_slot<F>(
    rx: &std::sync::mpsc::Receiver<()>,
    slot: &nmp_core::slots::ActiveAccountSlot,
    pred: F,
) -> Result<Option<String>, ()>
where
    F: Fn(&Option<String>) -> bool,
{
    // Initial check (the mutation may already have landed before we wait).
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

#[test]
fn active_account_handle_reflects_real_sign_in() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));

    // The handle exists BEFORE the kernel is built (the actor only constructs
    // the kernel on the first command). It is the host's read end of the slot.
    let handle = {
        let app_ref = super::app_ref(app).expect("app");
        app_ref.active_account_handle()
    };
    // Capture the slot's `Arc` identity: a single source of truth means the
    // kernel writes through THIS pointer, not a divergent mirror.
    let slot_ptr = Arc::as_ptr(&handle);

    // Cold start: no account signed in.
    assert!(
        handle.lock().unwrap().is_none(),
        "no account active before sign-in"
    );

    nmp_app_start(app, 0, 256, 4);
    let secret = std::ffi::CString::new(TEST_NSEC).unwrap();
    super::nmp_app_signin_nsec(app, secret.as_ptr());

    let expected = hex_pubkey(TEST_NSEC);
    let observed = wait_for_slot(&rx, &handle, |v| v.as_deref() == Some(expected.as_str()));
    assert_eq!(
        observed,
        Ok(Some(expected.clone())),
        "the slot the host reads must reflect the real kernel sign-in"
    );

    // Arc-identity proof: the host's handle and the value the kernel mutated
    // are the SAME slot (the mutation we observed landed at `slot_ptr`).
    let handle_again = {
        let app_ref = super::app_ref(app).expect("app");
        app_ref.active_account_handle()
    };
    assert_eq!(
        Arc::as_ptr(&handle_again),
        slot_ptr,
        "every accessor call returns a clone of the SAME Arc (single source of truth)"
    );

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn active_account_handle_reflects_account_switch() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let handle = super::app_ref(app).expect("app").active_account_handle();

    nmp_app_start(app, 0, 256, 4);

    // Sign in account A.
    let nsec_a = std::ffi::CString::new(TEST_NSEC).unwrap();
    super::nmp_app_signin_nsec(app, nsec_a.as_ptr());
    let pk_a = hex_pubkey(TEST_NSEC);
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.as_deref() == Some(pk_a.as_str())),
        Ok(Some(pk_a.clone()))
    );

    // Sign in account B — the active slot must now reflect B (account switch:
    // signing in a new local key makes it the active account).
    let nsec_b_str = second_nsec();
    let nsec_b = std::ffi::CString::new(nsec_b_str.clone()).unwrap();
    super::nmp_app_signin_nsec(app, nsec_b.as_ptr());
    let pk_b = hex_pubkey(&nsec_b_str);
    assert_ne!(pk_a, pk_b, "the two test keys must differ");
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.as_deref() == Some(pk_b.as_str())),
        Ok(Some(pk_b)),
        "the slot must reflect the new active account after a switch"
    );

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn active_account_handle_survives_reset() {
    // The Reset trap: a bare `Kernel::new` on Reset would mint a fresh slot
    // and orphan the host's handle. This test proves the SAME handle still
    // reflects a post-Reset sign-in.
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let app = nmp_app_new();
    super::nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let handle = super::app_ref(app).expect("app").active_account_handle();
    let slot_ptr = Arc::as_ptr(&handle);

    nmp_app_start(app, 0, 256, 4);

    // Sign in account A, then Reset (wipes all kernel state, including the
    // active account → slot returns to `None`).
    let nsec_a = std::ffi::CString::new(TEST_NSEC).unwrap();
    super::nmp_app_signin_nsec(app, nsec_a.as_ptr());
    let pk_a = hex_pubkey(TEST_NSEC);
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.as_deref() == Some(pk_a.as_str())),
        Ok(Some(pk_a.clone()))
    );

    super::nmp_app_reset(app);
    // After Reset the kernel is rebuilt and no account is active. The `Result`
    // distinguishes a genuine `None` transition from a hung-actor timeout — a
    // plain `Option` return would make this assertion pass on timeout too.
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.is_none()),
        Ok(None),
        "Reset clears the active account in the shared slot"
    );

    // Sign in a DIFFERENT account (B) after Reset — the SAME handle must
    // reflect B, proving the rebuilt kernel writes to the host's slot, not a
    // fresh orphan. A different key means a stale pre-Reset value (A) would
    // fail this assertion, so it cannot pass trivially on a retained value.
    let nsec_b_str = second_nsec();
    let nsec_b = std::ffi::CString::new(nsec_b_str.clone()).unwrap();
    super::nmp_app_signin_nsec(app, nsec_b.as_ptr());
    let pk_b = hex_pubkey(&nsec_b_str);
    assert_ne!(pk_a, pk_b, "the two test keys must differ");
    assert_eq!(
        wait_for_slot(&rx, &handle, |v| v.as_deref() == Some(pk_b.as_str())),
        Ok(Some(pk_b)),
        "post-Reset sign-in must land in the SAME host-held slot"
    );
    assert_eq!(
        Arc::as_ptr(&super::app_ref(app).expect("app").active_account_handle()),
        slot_ptr,
        "the host's slot Arc is stable across Reset"
    );

    nmp_app_free(app);
    uninstall_update_signal();
}
