//! Crate-level integration: register `UploadAction` on a real FFI app, sign in
//! a local nsec, and dispatch `nmp.blossom.upload` through the typed byte
//! doorway `nmp_app_dispatch_action_bytes` (ADR-0071 / Cut-B, #1756).
//!
//! This proves the action seam end-to-end up to the `Protocol(BlossomUploadCommand)`
//! emission (the dispatch is accepted and the host-supplied `correlation_id` is
//! echoed back) and that `start()` validation rejects malformed input through the
//! real registry. The Build → Sign → Transport leg (streaming sha256, kind:24242
//! build, the backend-transparent sign hop, BUD-02 PUT, and multi-server
//! aggregation) is pinned by the unit tests in `auth.rs`, `upload/http.rs`,
//! `upload/mod.rs`, and `nmp-core`'s `sign_event_for_account_tests.rs` —
//! including a real SHA-256 over a known blob and a local mock Blossom server.
//! (An async-completing action's `action_results` terminal lands on a later
//! snapshot tick via the update stream; pinning the synchronous
//! descriptor/aggregation shape over a real mock server in `upload/mod.rs` +
//! `http.rs` is the non-flaky equivalent.)

use std::sync::atomic::{AtomicU64, Ordering};

use nmp_blossom::UploadInput;
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload as _;
use nmp_native_runtime::NmpApp;

/// Known-good test nsec.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Serialize the FFI tests in this binary (they share process-wide actor state).
fn guard() -> std::sync::MutexGuard<'static, ()> {
    static G: std::sync::Mutex<()> = std::sync::Mutex::new(());
    G.lock().unwrap_or_else(|p| p.into_inner())
}

/// Mint a unique, process-local host correlation_id. The byte doorway echoes
/// this back verbatim (it is NOT a kernel-minted 32-hex id).
fn next_correlation_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("blossom-test-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// Encode `input` into a typed `nmp.blossom.upload` dispatch envelope, push it
/// through the byte doorway, free the returned C string, and return the parsed
/// `{"correlation_id":...}` / `{"error":...}` JSON.
fn dispatch(app: *mut NmpApp, input: &UploadInput) -> serde_json::Value {
    let correlation_id = next_correlation_id();
    let payload = input.encode();
    let envelope = encode_dispatch_envelope(
        &correlation_id,
        "nmp.blossom.upload",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &payload,
    );
    // SAFETY: app is a valid, non-null pointer.
    let outcome = nmp_native_runtime::dispatch_action_bytes_typed(unsafe { &*app }, &envelope);
    serde_json::json!({ "correlation_id": outcome.correlation_id, "error": outcome.error })
}

fn signin(app: *mut NmpApp) {
    unsafe { &*app }.signin_nsec_for_test(TEST_NSEC, true);
}

#[test]
fn dispatch_well_formed_blossom_upload_is_accepted_through_registry() {
    let _g = guard();
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    // SAFETY: `nmp_app_new` never returns null; the pointer is valid until
    // `nmp_app_free` and no aliasing `&NmpApp` is live during registration.
    nmp_blossom::register_actions(unsafe { &mut *app });
    signin(app);

    // Write a real blob the action can hash (file_path must point at a real
    // file for the worker, though the worker runs off-thread; dispatch itself
    // only validates + emits the Protocol command).
    let dir = std::env::temp_dir();
    let path = dir.join(format!("nmp-blossom-it-{}.png", std::process::id()));
    std::fs::write(&path, b"\x89PNG\r\n\x1a\n fake png bytes").unwrap();

    // Point at an unroutable local address: this test asserts only that the
    // dispatch is ACCEPTED (a correlation_id is echoed) and the Protocol command
    // is emitted. If the off-thread worker wins the post-test file-delete race
    // it would otherwise fire a real PUT at a public host (60s timeout);
    // `http://127.0.0.1:1` makes any such PUT fail instantly instead.
    let input = UploadInput {
        file_path: path.to_str().unwrap().to_string(),
        content_type: Some("image/png".to_string()),
        servers: vec!["http://127.0.0.1:1".to_string()],
        signer_pubkey: None,
    };

    let parsed = dispatch(app, &input);
    let cid = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert!(
        !cid.is_empty(),
        "a well-formed dispatch echoes a correlation_id"
    );
    assert!(
        parsed.get("error").is_none_or(serde_json::Value::is_null),
        "a well-formed dispatch is not an error: {parsed}"
    );

    let _ = std::fs::remove_file(&path);
    unsafe { drop(Box::from_raw(app)) };
}

#[test]
fn dispatch_rejects_empty_servers_through_registry() {
    let _g = guard();
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    // SAFETY: see above.
    nmp_blossom::register_actions(unsafe { &mut *app });
    signin(app);

    let input = UploadInput {
        file_path: "/tmp/whatever.png".to_string(),
        content_type: None,
        servers: vec![],
        signer_pubkey: None,
    };
    let parsed = dispatch(app, &input);
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected error for empty servers, got {parsed}"));
    assert!(
        err.contains("server"),
        "start() rejection must reach the caller: {err}"
    );

    unsafe { drop(Box::from_raw(app)) };
}

#[test]
fn dispatch_rejects_empty_file_path_through_registry() {
    let _g = guard();
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    // SAFETY: see above.
    nmp_blossom::register_actions(unsafe { &mut *app });
    signin(app);

    let input = UploadInput {
        file_path: "   ".to_string(),
        content_type: None,
        servers: vec!["https://blossom.example".to_string()],
        signer_pubkey: None,
    };
    let parsed = dispatch(app, &input);
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected error for empty file_path, got {parsed}"));
    assert!(err.contains("file_path"), "rejection reason: {err}");

    unsafe { drop(Box::from_raw(app)) };
}
