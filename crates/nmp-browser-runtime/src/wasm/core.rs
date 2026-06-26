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
}

impl NmpRuntimeCore {
    /// Construct an unstarted core.
    pub fn new() -> Self {
        Self {
            handle: None,
            snapshot_sink: None,
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
        let events = self.dispatch_request(request);
        serialize_events(&events)
    }

    /// Handle raw binary `DispatchEnvelope` bytes (ADR-0064 binary write
    /// doorway). Bypasses JSON serialization of the bytes (which would corrupt
    /// them to `{}`).
    pub fn handle_dispatch_bytes_raw(&mut self, bytes: &[u8]) -> String {
        let events = self.dispatch_dispatch_bytes(bytes);
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
    /// Call this after every request that may have produced inbox activity.
    pub fn pump_once(&mut self) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return Vec::new();
        };
        let outcome = handle.pump();
        map_pump_events(outcome.events)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Map `BrowserRuntimeEvent`s from a pump turn to `WorkerEvent`s.
fn map_pump_events(events: Vec<crate::BrowserRuntimeEvent>) -> Vec<WorkerEvent> {
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
            // Future: buffer in pending_host_events for the next handle_json call.
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
    use super::*;

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
        let req = serde_json::json!({
            "type": "start",
            "app_id": "chirp",
            "relays": [],
            "relay_bootstrap": [],
            "database_name": "chirp-test",
            "correlation_id": "start-1"
        });
        let resp = core.handle_json_request(&req.to_string());
        assert!(resp.contains("running"), "resp={resp}");
        assert!(core.handle.is_some(), "handle should be populated after start");
    }

    #[test]
    fn request_before_start_returns_not_started() {
        let mut core = NmpRuntimeCore::new();
        let req = serde_json::json!({
            "type": "set_identity",
            "kind": "nip07",
            "pubkey_hex": "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
            "correlation_id": "id-1"
        });
        let resp = core.handle_json_request(&req.to_string());
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
        let req = serde_json::json!({
            "type": "start",
            "app_id": "chirp",
            "relays": [],
            "relay_bootstrap": [],
            "database_name": "chirp-test",
            "correlation_id": "start-1"
        });
        let _ = core.handle_json_request(&req.to_string());
        let s = core.recent_routing_decisions();
        // Should return valid JSON (not the error sentinel).
        assert!(!s.contains("not_started"), "s={s}");
    }
}
