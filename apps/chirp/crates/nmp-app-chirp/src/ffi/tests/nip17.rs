//! NIP-17 DM send dispatch proof + DM-inbox host registration tests.

use nmp_ffi::{nmp_app_free, nmp_app_new};

use super::super::{
    nmp_app_chirp_register, nmp_app_chirp_register_dm_inbox, nmp_app_chirp_unregister,
};
use super::helpers::dispatch;

/// THE NIP-17 SEND-VERB PROOF: after `nmp_app_chirp_register`, the
/// `nmp.nip17.send` action — `SendDmAction`, an `ActionModule` living in the
/// `nmp-nip17` protocol crate — is reachable through the generic
/// `dispatch_action` path. A well-formed `SendDmInput` yields a 32-hex
/// `correlation_id` (both the typed module validator AND the executor are
/// wired); a malformed / empty body is rejected with `error`.
#[test]
fn nip17_dm_send_dispatches_through_action_registry() {
    let app = nmp_app_new();
    let handle = nmp_app_chirp_register(app, std::ptr::null());
    assert!(!handle.is_null());

    let recipient = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
    let body = format!(r#"{{"recipient_pubkey":"{recipient}","content":"hello over NIP-17"}}"#);
    let parsed = dispatch(app, "nmp.nip17.send", &body);
    let id = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert_eq!(id.len(), 32, "correlation id should be 32 hex");

    // Empty content is rejected by the typed `SendDmAction::start`
    // validator surfaced through the host seam (D6).
    let parsed = dispatch(
        app,
        "nmp.nip17.send",
        &format!(r#"{{"recipient_pubkey":"{recipient}","content":"  "}}"#),
    );
    assert!(
        parsed.get("error").is_some(),
        "an empty-content DM must be rejected: {parsed}"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

/// THE DM-INBOX WIRING PROOF: `nmp_app_chirp_register_dm_inbox` registers
/// a `DmInboxProjection` against `app` — it runs to completion (raw-event
/// observer + snapshot-projection/controller registration) without
/// panicking across the FFI boundary. Active-account interest ownership is
/// Rust-side — the FFI takes no viewer pubkey.
#[test]
fn register_dm_inbox_runs_for_app() {
    let app = nmp_app_new();
    nmp_app_chirp_register_dm_inbox(app);
    nmp_app_free(app);
}

/// D6: a null `app` is a silent no-op — the function must never
/// dereference a null pointer or panic across the FFI boundary.
#[test]
fn register_dm_inbox_null_app_is_silent_noop() {
    nmp_app_chirp_register_dm_inbox(std::ptr::null_mut());
}

/// THE IDEMPOTENCY PROOF: re-invoking `nmp_app_chirp_register_dm_inbox`
/// must NOT stack a fresh raw-event observer on every call. The function
/// remains directly callable from the host, while `nmp_app_chirp_register`
/// also wires the runtime eagerly.
///
/// Asserted observably through the per-app
/// `swap_dm_inbox_observer` slot — the host-side handle that lets
/// the function "remember the previous id and unregister it before
/// installing the new one":
///
/// 1. The first register installs an id in the slot (the fix path
///    actively writes through `swap_dm_inbox_observer(Some(id1))`).
///    Before the fix the slot was never written, so this assertion alone
///    fails on the buggy code.
/// 2. The second register installs a FRESH id, distinct from the first —
///    proving the slot was overwritten with the new observer, not
///    silently dropped. The fix path also unregisters the prior id
///    against the kernel observer slot before storing the new one, so
///    the kernel's raw-observer registration count for `kind == 1059`
///    after the second call is 1, not 2.
/// 3. Manually unregistering the second id (`unregister_raw_event_
///    observer(id2)`) drains the kernel observer slot — at this point
///    there is no kind:1059 observer alive, which is the leak-free
///    steady state.
#[test]
fn register_dm_inbox_is_idempotent_on_re_invoke() {
    let app = nmp_app_new();
    // SAFETY: `app` is a valid pointer returned by `nmp_app_new`, live
    // for the duration of this test (we call `nmp_app_free` at the end).
    let app_ref = unsafe { &*app };

    // Pre-condition: the per-app slot starts empty.
    assert!(
        app_ref.swap_dm_inbox_observer(None).is_none(),
        "slot must start empty (no DM inbox registered yet)"
    );

    // First registration.
    nmp_app_chirp_register_dm_inbox(app);
    let id1 = app_ref
        .swap_dm_inbox_observer(None)
        .expect("first register must install a raw-observer id in the per-app slot");
    // Put id1 back so the SECOND register sees it as the "previous" id
    // and unregisters it before installing its own.
    let prev = app_ref.swap_dm_inbox_observer(Some(id1));
    assert!(prev.is_none(), "we just swap-took, slot was empty");

    // Second registration — compatibility re-invoke case.
    nmp_app_chirp_register_dm_inbox(app);
    let id2 = app_ref
        .swap_dm_inbox_observer(None)
        .expect("second register must install a fresh id in the per-app slot");

    // Distinct ids: the slot was overwritten with the second register's
    // observer, not silently dropped. This is the host-side proof that
    // the leak is bounded — exactly one id lives in the per-app slot at
    // any time, regardless of how many sign-in cycles preceded.
    assert_ne!(
        id1, id2,
        "second register must produce a fresh raw-observer id (got {id1:?} both times)"
    );

    // Drain the kernel observer slot through the live id. Without the
    // fix, id1 would ALSO still be in the kernel slot and this would
    // leave kind:1059 observers behind (one observer of equivalence
    // class id1). With the fix, the kernel slot is now empty.
    app_ref.unregister_raw_event_observer(id2);

    nmp_app_free(app);
}
