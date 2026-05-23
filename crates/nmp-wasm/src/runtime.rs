//! Browser-side runtime built on the pure `KernelReducer` from `nmp-core`.
//!
//! # V-01 Stage 3 status
//!
//! The runtime drives a real [`nmp_core::KernelReducer`] (Stage 2) AND, on
//! `wasm32`, owns a pool of [`crate::relay_driver::BrowserRelayDriver`]s — one
//! `web_sys::WebSocket` per (URL, role) pair (Stage 3). Inbound relay frames
//! arrive on the JS event loop, route through `KernelReducer::handle_relay_frame`,
//! and the resulting outbound is fanned back out over the same sockets. The
//! kernel's `RelayStatus`, `RelayFrame` ingest, and `OutboundMessage` paths
//! are exercised end-to-end against live relays.
//!
//! On native targets the relay-driver pool is conditionally compiled out
//! (the native crate already owns the native transport via `relay_worker`);
//! `handle()` still works synchronously for the protocol-conformance tests
//! that the workspace runs on native CI.
//!
//! # What is real
//!
//! - `Start` / `Stop` dispatch through `KernelReducer::reduce` and produce
//!   real `KernelUpdate` values.
//! - `OpenUri` routes through `resolve_open_uri` and emits the corresponding
//!   `ViewOpened` update.
//! - Snapshot envelopes are produced via [`nmp_core::wrap_snapshot`].
//! - **(wasm32)** Relay sockets dial on `Start`, reconnect with the same
//!   exponential backoff + jitter constants the native worker uses, ingest
//!   frames into the kernel, and route outbound back to the wire.
//!
//! # What is honestly stubbed (Stage 3b / V-01 follow-up)
//!
//! - **Event store.** No `nostr-database` / IndexedDB backend is wired yet —
//!   the kernel runs entirely in memory and resets on page reload.
//! - **Identity / signing.** App-level *writes* (`PublishNote`, `React`,
//!   `Follow`, `Unfollow`) still return a `CapabilityFailure` carrying a
//!   `BrowserActorDriverMissing`-style reason — signing requires the identity
//!   runtime + bunker hooks (`actor::commands::sign_in_*`) that live behind
//!   `feature = "native"`. The wasm32 read path (relay frames → kernel →
//!   snapshot) is functional; the write path is not yet.
//! - **Async snapshot push.** Relay-driven kernel mutations don't yet push
//!   a fresh snapshot to JS — the JS host pulls by dispatching a
//!   `nmp.kernel.diagnostics` (or any other read action). A push channel via
//!   `js_sys::Function` callback is Stage 3b.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use nmp_core::{wrap_snapshot, KernelAction, KernelReducer, KernelUpdate, SNAPSHOT_SCHEMA_VERSION};
use serde_json::Value;

#[cfg(target_arch = "wasm32")]
use crate::relay_driver::BrowserRelayDriver;
#[cfg(target_arch = "wasm32")]
use crate::relay_pool;

use crate::protocol::{
    ActionDispatch, AppAction, CapabilityFailure, RelayBootstrapEntry, RuntimeStatus, StartConfig,
    WorkerEvent, WorkerRequest,
};

const PROTOCOL_VERSION: u16 = 1;

/// Browser-side runtime backed by a real `KernelReducer`.
///
/// `Default::default()` constructs the reducer eagerly — the kernel is cheap
/// to allocate (no I/O, no threads) and constructing it lazily would complicate
/// the snapshot path that runs before `Start` arrives (the `protocol_mismatch`
/// branch needs no kernel; everything else does).
pub struct WasmRuntime {
    /// Pure protocol kernel — the same reducer the native actor loop uses.
    /// Held behind `Rc<RefCell>` so the wasm32 relay-driver closures can
    /// share it without unsafe lifetime gymnastics. On native the wrapper is
    /// effectively free (single-owned, single-threaded test harness) and
    /// keeps both call sites symmetrical.
    reducer: Rc<RefCell<KernelReducer>>,
    /// `Start` flips this to `true`; `Stop` flips it back. Mirrors the
    /// "running" flag the native actor exposes through its snapshot.
    started: bool,
    /// Relay bootstrap captured at `Start` time. The pure kernel does not yet
    /// own a relay-bootstrap list (Stage 3 will wire the snapshot
    /// `relay_diagnostics` projection through `Kernel::relay_statuses`); for
    /// now the runtime carries it so the snapshot envelope can surface the
    /// configured relays as a `configured` diagnostic — proving the
    /// `StartConfig` was honored end-to-end.
    relay_bootstrap: Vec<RelayBootstrapEntry>,
    /// Database name captured at `Start` time. The pure kernel never sees a
    /// database (no IndexedDB binding yet — Stage 3b). Echoed back through the
    /// snapshot so hosts can verify the start handshake.
    database_name: String,
    /// Monotonic revision counter, mirroring the kernel's own `rev` field
    /// (visible through `KernelUpdate::Started { rev }`). Bumped on every
    /// successful kernel-driven update so hosts can apply the bible's
    /// monotonic-revision-guard rule.
    rev: u64,
    /// Live `web_sys::WebSocket` drivers — one per relay URL in the bootstrap.
    /// `wasm32`-only: native tests never construct drivers. The
    /// `RefCell<Vec<…>>` lets the sink closure (registered at `Start` time)
    /// look up the driver by URL on every outbound fan-out.
    #[cfg(target_arch = "wasm32")]
    relays: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self {
            reducer: Rc::new(RefCell::new(KernelReducer::new())),
            started: false,
            relay_bootstrap: Vec::new(),
            database_name: String::new(),
            rev: 0,
            #[cfg(target_arch = "wasm32")]
            relays: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

impl WasmRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Process one `WorkerRequest` and return the events to forward back to
    /// JS. Total — never panics. Returns `Err` only for caller-side validation
    /// failures (`InvalidConfig`); kernel-side rejections surface as
    /// `WorkerEvent::CapabilityFailure` so the JS host has one event channel
    /// instead of two.
    pub fn handle(&mut self, request: WorkerRequest) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        match request {
            WorkerRequest::Hello(hello) => {
                if hello.protocol_version != PROTOCOL_VERSION {
                    return Ok(vec![WorkerEvent::Error {
                        code: "protocol_mismatch".to_string(),
                        message: format!(
                            "expected protocol {PROTOCOL_VERSION}, got {}",
                            hello.protocol_version
                        ),
                        correlation_id: None,
                    }]);
                }
                Ok(vec![WorkerEvent::HelloAccepted {
                    protocol_version: PROTOCOL_VERSION,
                    status: RuntimeStatus::Ready,
                }])
            }
            WorkerRequest::Start(config) => self.start(config),
            WorkerRequest::AppAction(action) => {
                self.app_action(action.action, action.correlation_id)
            }
            WorkerRequest::Dispatch(action) => self.dispatch(action),
            WorkerRequest::CapabilityResult(result) => {
                // The native actor handles capability completions through its
                // capability-socket arm; that arm lives behind the `native`
                // feature gate and is not reachable here. Surface the
                // completion as a no-op failure so the host sees an honest
                // "no driver yet" signal rather than silent drop.
                Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: result.capability,
                    correlation_id: result.correlation_id,
                    reason: browser_driver_missing_reason(),
                })])
            }
            WorkerRequest::Stop { correlation_id } => self.stop(correlation_id),
        }
    }

    fn start(&mut self, config: StartConfig) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        if config.app_id.trim().is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "app_id is required".to_string(),
            ));
        }
        if config.database_name.trim().is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "database_name is required".to_string(),
            ));
        }
        if config.relays.is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "at least one relay is required".to_string(),
            ));
        }

        // Drive the pure kernel through its `Start` action — this is the same
        // reducer entry point `dispatch_kernel_action` calls on the native
        // actor thread, byte-for-byte. The returned `Started { rev }` is the
        // ground truth for the runtime's own monotonic counter.
        let started = self.reducer.borrow_mut().reduce(KernelAction::Start);
        match started {
            KernelUpdate::Started { rev } => {
                self.rev = rev;
            }
            // The `Start` arm of `dispatch_kernel_action` only ever emits
            // `Started`; any other variant would indicate a kernel contract
            // change. Surface it as InvalidConfig so the host sees a loud
            // failure rather than a silently degraded runtime.
            other => {
                return Err(WasmRuntimeError::KernelContract(format!(
                    "expected Started after KernelAction::Start, got {other:?}"
                )));
            }
        }

        self.started = true;
        self.relay_bootstrap = relay_bootstrap_from_config(config.relays, config.relay_bootstrap);
        self.database_name = config.database_name;

        // V-01 Stage 3 — spawn one `BrowserRelayDriver` per relay URL on
        // wasm32. The driver dials immediately and routes inbound frames
        // through `KernelReducer::handle_relay_frame`; outbound from those
        // frames fan out over the sink registered below. Native builds skip
        // this path entirely — they have no `web_sys` and use the native
        // `relay_worker` thread when they want real transport.
        #[cfg(target_arch = "wasm32")]
        self.spawn_relay_drivers()?;

        Ok(vec![
            WorkerEvent::RuntimeStatus {
                status: RuntimeStatus::Running,
                correlation_id: Some(config.correlation_id),
            },
            self.snapshot_event(),
        ])
    }

    fn stop(&mut self, correlation_id: String) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // Tear down every live relay driver — closing the JS sockets and
        // dropping the parked closures so the user-agent reclaims them.
        // Order matters: close sockets BEFORE driving the kernel `Stop`,
        // because the kernel's `Stop` arm resets the per-relay state we
        // want to settle observers on.
        #[cfg(target_arch = "wasm32")]
        relay_pool::close_drivers(&self.relays);

        // Mirror `Start`: drive the kernel through its `Stop` action so the
        // single reducer is the single writer of the running/stopped flag.
        let stopped = self.reducer.borrow_mut().reduce(KernelAction::Stop);
        match stopped {
            KernelUpdate::Stopped { rev } => {
                self.rev = rev;
            }
            other => {
                return Err(WasmRuntimeError::KernelContract(format!(
                    "expected Stopped after KernelAction::Stop, got {other:?}"
                )));
            }
        }
        self.started = false;
        Ok(vec![WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Stopped,
            correlation_id: Some(correlation_id),
        }])
    }

    /// V-01 Stage 3 — instantiate one `BrowserRelayDriver` per configured
    /// relay URL and wire each one's outbound sink so the kernel's responses
    /// (AUTH replies, EOSE-CLOSEs, view REQs registered while the socket was
    /// dialling) fan back out over the same socket pool. Implementation lives
    /// in [`crate::relay_pool`].
    #[cfg(target_arch = "wasm32")]
    fn spawn_relay_drivers(&mut self) -> Result<(), WasmRuntimeError> {
        let sink = relay_pool::build_sink(Rc::clone(&self.relays));
        let drivers = relay_pool::spawn_drivers(
            &self.relay_bootstrap,
            Rc::clone(&self.reducer),
            sink,
        )?;
        *self.relays.borrow_mut() = drivers;
        Ok(())
    }

    fn app_action(
        &mut self,
        action: AppAction,
        correlation_id: String,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        let (action_type, _payload) = action.into_dispatch_parts();
        // Every variant of `AppAction` is a *write* — PublishNote, React,
        // Follow, Unfollow all sign and publish events through the actor's
        // publish-engine path. Without the actor (gated behind `native`) and
        // without a wasm relay transport (Stage 3), no write can be honored.
        // Honest failure beats fabricated success: report
        // BrowserActorDriverMissing rather than fabricating a fake snapshot.
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: browser_driver_missing_reason(),
        })])
    }

    fn dispatch(&mut self, action: ActionDispatch) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // Generic `ActionDispatch` covers everything `AppAction` does plus the
        // kernel-namespaced actions (`nmp.open_uri`, `nmp.kernel.diagnostics`,
        // ...) the bible specifies. The kernel-namespaced ones map directly to
        // `KernelAction` variants and run through `KernelReducer::reduce`; the
        // app-namespaced ones (anything that emits a signed event) hit the
        // same BrowserActorDriverMissing wall as `app_action` above.
        if let Some(kernel_action) = kernel_action_from_dispatch(&action) {
            let update = self.reducer.borrow_mut().reduce(kernel_action);
            // The kernel reducer is the source of truth for the
            // `Started`/`Stopped` transition; mirror its outcome into the
            // runtime's `started` flag and `rev` counter so subsequent
            // snapshots stay coherent across both the typed `Start`/`Stop`
            // entry points and the generic `Dispatch` path.
            match update {
                KernelUpdate::Started { rev } => {
                    self.rev = rev;
                    self.started = true;
                }
                KernelUpdate::Stopped { rev } => {
                    self.rev = rev;
                    self.started = false;
                }
                // `ViewOpened` / `ViewClosed` / `Diagnostics` / `UriRejected`
                // are read-side operations on the registry; the kernel does
                // not bump `rev` and the runtime's running flag is unchanged.
                _ => {}
            }
            return Ok(vec![
                WorkerEvent::ActionAccepted {
                    action_type: action.action_type,
                    correlation_id: action.correlation_id,
                },
                self.snapshot_event(),
            ]);
        }
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action.action_type,
            correlation_id: action.correlation_id,
            reason: browser_driver_missing_reason(),
        })])
    }

    /// Build the snapshot envelope using the same `wrap_snapshot` helper the
    /// native actor uses, so the on-wire `{"t":"snapshot","v":…}` shape is
    /// identical across native and wasm hosts.
    ///
    /// The `v` payload is intentionally minimal in Stage 2 — only the fields
    /// derived from real kernel state (`rev`, `running`) plus the runtime's
    /// captured `relay_diagnostics` are populated. The kernel's full
    /// projection set (timeline, profile cards, publish-outbox, ...) is
    /// reachable only through `Kernel::make_update`, which is `pub(crate)` and
    /// runs inside the actor loop; exposing a non-native equivalent is part of
    /// Stage 3 alongside the relay-transport wiring.
    fn snapshot_event(&self) -> WorkerEvent {
        let envelope = self.snapshot_value();
        WorkerEvent::Update { envelope }
    }

    fn snapshot_value(&self) -> Value {
        // The inner snapshot shape — host shells decode this as the `v` field
        // of the canonical `UpdateEnvelope::Snapshot` variant. Stage 2 keeps
        // the surface intentionally small; Stage 3 expands it to mirror the
        // native `Kernel::make_update` projection set.
        let snapshot = serde_json::json!({
            "schema_version": SNAPSHOT_SCHEMA_VERSION,
            "rev": self.rev,
            "running": self.started,
            "database_name": self.database_name,
            "projections": {
                "relay_diagnostics": self.relay_bootstrap.iter().map(|relay| {
                    serde_json::json!({
                        "url": relay.url,
                        "role": relay.role,
                        // "configured" means the relay was named in
                        // `StartConfig` but no live transport has yet
                        // connected to it — the honest state given Stage 3
                        // (relay worker) has not landed. The native runtime
                        // would surface "connected"/"degraded"/... here once
                        // the wasm relay worker is wired.
                        "status": "configured",
                    })
                }).collect::<Vec<_>>()
            }
        });

        // Round-trip through `wrap_snapshot` so the on-wire envelope is
        // bit-identical to the native path (`{"t":"snapshot","v":…}`). Falling
        // back to the bare snapshot keeps the host functional if serialization
        // somehow fails — preferable to dropping the frame.
        let snapshot_json = serde_json::to_string(&snapshot)
            .unwrap_or_else(|_| String::from(r#"{"schema_version":1,"rev":0,"running":false}"#));
        wrap_snapshot(snapshot_json)
            .and_then(|wire| serde_json::from_str::<Value>(&wire).ok())
            .unwrap_or(serde_json::json!({
                "t": "snapshot",
                "v": snapshot,
            }))
    }
}

/// Map a generic `ActionDispatch` to its `KernelAction` if (and only if) the
/// `action_type` is in the kernel namespace. Returns `None` for app-namespaced
/// actions, which the caller surfaces as `BrowserActorDriverMissing` until
/// Stage 3 wires a relay transport.
///
/// Kept narrow on purpose: only the actions whose entire implementation lives
/// in the pure reducer are routed. Anything that needs the actor (signed-event
/// publication, capability dispatch, planner driver) returns `None`.
fn kernel_action_from_dispatch(action: &ActionDispatch) -> Option<KernelAction> {
    match action.action_type.as_str() {
        "nmp.kernel.start" => Some(KernelAction::Start),
        "nmp.kernel.stop" => Some(KernelAction::Stop),
        "nmp.kernel.diagnostics" => Some(KernelAction::RunDiagnostics),
        "nmp.kernel.open_uri" => action
            .payload
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| KernelAction::OpenUri { uri: uri.to_string() }),
        "nmp.kernel.open_view" => {
            let namespace = action.payload.get("namespace").and_then(Value::as_str)?;
            let key = action.payload.get("key").and_then(Value::as_str)?;
            Some(KernelAction::OpenView {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
        }
        "nmp.kernel.close_view" => {
            let namespace = action.payload.get("namespace").and_then(Value::as_str)?;
            let key = action.payload.get("key").and_then(Value::as_str)?;
            Some(KernelAction::CloseView {
                namespace: namespace.to_string(),
                key: key.to_string(),
            })
        }
        _ => None,
    }
}

/// Single source of truth for the "the wasm runtime cannot honor relay-backed
/// actions until the Stage 3 transport lands" message. Stable string so JS
/// hosts can pattern-match it for a degraded-mode banner without parsing the
/// full reason text.
fn browser_driver_missing_reason() -> String {
    "browser_actor_driver_missing: wasm relay transport is not yet wired (V-01 Stage 3 — \
     web_sys::WebSocket bridge). Live relay-backed actions and capability completions \
     require the actor + relay_worker, both gated behind `feature = \"native\"`."
        .to_string()
}

fn relay_bootstrap_from_config(
    relays: Vec<String>,
    relay_bootstrap: Vec<RelayBootstrapEntry>,
) -> Vec<RelayBootstrapEntry> {
    if !relay_bootstrap.is_empty() {
        return relay_bootstrap;
    }
    relays
        .into_iter()
        .map(|url| RelayBootstrapEntry {
            url,
            role: "both".to_string(),
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub enum WasmRuntimeError {
    InvalidConfig(String),
    /// The pure `KernelReducer` returned an unexpected `KernelUpdate` variant
    /// for a `KernelAction` whose contract is single-valued (e.g. `Start`
    /// always yields `Started`). Surfaced rather than panicked so the host
    /// sees a loud failure if the kernel contract ever changes underneath us.
    KernelContract(String),
}

impl fmt::Display for WasmRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfig(message) => write!(formatter, "invalid config: {message}"),
            Self::KernelContract(message) => write!(formatter, "kernel contract: {message}"),
        }
    }
}

impl std::error::Error for WasmRuntimeError {}
