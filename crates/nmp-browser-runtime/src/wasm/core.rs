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
//! `push_snapshot_bytes_if_sink`; it pushes a merged snapshot only when the
//! kernel is dirty. The snapshot sink is `Box<dyn Fn(&[u8])>` here
//! (host-callback-agnostic); the wasm layer installs a closure that calls
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
    /// Durable store opened by the async pre-`Start` hook
    /// (`NmpWasmRuntime::prepare_store`, #1007 PR-7), parked for `handle_start` to
    /// inject. `None` → `handle_start` falls back to `.in_memory()`. Native keeps
    /// it `None`: only the wasm32 OPFS hook populates it.
    pub(super) injected_store: Option<std::sync::Arc<dyn nmp_store::EventStore>>,
    /// Stable degraded-open reason parked by `prepare_store` when the OPFS-SQLite
    /// open FAILED (#1007 PR-8). `handle_start` threads it through
    /// `BrowserAppBuilder::with_store_open_failure` so the in-memory fallback
    /// session reports the Tier-3 `store_open_failure` diagnostic. Mutually
    /// exclusive with `injected_store` in practice (a successful open carries no
    /// reason). Native keeps it `None`.
    pub(super) store_open_failure: Option<String>,
}

impl NmpRuntimeCore {
    /// Construct an unstarted core.
    pub fn new() -> Self {
        Self {
            handle: None,
            snapshot_sink: None,
            pending_host_events: Vec::new(),
            injected_store: None,
            store_open_failure: None,
        }
    }

    /// Stash the durable `EventStore` opened by the async pre-`Start` hook
    /// (`NmpWasmRuntime::prepare_store`) so the next synchronous `Start` injects
    /// it instead of `.in_memory()` (#1007 PR-7 — the async-open-before-`Start`
    /// seam). `handle_start` consumes it via `injected_store.take()`.
    pub fn set_injected_store(&mut self, store: std::sync::Arc<dyn nmp_store::EventStore>) {
        self.injected_store = Some(store);
    }

    /// Park the stable degraded-open reason from a failed `prepare_store` OPFS
    /// open (#1007 PR-8). `handle_start` consumes it via
    /// `store_open_failure.take()` and threads it onto the kernel's Tier-3
    /// `store_open_failure` diagnostic. Only the wasm32 OPFS hook calls this.
    pub fn set_store_open_failure(&mut self, reason: String) {
        self.store_open_failure = Some(reason);
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
        events.extend(self.pump_once());
        serialize_events(&events)
    }

    /// Handle raw binary `DispatchEnvelope` bytes (ADR-0071 binary write
    /// doorway). Bypasses JSON serialization of the bytes (which would corrupt
    /// them to `{}`).
    pub fn handle_dispatch_bytes_raw(&mut self, bytes: &[u8]) -> String {
        let mut events = std::mem::take(&mut self.pending_host_events);
        events.extend(self.dispatch_dispatch_bytes(bytes));
        events.extend(self.pump_once());
        serialize_events(&events)
    }

    /// JSON snapshot of the kernel's recent routing decisions (pull-only).
    pub fn recent_routing_decisions(&self) -> String {
        match &self.handle {
            Some(h) => h.recent_routing_decisions_json(),
            None => r#"{"error":"not_started"}"#.to_string(),
        }
    }

    /// Push the current merged snapshot to the installed sink, if any, but only
    /// when the kernel reports a real change since the last emitted frame.
    ///
    /// Called after each mutable operation and after relay-driven async pumps.
    pub fn push_snapshot_bytes_if_sink(&mut self) {
        if self.snapshot_sink.is_none() {
            return;
        }
        if let Some(h) = self.handle.as_mut() {
            if let Some(bytes) = h.next_frame_if_dirty(true) {
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

/// Compose the per-app OPFS-SQLite database name (#1007 PR-7).
///
/// `app_id` namespaces durable storage so two NMP apps in one origin's OPFS never
/// collide; `database_name` is the app's logical store. Both trimmed; empty parts
/// collapse (no stray separator); a fully empty pair degrades to a stable `"nmp"`
/// (D6 — total: always a usable, deterministic name, never panics).
pub(super) fn opfs_database_name(app_id: &str, database_name: &str) -> String {
    match (app_id.trim(), database_name.trim()) {
        ("", "") => "nmp".to_string(),
        (app, "") => app.to_string(),
        ("", db) => db.to_string(),
        (app, db) => format!("{app}-{db}"),
    }
}

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
                action_correlation_id: None,
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
            | BrowserRuntimeEvent::RelayOutboundDropped { .. }
            | BrowserRuntimeEvent::SnapshotDecodeFailed { .. } => None,
        })
        .collect()
}

// Unit tests live in the sibling `core_tests.rs` to keep this file under the
// 500-LOC ceiling (AGENTS.md). They share this module's private surface via
// `use super::*`.
#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
