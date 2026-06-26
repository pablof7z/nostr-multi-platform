// Items in this module are primary consumers for the wasm32 target;
// on native they are exercised only from `#[cfg(test)]` blocks.
// `type_complexity` fires on the `SnapshotSink` type alias use-site even after
// aliasing — suppressed here since the type is inherently compositional.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#![allow(clippy::type_complexity)]

//! `NmpRuntimeCore` — non-wasm-gated request-handler for the wasm entry point.
//!
//! Contains the drive-loop and protocol-dispatch logic for `NmpWasmRuntime`
//! without the `#[wasm_bindgen]` annotations, `JsValue`, or `js_sys::Function`
//! dependencies. This lets native CI exercise the routing logic via
//! `cargo test -p nmp-browser-runtime`, while the thin `#[wasm_bindgen]`
//! wrappers in `super::mod.rs` provide the JS-facing surface.
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

use crate::runtime::DispatchBytesResult;
use crate::{BrowserAppBuilder, BrowserRunConfig, BrowserRuntimeHandle};

use super::identity::canonical_pubkey_from_kind;
use super::protocol::{
    relay_bootstrap_from_config, serialize_events, serialize_one, BeginSign, ClientHello,
    DeliverSignerResponse, ReleaseRef, ResolveRef, RuntimeStatus, SetIdentity, StartConfig,
    WorkerEvent, WorkerRequest, PROTOCOL_VERSION,
};
use super::ref_routing::{
    invalid_ref_request_reason, ref_dispatch_from_release, ref_dispatch_from_resolve,
    signer_not_installed_reason, RefDispatch,
};

// ── NmpRuntimeCore ────────────────────────────────────────────────────────────

/// Non-wasm-gated runtime core owned by `NmpWasmRuntime`.
///
/// Holds the `BrowserRuntimeHandle` (populated on `Start`) and the optional
/// snapshot callback sink. All protocol routing lives here so native CI can
/// compile and test the logic.
pub struct NmpRuntimeCore {
    /// The live runtime handle; `None` until a `Start` request is processed.
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

    // ── Private dispatch helpers ──────────────────────────────────────────────

    fn dispatch_request(&mut self, request: WorkerRequest) -> Vec<WorkerEvent> {
        match request {
            WorkerRequest::Hello(hello) => self.handle_hello(hello),
            WorkerRequest::Start(config) => self.handle_start(config),
            WorkerRequest::SetIdentity(req) => self.handle_set_identity(req),
            WorkerRequest::ResolveRef(req) => self.handle_resolve_ref(req),
            WorkerRequest::ReleaseRef(req) => self.handle_release_ref(req),
            WorkerRequest::BeginSign(req) => self.handle_begin_sign(req),
            WorkerRequest::DeliverSignerResponse(resp) => self.handle_deliver_signer_response(resp),
            WorkerRequest::DispatchBytes(payload) => self.dispatch_dispatch_bytes(&payload.bytes),
            WorkerRequest::CapabilityResult(r) => {
                // No native capability handler in this crate (requires native
                // actor); surface honestly rather than silently dropping.
                vec![WorkerEvent::CapabilityFailure {
                    capability: r.capability,
                    correlation_id: r.correlation_id,
                    reason: "browser_actor_driver_missing: capability completions require \
                             the native actor"
                        .to_string(),
                }]
            }
            WorkerRequest::Stop { correlation_id } => {
                self.handle = None;
                vec![WorkerEvent::RuntimeStatus {
                    status: RuntimeStatus::Stopped,
                    correlation_id: Some(correlation_id),
                }]
            }
        }
    }

    fn handle_hello(&self, hello: ClientHello) -> Vec<WorkerEvent> {
        if hello.protocol_version != PROTOCOL_VERSION {
            return vec![WorkerEvent::Error {
                code: "protocol_mismatch".to_string(),
                message: format!(
                    "expected protocol version {PROTOCOL_VERSION}, got {}",
                    hello.protocol_version
                ),
                correlation_id: None,
            }];
        }
        vec![WorkerEvent::HelloAccepted {
            protocol_version: PROTOCOL_VERSION,
            status: RuntimeStatus::Ready,
        }]
    }

    fn handle_start(&mut self, config: StartConfig) -> Vec<WorkerEvent> {
        if config.app_id.trim().is_empty() {
            return vec![WorkerEvent::Error {
                code: "invalid_config".to_string(),
                message: "app_id is required".to_string(),
                correlation_id: Some(config.correlation_id),
            }];
        }

        let bootstrap =
            relay_bootstrap_from_config(config.relays.clone(), config.relay_bootstrap);

        // Build the typed BrowserRuntimeHandle through the builder typestate.
        let builder = BrowserAppBuilder::new()
            .in_memory()
            .consume_all_builtin_projections();

        let builder = if bootstrap.is_empty() {
            builder.without_initial_relays()
        } else {
            let relay_pairs: Vec<(String, String)> = bootstrap
                .iter()
                .map(|e| (e.url.clone(), e.role.clone()))
                .collect();
            builder.set_relays(relay_pairs)
        };

        let handle = builder
            .decide_providers(BrowserRunConfig {
                app_id: config.app_id,
            })
            .with_system_clock()
            .start();

        self.handle = Some(handle);

        vec![WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Running,
            correlation_id: Some(config.correlation_id),
        }]
    }

    fn handle_set_identity(&mut self, req: SetIdentity) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        match canonical_pubkey_from_kind(&req.kind, &req.pubkey_hex) {
            Ok(canonical_hex) => {
                let outbound = handle.apply_set_active_account(canonical_hex);
                handle.fan_out_outbound(outbound);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.set_identity".to_string(),
                    correlation_id: req.correlation_id,
                }]
            }
            Err(err) => {
                vec![WorkerEvent::CapabilityFailure {
                    capability: "nmp.set_identity".to_string(),
                    correlation_id: req.correlation_id,
                    reason: err.detail(),
                }]
            }
        }
    }

    fn handle_resolve_ref(&mut self, req: ResolveRef) -> Vec<WorkerEvent> {
        let correlation_id = req.correlation_id.clone();
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(correlation_id));
        };

        match ref_dispatch_from_resolve(&req) {
            None => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.kernel.resolve_ref".to_string(),
                correlation_id,
                reason: invalid_ref_request_reason("nmp.kernel.resolve_ref"),
            }],
            Some(RefDispatch::Resolve {
                namespace,
                key,
                consumer_id,
                shape,
                liveness,
                metadata,
            }) => {
                let outbound = handle.apply_resolve_ref_with_metadata(
                    namespace, key, consumer_id, shape, liveness, metadata,
                );
                handle.fan_out_outbound(outbound);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.kernel.resolve_ref".to_string(),
                    correlation_id,
                }]
            }
            Some(_) => {
                // D6 — total/honest: ref_dispatch_from_resolve can only return
                // Resolve variants; a Release here is an invariant violation.
                // Never panic across the FFI boundary — surface as an error.
                vec![WorkerEvent::Error {
                    code: "invariant_violated".to_string(),
                    message: "ref_dispatch_from_resolve returned unexpected Release variant"
                        .to_string(),
                    correlation_id: Some(correlation_id),
                }]
            }
        }
    }

    fn handle_release_ref(&mut self, req: ReleaseRef) -> Vec<WorkerEvent> {
        let correlation_id = req.correlation_id.clone();
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(correlation_id));
        };

        match ref_dispatch_from_release(&req) {
            None => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.kernel.release_ref".to_string(),
                correlation_id,
                reason: invalid_ref_request_reason("nmp.kernel.release_ref"),
            }],
            Some(RefDispatch::Release {
                namespace,
                key,
                consumer_id,
            }) => {
                let outbound = handle.apply_release_ref(namespace, &key, &consumer_id);
                handle.fan_out_outbound(outbound);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.kernel.release_ref".to_string(),
                    correlation_id,
                }]
            }
            Some(_) => {
                // D6 — total/honest: ref_dispatch_from_release can only return
                // Release variants; a Resolve here is an invariant violation.
                // Never panic across the FFI boundary — surface as an error.
                vec![WorkerEvent::Error {
                    code: "invariant_violated".to_string(),
                    message: "ref_dispatch_from_release returned unexpected Resolve variant"
                        .to_string(),
                    correlation_id: Some(correlation_id),
                }]
            }
        }
    }

    fn handle_begin_sign(&mut self, req: BeginSign) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(None);
        };

        match handle.begin_sign_roundtrip(req.account_pubkey, &req.unsigned_json) {
            Ok(sign_req) => vec![WorkerEvent::SignRequest {
                correlation_id: sign_req.correlation_id,
                account_pubkey: sign_req.account_pubkey,
                unsigned_json: sign_req.unsigned_json,
            }],
            Err(reason) => vec![WorkerEvent::SignFailed {
                correlation_id: String::new(),
                reason,
            }],
        }
    }

    fn handle_deliver_signer_response(&mut self, resp: DeliverSignerResponse) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(resp.correlation_id));
        };

        let result = match (resp.signed_json, resp.error) {
            (_, Some(err)) => Err(err),
            (Some(json), None) => Ok(json),
            (None, None) => Err(
                "deliver_signer_response carried neither signed_json nor error".to_string(),
            ),
        };

        handle.deliver_signer_response(resp.correlation_id.clone(), result);
        // Events from the settled sign completion surface on the next pump turn.
        Vec::new()
    }

    fn dispatch_dispatch_bytes(&mut self, bytes: &[u8]) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(None);
        };

        match handle.apply_dispatch_bytes(bytes) {
            DispatchBytesResult::Applied {
                action_type,
                correlation_id,
            } => {
                vec![WorkerEvent::ActionAccepted {
                    action_type,
                    correlation_id,
                }]
            }
            DispatchBytesResult::SignRequired {
                correlation_id,
                account_pubkey,
                unsigned_json,
            } => {
                vec![WorkerEvent::SignRequest {
                    correlation_id,
                    account_pubkey,
                    unsigned_json,
                }]
            }
            DispatchBytesResult::Rejected {
                capability,
                correlation_id,
                reason,
            } => {
                vec![WorkerEvent::CapabilityFailure {
                    capability,
                    correlation_id,
                    reason,
                }]
            }
            DispatchBytesResult::NoActiveAccount {
                capability,
                correlation_id,
            } => {
                vec![WorkerEvent::CapabilityFailure {
                    capability,
                    correlation_id,
                    reason: signer_not_installed_reason(),
                }]
            }
            DispatchBytesResult::DecodeError { message } => {
                vec![WorkerEvent::Error {
                    code: "dispatch_envelope_rejected".to_string(),
                    message,
                    correlation_id: None,
                }]
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn not_started_error(correlation_id: Option<String>) -> Vec<WorkerEvent> {
    vec![WorkerEvent::Error {
        code: "not_started".to_string(),
        message: "runtime not started — send WorkerRequest::Start first".to_string(),
        correlation_id,
    }]
}

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
