//! Crate-level integration: register `UploadAction` on a real FFI app, sign in
//! a local nsec, and dispatch `nmp.blossom.upload` through the generic
//! `nmp_app_dispatch_action` registry seam.
//!
//! This proves the action seam end-to-end up to the `Protocol(BlossomUploadCommand)`
//! emission (the dispatch is accepted and a `correlation_id` is minted) and that
//! `start()` validation rejects malformed input through the real registry. The
//! Build → Sign → Transport leg (streaming sha256, kind:24242 build, the
//! backend-transparent sign hop, BUD-02 PUT, and multi-server aggregation) is
//! pinned by the unit tests in `auth.rs`, `upload/http.rs`, `upload/mod.rs`, and
//! `nmp-core`'s `sign_event_for_account_tests.rs` — including a real SHA-256 over
//! a known blob and a local mock Blossom server. (An async-completing action's
//! `action_results` terminal lands on a later snapshot tick via the update
//! stream; pinning the synchronous descriptor/aggregation shape over a real
//! mock server in `upload/mod.rs` + `http.rs` is the non-flaky equivalent.)

use std::ffi::{CStr, CString};

use nmp_ffi::{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_new, nmp_app_signin_nsec, nmp_free_string,
    NmpApp,
};

/// Known-good test nsec.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

/// Serialize the FFI tests in this binary (they share process-wide actor state).
fn guard() -> std::sync::MutexGuard<'static, ()> {
    static G: std::sync::Mutex<()> = std::sync::Mutex::new(());
    G.lock().unwrap_or_else(|p| p.into_inner())
}

fn dispatch(app: *mut NmpApp, namespace: &str, body: &str) -> serde_json::Value {
    let ns = CString::new(namespace).unwrap();
    let b = CString::new(body).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), b.as_ptr());
    assert!(!ptr.is_null(), "dispatch_action never returns null");
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

fn signin(app: *mut NmpApp) {
    let nsec = CString::new(TEST_NSEC).unwrap();
    nmp_app_signin_nsec(app, nsec.as_ptr(), 1);
}

#[test]
fn dispatch_well_formed_blossom_upload_is_accepted_through_registry() {
    let _g = guard();
    let app = nmp_app_new();
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
    // dispatch is ACCEPTED (a correlation_id is minted) and the Protocol command
    // is emitted. If the off-thread worker wins the post-test file-delete race
    // it would otherwise fire a real PUT at a public host (60s timeout);
    // `http://127.0.0.1:1` makes any such PUT fail instantly instead.
    let body = serde_json::json!({
        "file_path": path.to_str().unwrap(),
        "content_type": "image/png",
        "servers": ["http://127.0.0.1:1"],
    })
    .to_string();

    let parsed = dispatch(app, "nmp.blossom.upload", &body);
    let cid = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
    assert!(
        !cid.is_empty(),
        "a well-formed dispatch mints a correlation_id"
    );
    assert!(
        parsed.get("error").is_none(),
        "a well-formed dispatch is not an error: {parsed}"
    );

    let _ = std::fs::remove_file(&path);
    nmp_app_free(app);
}

#[test]
fn dispatch_rejects_empty_servers_through_registry() {
    let _g = guard();
    let app = nmp_app_new();
    // SAFETY: see above.
    nmp_blossom::register_actions(unsafe { &mut *app });
    signin(app);

    let body = serde_json::json!({
        "file_path": "/tmp/whatever.png",
        "servers": [],
    })
    .to_string();
    let parsed = dispatch(app, "nmp.blossom.upload", &body);
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected error for empty servers, got {parsed}"));
    assert!(
        err.contains("server"),
        "start() rejection must reach the caller: {err}"
    );

    nmp_app_free(app);
}

#[test]
fn dispatch_rejects_empty_file_path_through_registry() {
    let _g = guard();
    let app = nmp_app_new();
    // SAFETY: see above.
    nmp_blossom::register_actions(unsafe { &mut *app });
    signin(app);

    let body = serde_json::json!({
        "file_path": "   ",
        "servers": ["https://blossom.example"],
    })
    .to_string();
    let parsed = dispatch(app, "nmp.blossom.upload", &body);
    let err = parsed
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected error for empty file_path, got {parsed}"));
    assert!(err.contains("file_path"), "rejection reason: {err}");

    nmp_app_free(app);
}
