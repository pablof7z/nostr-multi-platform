//! W1 acceptance test — the wasm runtime EXECUTES in a headless browser.
//!
//! This is the first test in the repository that actually *runs* the
//! `NmpWasmRuntime` inside a real browser (Chrome headless via wasm-pack).
//! It proves:
//!
//! 1. The `Instant::now()` / `SystemTime::now()` time concerns are addressed — the
//!    runtime boots without aborting on `wasm32-unknown-unknown`.
//! 2. `Kernel::start()` completes and returns `RuntimeStatus::Running`.
//! 3. At least one `UpdateBytes` frame (a real FlatBuffers snapshot) is
//!    emitted — the PR-1 kernel-authored snapshot pipeline fired.
//!
//! The relay URL (`ws://127.0.0.1:1`) is non-routable; the driver's
//! `onerror`/reconnect path will run, but that is expected and does NOT
//! affect the assertions (which only look at the synchronous Start response).
//!
//! ## Running locally
//!
//! ```bash
//! CC_wasm32_unknown_unknown=clang \
//!   wasm-pack test --headless --chrome crates/nmp-wasm
//! ```
//!
//! ## CI
//!
//! The `chirp-web` GitHub Actions job runs this test with `wasm-pack test
//! --headless --chrome` (ubuntu-latest ships Chrome + chromedriver).
//! See `.github/workflows/chirp-web.yml`.

use wasm_bindgen_test::*;

// `run_in_browser` is required for `Instant` (performance.now) and
// `WebSocket` (Driver teardown path) to be available. Worker context is
// proved by PR-W3's Playwright harness; page context is sufficient here.
wasm_bindgen_test_configure!(run_in_browser);

use nmp_core::{
    decode_snapshot_envelope, decode_snapshot_typed_projections,
    typed_projections::{decode_configured_relays, CONFIGURED_RELAYS_SCHEMA_ID},
};
use nmp_wasm::{
    ClientHello, RelayBootstrapEntry, RuntimeStatus, StartConfig, WasmRuntime, WorkerEvent,
    WorkerRequest,
};

// Imports used only by the wasm32-only typed-write routing guard
// (#1202 / #1008 guard). `DispatchBytes`/`SetIdentity` are wasm32-gated here too
// so native builds (where that test is `cfg`-compiled out) carry no unused
// imports.
#[cfg(target_arch = "wasm32")]
use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
#[cfg(target_arch = "wasm32")]
use nmp_wasm::{CapabilityFailure, DispatchBytes, SetIdentity};

/// Boot the runtime through Hello → Start and assert:
/// - `HelloAccepted` is returned for a matching protocol version.
/// - `RuntimeStatus::Running` is returned after Start.
/// - At least one `UpdateBytes` snapshot frame is returned after Start,
///   proving the PR-1 kernel-authored snapshot pipeline fired.
#[wasm_bindgen_test]
fn wasm_runtime_boots_without_panicking() {
    let mut runtime = WasmRuntime::new();

    // ── Step 1: Hello ────────────────────────────────────────────────────────
    let hello_events = runtime
        .handle(WorkerRequest::Hello(ClientHello {
            app_id: "chirp".to_string(),
            platform: "web".to_string(),
            protocol_version: 1,
        }))
        .expect("Hello must not error");

    assert!(
        hello_events
            .iter()
            .any(|e| matches!(e, WorkerEvent::HelloAccepted { .. })),
        "Hello(version=1) must return HelloAccepted; got: {hello_events:?}"
    );

    // ── Step 2: Start ────────────────────────────────────────────────────────
    // Non-routable relay: the driver's onerror/reconnect path fires on wasm32
    // but that is async and does NOT affect the synchronous Start response.
    let start_events = runtime
        .handle(WorkerRequest::Start(StartConfig {
            app_id: "chirp".to_string(),
            relays: vec!["ws://127.0.0.1:1".to_string()],
            relay_bootstrap: vec![RelayBootstrapEntry {
                url: "ws://127.0.0.1:1".to_string(),
                role: "both".to_string(),
            }],
            database_name: "wasm-boot-test".to_string(),
            correlation_id: "w1-boot".to_string(),
        }))
        .expect("Start must not error");

    let running = start_events.iter().any(|e| {
        matches!(
            e,
            WorkerEvent::RuntimeStatus {
                status: RuntimeStatus::Running,
                ..
            }
        )
    });
    assert!(
        running,
        "Start must return RuntimeStatus::Running; got: {start_events:?}"
    );

    // ── Step 3: Decode the UpdateBytes frame ────────────────────────────────
    // Find the first UpdateBytes event and extract its bytes.
    let update_bytes = start_events
        .iter()
        .find_map(|e| {
            if let WorkerEvent::UpdateBytes { bytes } = e {
                Some(bytes.clone())
            } else {
                None
            }
        })
        .expect("Start must emit at least one UpdateBytes snapshot frame; got: {start_events:?}");

    // Decode the envelope: the shim fix (PR-W1) makes Instant available on
    // wasm32 so `running` should be set to true.
    let env = decode_snapshot_envelope(&update_bytes)
        .expect("UpdateBytes must decode as a valid SnapshotEnvelope");
    assert!(
        env.running,
        "SnapshotEnvelope.running must be true after a successful Start"
    );

    // Decode typed projections: the configured_relays sidecar must contain
    // the bootstrap URL that was passed in StartConfig.
    let projections = decode_snapshot_typed_projections(&update_bytes)
        .expect("UpdateBytes must decode its typed projections");

    let cr_entry = projections
        .iter()
        .find(|p| p.schema_id == CONFIGURED_RELAYS_SCHEMA_ID)
        .expect("configured_relays typed projection must be present in the snapshot");

    let cr =
        decode_configured_relays(&cr_entry.payload).expect("configured_relays payload must decode");

    assert!(
        cr.relays.iter().any(|r| r.url == "ws://127.0.0.1:1"),
        "configured_relays must contain the bootstrap relay URL 'ws://127.0.0.1:1'; got: {:?}",
        cr.relays
    );
}

/// #1008 routing guard — a typed wasm write MUST enter the typed decode path
/// (NOT silently swallow the event, and NOT emit the old pre-#1008
/// `publish_not_supported_in_web_preview` disable token).
///
/// Before #1008 the wasm publish path was entirely disabled: every
/// `nmp.publish` write over `DispatchBytes` was intercepted before reaching
/// the `PublishModule` and returned a `CapabilityFailure` with the
/// `publish_not_supported_in_web_preview:` token. That hard-disable is now
/// REMOVED (#1008). The `PublishModule` is live in the default action registry;
/// app composition is responsible for installing the shared publish resolver.
///
/// This test sends a structurally invalid publish payload (opaque bytes that
/// will fail FlatBuffers decode) to prove:
/// 1. The action REACHES the typed dispatch router (i.e. `publish_not_supported_in_web_preview`
///    is gone — no early-exit before the module runs).
/// 2. The response is a `CapabilityFailure` from the DECODE stage, not a
///    silent `ActionAccepted` (no #1202 regression where NoopOutboxResolver
///    dropped events silently).
///
/// The native variant of this guard is `signer_not_installed_reason_is_stable`
/// in `dispatch_routing_tests.rs` (which asserts the legacy token is absent).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn typed_write_routes_through_publish_module_not_legacy_disable() {
    // Seed the active identity so the gate cannot be attributed to a missing
    // account (we want to reach the typed-decode gate, not `signer_not_installed`).
    // ADR-0064 §5: no persistent signer.
    let mut runtime = WasmRuntime::new();
    runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                .to_string(),
            correlation_id: "set-1".to_string(),
            identity_relays: Vec::new(),
        }))
        .expect("SetIdentity must succeed");

    // Drive a typed write through the one binary doorway with an intentionally
    // invalid payload (opaque bytes). After #1008 this reaches `PublishModule`
    // which rejects the malformed FlatBuffers payload with a decode error —
    // proving the old early-exit disable path is gone.
    let bytes = encode_dispatch_envelope(
        "pub-wasm-1",
        "nmp.publish",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        b"opaque-publish-payload",
    );
    let events = runtime
        .handle(WorkerRequest::DispatchBytes(DispatchBytes { bytes }))
        .expect("DispatchBytes must not error");

    match &events[0] {
        WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability, reason, ..
        }) => {
            assert_eq!(
                capability, "nmp.publish",
                "CapabilityFailure must carry the decoded namespace; got: {capability:?}"
            );
            // After #1008 the old disable token is GONE. The failure must come
            // from the typed decode stage (malformed payload), not the pre-#1008
            // early-exit guard.
            assert!(
                !reason.starts_with("publish_not_supported_in_web_preview"),
                "#1008 regression: old 'publish_not_supported_in_web_preview' disable token \
                 must be gone — publish routing is now active; got: {reason:?}"
            );
            assert!(
                !reason.starts_with("publish_path_not_wired"),
                "#1008 regression: stale 'publish_path_not_wired' disable token must be gone; \
                 got: {reason:?}"
            );
            // The action reached the typed decode path: reason comes from the
            // module's decode/validation stage (e.g. malformed FlatBuffers).
            // We don't assert the exact string — that is an implementation
            // detail of PublishModule — but it must not be a silent accept.
        }
        WorkerEvent::ActionAccepted { .. } => {
            panic!(
                "#1202 regression: typed write with malformed payload returned ActionAccepted. \
                 A decode failure must surface as CapabilityFailure, never silent-accept."
            );
        }
        other => {
            panic!("expected CapabilityFailure from typed decode stage, got: {other:?}");
        }
    }
}
