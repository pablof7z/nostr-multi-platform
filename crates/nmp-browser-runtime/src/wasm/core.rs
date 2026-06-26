// Items in this module are primary consumers for the wasm32 target;
// on native they are exercised only from `#[cfg(test)]` blocks.
// `type_complexity` fires on the `SnapshotSink` type alias use-site even after
// aliasing — suppressed here since the type is inherently compositional.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#![allow(clippy::type_complexity)]

//! `NmpRuntimeCore` — non-wasm-gated request-handler for the wasm entry point.
//!
//! Contains the drive-loop and lifecycle/snapshot plumbing for `NmpWasmRuntime`
//! without the `#[wasm_bindgen]` annotations, `JsValue`, or `js_sys::Function`
//! dependencies. This lets native CI exercise the routing logic via
//! `cargo test -p nmp-browser-runtime`, while the thin `#[wasm_bindgen]`
//! wrappers in `super::mod.rs` provide the JS-facing surface.
//!
//! The per-`WorkerRequest` dispatch handlers (the nine variants) live in the
//! sibling `super::dispatch` module to keep both files under the 500-LOC
//! ceiling (AGENTS.md); they are declared as `impl NmpRuntimeCore` methods
//! there and reach the same `pub(super)` `handle` field.
//!
//! # Lifecycle
//!
//! - Constructed unstarted: `NmpRuntimeCore::new()`.
//! - A `WorkerRequest::Start` triggers `BrowserAppBuilder` composition and
//!   populates `self.handle`. Before `Start`, only `Hello` is accepted.
//! - After `Start`, request dispatch goes through the `BrowserRuntimeHandle`
//!   kernel-op helpers (see `crate::runtime::kernel_ops`).
//!
//! # Snapshot delivery
//!
//! After every mutable request (Start, SetIdentity, DispatchBytes, …) and after
//! every relay-inbound-driven pump, the caller should invoke
//! `push_snapshot_bytes_if_sink` to push the merged snapshot frame to the JS
//! host via the installed callback. The snapshot sink is `Box<dyn Fn(&[u8])>`
//! here (host-callback-agnostic); the wasm layer installs a closure that calls
//! `js_sys::Function::call1`.
//!
//! # Async pump event buffering (#2139 BLOCKER 2)
//!
//! `pump_once()` returns sign terminal events. When the async wake timer calls
//! `pump_and_push_snapshot` the wasm layer buffers those events in
//! `pending_host_events`. On the next `handle_json_request` call those events
//! are drained and prepended to the response so the main-thread broker can
//! resolve pending sign promises.

use crate::BrowserRuntimeHandle;

use super::protocol::{serialize_events, serialize_one, WorkerEvent, WorkerRequest};

// ── NmpRuntimeCore ────────────────────────────────────────────────────────────

/// Non-wasm-gated runtime core owned by `NmpWasmRuntime`.
///
/// Holds the `BrowserRuntimeHandle` (populated on `Start`) and the optional
/// snapshot callback sink. All protocol routing lives here (and in the sibling
/// `super::dispatch` module) so native CI can compile and test the logic.
pub struct NmpRuntimeCore {
    /// The live runtime handle; `None` until a `Start` request is processed.
    /// `pub(super)` so the dispatch handlers in `super::dispatch` reach it.
    pub(super) handle: Option<BrowserRuntimeHandle>,
    /// Sink invoked with merged snapshot bytes after mutable operations.
    /// Installed by the wasm layer via `set_snapshot_sink`.
    snapshot_sink: Option<Box<dyn Fn(&[u8])>>,
    /// Events buffered from async pump turns (relay-driven wakes) that have not
    /// yet been delivered to the JS host. Drained and prepended to the next
    /// `handle_json_request` response (#2139 BLOCKER 2).
    pub(super) pending_host_events: Vec<WorkerEvent>,
}

impl NmpRuntimeCore {
    /// Construct an unstarted core.
    pub fn new() -> Self {
        Self {
            handle: None,
            snapshot_sink: None,
            pending_host_events: Vec::new(),
        }
    }

    /// Install (or clear, with `None`) the snapshot push sink.
    ///
    /// The sink is called with raw merged FlatBuffers update-frame bytes.
    /// The wasm layer wraps a `js_sys::Function` call in this closure.
    pub fn set_snapshot_sink(&mut self, sink: Option<Box<dyn Fn(&[u8])>>) {
        self.snapshot_sink = sink;
    }

    // ── Public interface (called by the wasm thin wrappers) ───────────────────

    /// Handle a JSON-serialized `WorkerRequest` and return a JSON array of
    /// `WorkerEvent`s.
    ///
    /// Prepends any buffered events from async pump turns (relay-driven wakes)
    /// so sign terminals queued while waiting for the next JS call are delivered
    /// (#2139 BLOCKER 2 — async pump events were previously discarded).
    ///
    /// This is the main doorway: start / config / identity / resolve_ref /
    /// release_ref / begin_sign / deliver_signer_response / diagnostics.
    pub fn handle_json_request(&mut self, request_json: &str) -> String {
        let request = match serde_json::from_str::<WorkerRequest>(request_json) {
            Ok(r) => r,
            Err(err) => {
                return serialize_one(WorkerEvent::Error {
                    code: "parse_error".to_string(),
                    message: format!("WorkerRequest deserialize failed: {err}"),
                    correlation_id: None,
                });
            }
        };
        // Drain events buffered from async pump turns and prepend to response.
        let mut events = std::mem::take(&mut self.pending_host_events);
        events.extend(self.dispatch_request(request));
        serialize_events(&events)
    }

    /// Handle raw binary `DispatchEnvelope` bytes (ADR-0064 binary write
    /// doorway). Bypasses JSON serialization of the bytes (which would corrupt
    /// them to `{}`).
    pub fn handle_dispatch_bytes_raw(&mut self, bytes: &[u8]) -> String {
        let mut events = std::mem::take(&mut self.pending_host_events);
        events.extend(self.dispatch_dispatch_bytes(bytes));
        serialize_events(&events)
    }

    /// JSON snapshot of the kernel's recent routing decisions (pull-only).
    pub fn recent_routing_decisions(&self) -> String {
        match &self.handle {
            Some(h) => h.recent_routing_decisions_json(),
            None => r#"{"error":"not_started"}"#.to_string(),
        }
    }

    /// Push the current merged snapshot to the installed sink, if any.
    ///
    /// Called after each mutable operation and after relay-driven async pumps.
    pub fn push_snapshot_bytes_if_sink(&mut self) {
        if self.snapshot_sink.is_none() {
            return;
        }
        if let Some(h) = self.handle.as_mut() {
            if let Some(bytes) = h.produce_snapshot_bytes(true) {
                if let Some(sink) = &self.snapshot_sink {
                    sink(&bytes);
                }
            }
        }
    }

    /// Drive one pump turn. Returns the pump events mapped to `WorkerEvent`s.
    /// Call this after every request that may have produced inbox activity
    /// (notably after `deliver_signer_response` to capture sign terminals).
    pub fn pump_once(&mut self) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return Vec::new();
        };
        let outcome = handle.pump();
        map_pump_events(outcome.events)
    }

    /// Buffer host events from an async pump turn for delivery on the next
    /// `handle_json_request` call.
    ///
    /// Called by the wasm layer's `pump_and_push_snapshot` so sign terminal
    /// events produced by relay-driven pump turns are not silently dropped
    /// (#2139 BLOCKER 2).
    pub(super) fn buffer_host_events(&mut self, events: Vec<WorkerEvent>) {
        self.pending_host_events.extend(events);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Map `BrowserRuntimeEvent`s from a pump turn to `WorkerEvent`s.
pub(super) fn map_pump_events(events: Vec<crate::BrowserRuntimeEvent>) -> Vec<WorkerEvent> {
    use crate::BrowserRuntimeEvent;
    events
        .into_iter()
        .filter_map(|ev| match ev {
            BrowserRuntimeEvent::SignRequest {
                correlation_id,
                account_pubkey,
                unsigned_json,
            } => Some(WorkerEvent::SignRequest {
                correlation_id,
                account_pubkey,
                unsigned_json,
            }),
            // Map SignCompleted → WorkerEvent::SignCompleted (#2139 BLOCKER 2).
            BrowserRuntimeEvent::SignCompleted {
                correlation_id,
                signed_json,
            } => Some(WorkerEvent::SignCompleted {
                correlation_id,
                signed_json,
            }),
            BrowserRuntimeEvent::SignFailed {
                correlation_id,
                reason,
            } => Some(WorkerEvent::SignFailed {
                correlation_id,
                reason,
            }),
            BrowserRuntimeEvent::CommandFailed { reason } => Some(WorkerEvent::Error {
                code: "command_failed".to_string(),
                message: reason,
                correlation_id: None,
            }),
            // Relay events are diagnostic; drop from the protocol event stream.
            BrowserRuntimeEvent::RelayBudgetExceeded { .. }
            | BrowserRuntimeEvent::RelaySpawnFailed { .. }
            | BrowserRuntimeEvent::RelaySendFailed { .. }
            | BrowserRuntimeEvent::RelayInboundDropped { .. }
            | BrowserRuntimeEvent::SnapshotDecodeFailed { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    use super::*;

    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    fn start_req() -> String {
        serde_json::json!({
            "type": "start",
            "app_id": "chirp",
            "relays": [],
            "relay_bootstrap": [],
            "database_name": "chirp-test",
            "correlation_id": "start-1"
        })
        .to_string()
    }

    fn set_identity_req() -> String {
        serde_json::json!({
            "type": "set_identity",
            "kind": "nip07",
            "pubkey_hex": PK,
            "correlation_id": "id-1"
        })
        .to_string()
    }

    #[test]
    fn new_core_has_no_handle() {
        let core = NmpRuntimeCore::new();
        assert!(core.handle.is_none());
    }

    #[test]
    fn hello_accepted_on_correct_version() {
        let mut core = NmpRuntimeCore::new();
        let req = serde_json::json!({
            "type": "hello",
            "app_id": "test",
            "platform": "web",
            "protocol_version": 1
        });
        let resp = core.handle_json_request(&req.to_string());
        assert!(resp.contains("hello_accepted"), "resp={resp}");
    }

    #[test]
    fn hello_rejected_on_wrong_version() {
        let mut core = NmpRuntimeCore::new();
        let req = serde_json::json!({
            "type": "hello",
            "app_id": "test",
            "platform": "web",
            "protocol_version": 99
        });
        let resp = core.handle_json_request(&req.to_string());
        assert!(resp.contains("protocol_mismatch"), "resp={resp}");
    }

    #[test]
    fn start_creates_handle() {
        let mut core = NmpRuntimeCore::new();
        let resp = core.handle_json_request(&start_req());
        assert!(resp.contains("running"), "resp={resp}");
        assert!(core.handle.is_some(), "handle should be populated after start");
    }

    #[test]
    fn request_before_start_returns_not_started() {
        let mut core = NmpRuntimeCore::new();
        let resp = core.handle_json_request(&set_identity_req());
        assert!(resp.contains("not_started"), "resp={resp}");
    }

    #[test]
    fn recent_routing_decisions_returns_error_before_start() {
        let core = NmpRuntimeCore::new();
        let s = core.recent_routing_decisions();
        assert!(s.contains("not_started"), "s={s}");
    }

    #[test]
    fn recent_routing_decisions_returns_string_after_start() {
        let mut core = NmpRuntimeCore::new();
        let _ = core.handle_json_request(&start_req());
        let s = core.recent_routing_decisions();
        assert!(!s.contains("not_started"), "s={s}");
    }

    // ── BLOCKER 1: wake ordering ──────────────────────────────────────────────

    /// Snapshot sink installed BEFORE start must fire after start. This is the
    /// native-CI proxy for the wasm wake-ordering fix (#2139 BLOCKER 1): if
    /// the sink works before/after start, the wake (wasm-only) is also
    /// correctly deferred.
    #[test]
    fn snapshot_sink_set_before_start_receives_bytes_after_start() {
        let mut core = NmpRuntimeCore::new();

        let received = Arc::new(AtomicBool::new(false));
        let received2 = Arc::clone(&received);
        core.set_snapshot_sink(Some(Box::new(move |_bytes| {
            received2.store(true, Ordering::SeqCst);
        })));

        // Sink is set; start is NOT done yet.
        assert!(core.handle.is_none());

        // Now start.
        let _ = core.handle_json_request(&start_req());
        assert!(core.handle.is_some());

        // Push snapshot — sink must be called despite being set before start.
        core.push_snapshot_bytes_if_sink();
        assert!(
            received.load(Ordering::SeqCst),
            "sink set before start must fire after start (#2139 BLOCKER 1)"
        );
    }

    // ── BLOCKER 2: sign terminals emitted from deliver_signer_response ────────

    /// A `deliver_signer_response` with error must return `sign_failed` from
    /// `handle_json` (not an empty array). Proves sign terminals travel through
    /// the sync pump path (#2139 BLOCKER 2).
    #[test]
    fn deliver_signer_response_failure_emits_sign_failed() {
        let mut core = NmpRuntimeCore::new();
        let _ = core.handle_json_request(&start_req());
        let _ = core.handle_json_request(&set_identity_req());

        // Begin a sign round-trip so the kernel parks one.
        let sign_resp = core.handle_json_request(
            &serde_json::json!({
                "type": "begin_sign",
                "account_pubkey": PK,
                "unsigned_json": r#"{"kind":1,"created_at":0,"tags":[],"content":"hi"}"#
            })
            .to_string(),
        );
        let events: serde_json::Value =
            serde_json::from_str(&sign_resp).expect("valid JSON");
        let cid = events[0]["correlation_id"]
            .as_str()
            .expect("sign_request must have correlation_id")
            .to_string();

        // Deliver a failure — must produce sign_failed, not empty array.
        let resp = core.handle_json_request(
            &serde_json::json!({
                "type": "deliver_signer_response",
                "correlation_id": cid,
                "error": "user rejected"
            })
            .to_string(),
        );
        assert!(
            resp.contains("sign_failed"),
            "deliver_signer_response with error must emit sign_failed, got: {resp}"
        );
        assert!(
            resp.contains(&cid),
            "sign_failed must echo back correlation_id, got: {resp}"
        );
    }

    // ── BLOCKER 3: nmp_encode_npub JSON shape ─────────────────────────────────

    /// `nmp_encode_npub` must return a JSON object with `npub` and `npubShort`
    /// fields so `wasmBridge.ts`'s `JSON.parse(json)` call works (#2139 BLOCKER 3).
    #[test]
    fn encode_npub_returns_json_with_npub_and_npub_short() {
        let json = crate::wasm::nmp_encode_npub(PK);
        assert!(!json.is_empty(), "must return non-empty string for valid hex");
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("must be valid JSON");
        let npub = parsed["npub"].as_str().expect("npub field must be a string");
        let npub_short = parsed["npubShort"].as_str().expect("npubShort field must be a string");
        assert!(npub.starts_with("npub1"), "npub must start with npub1, got: {npub}");
        assert!(!npub_short.is_empty(), "npubShort must be non-empty");
        assert!(npub_short.contains('…'), "npubShort must be abbreviated with ellipsis");
    }

    // ── HIGH 4: identity_relays applied at set_active_account ────────────────

    /// Sending `set_identity` with `identity_relays` must result in those relays
    /// being added to the configured relay list (#2139 HIGH 4).
    #[test]
    fn set_identity_with_identity_relays_configures_relays() {
        let mut core = NmpRuntimeCore::new();
        let _ = core.handle_json_request(&start_req());

        // Start with zero configured relays.
        let relay_count_before = core
            .handle
            .as_ref()
            .unwrap()
            .configured_relays()
            .as_slice()
            .len();
        assert_eq!(relay_count_before, 0, "no relays before set_identity");

        // set_identity with identity_relays.
        let resp = core.handle_json_request(
            &serde_json::json!({
                "type": "set_identity",
                "kind": "nip07",
                "pubkey_hex": PK,
                "correlation_id": "id-1",
                "identity_relays": [
                    { "url": "wss://relay.example.com", "read": true, "write": true }
                ]
            })
            .to_string(),
        );
        assert!(resp.contains("action_accepted"), "resp={resp}");

        let relay_count_after = core
            .handle
            .as_ref()
            .unwrap()
            .configured_relays()
            .as_slice()
            .len();
        assert!(
            relay_count_after > relay_count_before,
            "identity relay must be added to configured relays (#2139 HIGH 4)"
        );
    }
}
