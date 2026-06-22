//! W1 acceptance test — the wasm runtime EXECUTES in a headless browser.
//!
//! This is the first test in the repository that actually *runs* the
//! `NmpWasmRuntime` inside a real browser (Chrome headless via wasm-pack).
//! It proves:
//!
//! 1. The `Instant::now()` / `SystemTime::now()` time panics (§1 of
//!    `web/chirp/docs/wasm-runtime-execution-plan.md`) are all fixed — the
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

// Imports used only by the wasm32-only typed-write honest-disable test
// (#1202 / #1007 guard). `DispatchBytes`/`SetIdentity` are wasm32-gated here too
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

    let running = start_events
        .iter()
        .any(|e| matches!(e, WorkerEvent::RuntimeStatus { status: RuntimeStatus::Running, .. }));
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

    let cr = decode_configured_relays(&cr_entry.payload)
        .expect("configured_relays payload must decode");

    assert!(
        cr.relays.iter().any(|r| r.url == "ws://127.0.0.1:1"),
        "configured_relays must contain the bootstrap relay URL 'ws://127.0.0.1:1'; got: {:?}",
        cr.relays
    );
}

/// #1202/#1007 regression guard — a typed wasm write MUST surface an explicit
/// `publish_not_supported_in_web_preview` `CapabilityFailure` instead of
/// silently swallowing the event.
///
/// Before #1202 the publish path called `publish_signed_event` on a kernel
/// whose `NoopOutboxResolver` resolved zero relay targets (`PublishTarget::Auto`
/// → `NoTargets`), then returned `ActionAccepted` — the host had no way to know
/// the event was never sent. This test asserts the honest-disable contract:
/// a write over the typed `DispatchBytes` doorway (the ONLY wasm write path
/// after #1743 Cut A) must resolve to a `CapabilityFailure` with the
/// `publish_not_supported_in_web_preview:` prefix while the real composition
/// root is pending (#1007).
///
/// The native variant of this guard is the
/// `publish_not_supported_in_web_preview_reason_has_stable_prefix` unit test in
/// `publish_path.rs`, plus the `typed_write_after_set_identity_*` test in
/// `protocol.rs`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn typed_write_surfaces_honest_disable_not_action_accepted() {
    // Seed the active identity so the gate cannot be attributed to a missing
    // account (we want to reach the honest-disable gate, not the
    // `signer_not_installed` gate). ADR-0064 §5: no persistent signer.
    let mut runtime = WasmRuntime::new();
    runtime
        .handle(WorkerRequest::SetIdentity(SetIdentity {
            kind: "nip07".to_string(),
            pubkey_hex: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"
                .to_string(),
            correlation_id: "set-1".to_string(),
        }))
        .expect("SetIdentity must succeed");

    // Drive a typed write through the one binary doorway — the path that would
    // hit NoopOutboxResolver → NoTargets → silent ActionAccepted (the #1202 bug)
    // once publishing is wired.
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
        WorkerEvent::CapabilityFailure(CapabilityFailure { capability, reason, .. }) => {
            assert_eq!(
                capability, "nmp.publish",
                "CapabilityFailure must carry the decoded namespace; got: {capability:?}"
            );
            assert!(
                reason.starts_with("publish_not_supported_in_web_preview:"),
                "typed write must surface 'publish_not_supported_in_web_preview:' prefix, \
                 NOT 'action_accepted' or any silent-success indicator; got: {reason:?}"
            );
        }
        WorkerEvent::ActionAccepted { .. } => {
            panic!(
                "#1202 regression: typed write returned ActionAccepted but the event \
                 was silently dropped (NoopOutboxResolver → NoTargets). The honest-disable \
                 gate must return CapabilityFailure before any publish_signed_event call."
            );
        }
        other => {
            panic!(
                "expected CapabilityFailure with publish_not_supported_in_web_preview prefix, \
                 got: {other:?}"
            );
        }
    }
}
