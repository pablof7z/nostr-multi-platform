//! Browser-side runtime built on the pure `KernelReducer` from `nmp-core`.
//!
//! # V-01 Stage 3b status
//!
//! Stage 3 (read path) shipped the live relay transport: `WasmRuntime` drives
//! a real [`nmp_core::KernelReducer`] AND, on `wasm32`, owns a pool of
//! [`nmp_network::browser_driver::BrowserRelayDriver`]s — one
//! `web_sys::WebSocket` per (URL, role) pair. Inbound relay frames arrive on
//! the JS event loop, route through `KernelReducer::handle_relay_frame`
//! (wrapped by the [`crate::relay_pool::build_handlers`] callback bag), and
//! the resulting outbound is fanned back out over the same sockets.
//!
//! Stage 3b (this commit) adds the **signer install path** and the **async
//! snapshot push channel**:
//!
//! - [`crate::protocol::WorkerRequest::SetSigner`] installs an
//!   `Arc<dyn nmp_signers::Signer>` into the runtime's signer slot. The only
//!   wired kind today is `"nip07"`, which the host first handshakes
//!   asynchronously through JS (`window.nostr.getPublicKey()`) before sending
//!   the wasm-side install request with the cached pubkey hex.
//! - The `NmpWasmRuntime::set_snapshot_callback` wasm-bindgen method stores
//!   a `js_sys::Function` the relay-pool sink invokes whenever an inbound
//!   relay frame mutates kernel state. The callback receives the same binary
//!   update event shape `handle()` returns synchronously, so the JS
//!   event-handling code does not branch on push vs. pull.
//!
//! What Stage 3b deliberately does NOT do (Stage 3c follow-up):
//!
//! - **In-process publish path.** App-level writes (PublishNote / React /
//!   Follow / Unfollow) need a `KernelReducer` surface that takes a
//!   `SignedEvent` and routes it through `PublishEngine`. That surface does
//!   not yet exist — the native path goes through `ActorCommand` which is
//!   `feature = "native"`-gated. Until that lands, app writes return
//!   `signer_not_installed` (no signer in the slot) or `publish_path_not_wired`
//!   (signer present but no kernel-publish surface to feed it through).
//! - **IndexedDB store.** Kernel still runs in memory, resets on page reload.
//!
//! # What is real (Stage 3 + Stage 3b combined)
//!
//! - `Start` / `Stop` dispatch through `KernelReducer::reduce` and produce
//!   real `KernelUpdate` values.
//! - `OpenUri` routes through `resolve_open_uri` and emits the corresponding
//!   `ViewOpened` update.
//! - Snapshot updates are produced as FlatBuffers `UpdateFrame` bytes.
//! - **(wasm32)** Relay sockets dial on `Start`, reconnect with the same
//!   exponential backoff + jitter constants the native worker uses, ingest
//!   frames into the kernel, route outbound back to the wire, and push a
//!   fresh snapshot to the JS host through the registered callback (if any).
//! - **(wasm32, feature = "wasm" in nmp-signers)** `Nip07Signer::sign()`
//!   bridges into `window.nostr.signEvent(...)` via `spawn_local`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use nmp_core::{KernelAction, KernelReducer, KernelUpdate};
use nmp_signers::Signer;

#[cfg(target_arch = "wasm32")]
use crate::relay_pool;
#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::BrowserRelayDriver;

use crate::dispatch_routing::{
    browser_driver_missing_reason, claim_dispatch_from_action, kernel_action_from_dispatch,
    write_path_unavailable_reason, ClaimDispatch,
};
use crate::protocol::{
    ActionDispatch, AppAction, CapabilityFailure, RuntimeStatus, SetSigner, StartConfig,
    WorkerEvent, WorkerRequest,
};
use crate::signer_slot;
use crate::snapshot::{build_snapshot_bytes, RuntimeMeta};

const PROTOCOL_VERSION: u16 = 1;

/// Type alias for the snapshot-callback slot. On `wasm32` this is a real JS
/// function the host installed; on native targets there is no JS to call so
/// the slot carries `()` (the push helper is a no-op shim). Keeping the alias
/// makes the runtime struct definition portable across targets without a
/// `#[cfg]` on every field reference.
#[cfg(target_arch = "wasm32")]
type SnapshotCallback = js_sys::Function;
#[cfg(not(target_arch = "wasm32"))]
type SnapshotCallback = ();

/// Browser-side runtime backed by a real `KernelReducer` plus the Stage 3b
/// signer slot and snapshot-callback push channel.
///
/// `Default::default()` constructs the reducer eagerly — the kernel is cheap
/// to allocate (no I/O, no threads) and constructing it lazily would complicate
/// the snapshot path that runs before `Start` arrives.
pub struct WasmRuntime {
    /// Pure protocol kernel — the same reducer the native actor loop uses.
    /// Held behind `Rc<RefCell>` so the wasm32 relay-driver closures can
    /// share it without unsafe lifetime gymnastics.
    reducer: Rc<RefCell<KernelReducer>>,
    /// Runtime metadata mirrored into every snapshot update. Shared with
    /// the relay-pool sink via `Rc<RefCell>` so the sink can build a fresh
    /// snapshot from kernel + meta without holding a reference to the
    /// runtime itself (which the sink, captured by JS event handlers,
    /// cannot).
    meta: Rc<RefCell<RuntimeMeta>>,
    /// V-01 Stage 3b — signer slot. `None` until the host calls
    /// `SetSigner`. App-level writes that hit `app_action()` distinguish
    /// the two states (no slot → `signer_not_installed`; slot filled →
    /// `publish_path_not_wired`) so the JS host can present an honest UX
    /// banner instead of guessing.
    ///
    /// `Arc<dyn Signer>` (not `Rc`) matches the existing `nmp-signers`
    /// shape — `Signer` is `Send + Sync` because the native actor loop
    /// hands signer ops across threads. On wasm32 there are no threads
    /// to cross; the `Arc` cost over `Rc` is one atomic increment per
    /// install and is otherwise free.
    signer: Option<Arc<dyn Signer>>,
    /// V-01 Stage 3b — snapshot push callback. Wasm32 stores the JS
    /// `Function`; native carries `()`. The relay-pool sink reads this slot
    /// after every kernel-mutating inbound frame and pushes a fresh snapshot
    /// if a callback is installed. Unused on native (no JS to call into;
    /// `set_snapshot_callback` is a no-op shim), so silence the dead-code
    /// warning the symmetric struct layout otherwise triggers there.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    snapshot_callback: Rc<RefCell<Option<SnapshotCallback>>>,
    /// Live `web_sys::WebSocket` drivers — one per relay URL in the bootstrap.
    /// `wasm32`-only: native tests never construct drivers.
    #[cfg(target_arch = "wasm32")]
    relays: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self {
            reducer: Rc::new(RefCell::new(KernelReducer::new())),
            meta: Rc::new(RefCell::new(RuntimeMeta::new())),
            signer: None,
            snapshot_callback: Rc::new(RefCell::new(None)),
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

    /// Install (or clear, with `None`) the snapshot push callback. Wasm32
    /// only — native targets have no `js_sys::Function` to install.
    ///
    /// Calling this with `Some(f)` replaces any previously-installed
    /// callback atomically (the slot is swapped under a single `RefMut`
    /// borrow). Calling with `None` clears the slot; subsequent relay
    /// frames will not push, and the host falls back to pull-by-dispatch.
    #[cfg(target_arch = "wasm32")]
    pub fn set_snapshot_callback(&mut self, callback: Option<js_sys::Function>) {
        *self.snapshot_callback.borrow_mut() = callback;
    }

    /// Hand the wasm-bindgen wrapper a borrow of the snapshot-callback slot
    /// so the `handle_json` drain path can route `UpdateBytes` through the
    /// same `Uint8Array` channel the relay-pool sink uses. Wasm32-only —
    /// callers off-wasm have no `js_sys::Function` to push to.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn snapshot_callback_handle(
        &self,
    ) -> &Rc<RefCell<Option<js_sys::Function>>> {
        &self.snapshot_callback
    }

    /// Native test-side shim — the wasm-bindgen `NmpWasmRuntime` only
    /// exposes the `wasm32` method, but the protocol-conformance tests run
    /// on native CI and need a no-op equivalent so the test target compiles
    /// without `#[cfg]` fences in every fixture.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_snapshot_callback(&mut self, _callback: Option<()>) {}

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
            WorkerRequest::SetSigner(request) => Ok(self.set_signer(request)),
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

        // Drive the pure kernel through its `Start` action — same reducer
        // entry point `dispatch_kernel_action` calls on the native actor
        // thread, byte-for-byte. The returned `Started { rev }` is the
        // ground truth for the runtime's own monotonic counter.
        let started = self.reducer.borrow_mut().reduce(KernelAction::Start);
        let rev = match started {
            KernelUpdate::Started { rev } => rev,
            other => {
                return Err(WasmRuntimeError::KernelContract(format!(
                    "expected Started after KernelAction::Start, got {other:?}"
                )));
            }
        };

        {
            let mut meta = self.meta.borrow_mut();
            meta.started = true;
            meta.rev = rev;
            meta.relay_bootstrap =
                relay_bootstrap_from_config(config.relays, config.relay_bootstrap);
            meta.database_name = config.database_name;
        }

        // V-01 Stage 3 — spawn one `BrowserRelayDriver` per relay URL on
        // wasm32. Native builds skip this path entirely.
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

        let stopped = self.reducer.borrow_mut().reduce(KernelAction::Stop);
        let rev = match stopped {
            KernelUpdate::Stopped { rev } => rev,
            other => {
                return Err(WasmRuntimeError::KernelContract(format!(
                    "expected Stopped after KernelAction::Stop, got {other:?}"
                )));
            }
        };
        {
            let mut meta = self.meta.borrow_mut();
            meta.rev = rev;
            meta.started = false;
        }
        Ok(vec![WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Stopped,
            correlation_id: Some(correlation_id),
        }])
    }

    /// V-01 Stage 3 — instantiate one `BrowserRelayDriver` per configured
    /// relay URL. Wires each driver's kernel-handler callbacks (Step 8
    /// phase C: the driver itself lives in `nmp-network` and is kernel-
    /// agnostic; the callback bag bridges it back into our `KernelReducer`)
    /// to the relay-pool helpers, which also push a snapshot through the
    /// registered callback (if any) so the JS host sees kernel mutations
    /// as they happen.
    #[cfg(target_arch = "wasm32")]
    fn spawn_relay_drivers(&mut self) -> Result<(), WasmRuntimeError> {
        let handlers = relay_pool::build_handlers(
            Rc::clone(&self.relays),
            Rc::clone(&self.snapshot_callback),
            Rc::clone(&self.reducer),
            Rc::clone(&self.meta),
        );
        let drivers = relay_pool::spawn_drivers(&self.meta.borrow().relay_bootstrap, handlers)?;
        *self.relays.borrow_mut() = drivers;
        Ok(())
    }

    /// V-01 Stage 3b — install a signer from a [`SetSigner`] request.
    ///
    /// Pure: no I/O, no JS-event-loop interaction. Construction failure
    /// surfaces as `CapabilityFailure` with a stable code (e.g.
    /// `unsupported_signer_kind`, `invalid_signer_pubkey`); success
    /// surfaces as `ActionAccepted` with `action_type = "nmp.set_signer"`
    /// so the host can resolve a spinner the same way it does for any
    /// other dispatched action.
    fn set_signer(&mut self, request: SetSigner) -> Vec<WorkerEvent> {
        match signer_slot::install_from_request(&request) {
            Ok(signer) => {
                self.signer = Some(signer);
                vec![WorkerEvent::ActionAccepted {
                    action_type: "nmp.set_signer".to_string(),
                    correlation_id: request.correlation_id,
                }]
            }
            Err(error) => vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: "nmp.set_signer".to_string(),
                correlation_id: request.correlation_id,
                reason: error.detail(),
            })],
        }
    }

    fn app_action(
        &mut self,
        action: AppAction,
        correlation_id: String,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        let (action_type, _payload) = action.into_dispatch_parts();
        Ok(vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
            capability: action_type,
            correlation_id,
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })])
    }

    fn dispatch(&mut self, action: ActionDispatch) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // ── F-CR-00 claim arm ─────────────────────────────────────────────────
        //
        // Claim/release operations are NOT `KernelAction`s — they live on the
        // `KernelReducer` surface directly. Check for them BEFORE the
        // `kernel_action_from_dispatch` arm so they do not fall through to the
        // write-path-unavailable path.
        //
        // `can_send` mirrors the native `claim_send_gate` semantics: true when
        // any relay lane has reported `Connected` to `handle_relay_connected`.
        // Using `KernelReducer::any_relay_connected` rather than driver socket
        // state avoids the lost-fetch trap (driver `current_socket.is_some()`
        // fires at dial time, before the kernel learns of `Connected`; the REQ
        // would be emitted then dropped with no re-queue — see `claim_send_gate`
        // comment in `actor/relay_mgmt.rs`).
        //
        // The returned `Vec<OutboundMessage>` is already `partition_auth_paused`
        // inside the `KernelReducer` methods; we fan it out here rather than
        // in the kernel so the wasm relay-driver pool sees it. Release calls
        // always return empty vecs and the fan-out becomes a no-op.
        //
        // Synchronous — claims need no async signer Promise path.
        if let Some(claim) = claim_dispatch_from_action(&action) {
            let can_send = self.reducer.borrow().any_relay_connected();
            let outbound = {
                let mut r = self.reducer.borrow_mut();
                match claim {
                    ClaimDispatch::ClaimProfile { pubkey, consumer_id } => {
                        r.claim_profile(pubkey, consumer_id, can_send)
                    }
                    ClaimDispatch::ReleaseProfile { pubkey, consumer_id } => {
                        r.release_profile(&pubkey, &consumer_id)
                    }
                    ClaimDispatch::ClaimEvent { uri, consumer_id } => {
                        r.claim_event(uri, consumer_id, can_send)
                    }
                    ClaimDispatch::ReleaseEvent { uri, consumer_id } => {
                        r.release_event(&uri, &consumer_id)
                    }
                }
            };
            // Fan the outbound REQ frames to live relay drivers (wasm32 only).
            // On native targets `fan_out_outbound` is a no-op shim.
            #[cfg(target_arch = "wasm32")]
            crate::publish_path::fan_out_outbound(&self.relays, &outbound);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = outbound;
            return Ok(vec![
                WorkerEvent::ActionAccepted {
                    action_type: action.action_type,
                    correlation_id: action.correlation_id,
                },
                self.snapshot_event(),
            ]);
        }

        // Generic `ActionDispatch` covers everything `AppAction` does plus the
        // kernel-namespaced actions (`nmp.open_uri`, `nmp.kernel.diagnostics`,
        // ...) the bible specifies. The kernel-namespaced ones map directly to
        // `KernelAction` variants and run through `KernelReducer::reduce`; the
        // app-namespaced ones (anything that emits a signed event) hit the
        // same write-path-unavailable wall as `app_action` above.
        if let Some(kernel_action) = kernel_action_from_dispatch(&action) {
            let update = self.reducer.borrow_mut().reduce(kernel_action);
            match update {
                KernelUpdate::Started { rev } => {
                    let mut meta = self.meta.borrow_mut();
                    meta.rev = rev;
                    meta.started = true;
                }
                KernelUpdate::Stopped { rev } => {
                    let mut meta = self.meta.borrow_mut();
                    meta.rev = rev;
                    meta.started = false;
                }
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
            reason: write_path_unavailable_reason(self.signer.as_ref()),
        })])
    }

    /// Build a binary `WorkerEvent::UpdateBytes` from the current kernel +
    /// meta state. The legacy JSON snapshot builder remains only for native
    /// tests and non-update diagnostics; runtime snapshot transport is bytes.
    fn snapshot_event(&self) -> WorkerEvent {
        let bytes = build_snapshot_bytes(&self.reducer.borrow(), &self.meta.borrow());
        WorkerEvent::UpdateBytes { bytes }
    }

    /// V-51 phase 2 — JSON snapshot of the kernel's recent routing
    /// decisions. Sibling of the FFI `nmp_app_recent_routing_decisions`
    /// symbol; same payload shape on both surfaces so the web Chirp shell
    /// and the iOS Chirp shell can share a single routing-inspector
    /// renderer (V-51 phase 3).
    ///
    /// Pull-only: the runtime does not push this on every snapshot tick
    /// (routing traces are diagnostic; the cost model is "pay when a host
    /// asks"). The `wasm-bindgen` wrapper exposes this as
    /// `NmpWasmRuntime::recent_routing_decisions()`.
    #[must_use]
    pub fn recent_routing_decisions(&self) -> String {
        self.reducer.borrow().recent_routing_decisions_json()
    }

    /// Build the inner snapshot `v` payload. Used by tests that want to
    /// inspect the snapshot without unwrapping the envelope.
    #[cfg(test)]
    pub(crate) fn snapshot_value(&self) -> serde_json::Value {
        crate::snapshot::build_snapshot_value(&self.reducer.borrow(), &self.meta.borrow())
    }

    /// V-01 Stage 3c — start an async publish for an `AppAction`. Wasm32-only.
    ///
    /// Returns a [`std::future::Future`] resolving to the [`WorkerEvent`] the
    /// host should observe — `ActionAccepted` if the sign + publish succeeded,
    /// `CapabilityFailure` for every honest failure mode (no signer, wrong
    /// backend, unsupported action variant, sign rejected, sign failed).
    ///
    /// Lifetime / borrow contract: this method snapshots the runtime's `Rc`
    /// handles up-front (signer, reducer, drivers, snapshot_callback, meta)
    /// and the returned future owns those clones — no reference into `self`
    /// outlives the call. That lets the `NmpWasmRuntime` wasm-bindgen wrapper
    /// hand the future to `wasm_bindgen_futures::future_to_promise(...)` and
    /// the Promise can outlive any particular `&mut self` borrow window.
    ///
    /// `now_secs` is supplied by the wasm bindings layer (which sources it
    /// from `js_sys::Date::now() / 1000.0`) so the kernel's internal clock
    /// (which is `pub(crate)` on the native side and not reachable through
    /// `KernelReducer`) is bypassed. Production correctness is unaffected —
    /// the publish engine treats `created_at` as a per-event field, not a
    /// scheduling input.
    #[cfg(target_arch = "wasm32")]
    pub fn start_publish_app_action(
        &self,
        action: AppAction,
        correlation_id: String,
        now_secs: u64,
    ) -> impl std::future::Future<Output = WorkerEvent> + 'static {
        let signer_slot = self.signer.clone();
        let reducer = Rc::clone(&self.reducer);
        let drivers = Rc::clone(&self.relays);
        let snapshot_callback = Rc::clone(&self.snapshot_callback);
        let meta = Rc::clone(&self.meta);
        async move {
            let Some(signer) = signer_slot else {
                let (action_type, _) = action.into_dispatch_parts();
                return WorkerEvent::CapabilityFailure(CapabilityFailure {
                    capability: action_type,
                    correlation_id,
                    reason: crate::dispatch_routing::write_path_unavailable_reason(None),
                });
            };
            crate::publish_path::publish_app_action(
                action,
                correlation_id,
                signer,
                reducer,
                drivers,
                snapshot_callback,
                meta,
                now_secs,
            )
            .await
        }
    }
}

fn relay_bootstrap_from_config(
    relays: Vec<String>,
    relay_bootstrap: Vec<crate::protocol::RelayBootstrapEntry>,
) -> Vec<crate::protocol::RelayBootstrapEntry> {
    if !relay_bootstrap.is_empty() {
        return relay_bootstrap;
    }
    relays
        .into_iter()
        .map(|url| crate::protocol::RelayBootstrapEntry {
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
    /// always yields `Started`).
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
