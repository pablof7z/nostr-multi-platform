//! Browser-side runtime (`WasmRuntime`) backed by `KernelReducer`, the
//! wasm32 relay pool, and the snapshot-callback push channel.
//!
//! # Current capabilities
//!
//! - `Start` / `Stop` are runtime lifecycle requests and produce
//!   runtime-status replies.
//! - `DispatchBytes` routes typed app writes through `DispatchEnvelope` bytes.
//! - `ResolveRef` / `ReleaseRef` route structured reference controls without
//!   reopening the retired JSON action-dispatch surface.
//! - Snapshot updates are produced as FlatBuffers `UpdateFrame` bytes.
//! - **(wasm32)** Relay sockets dial on `Start`, reconnect with the same
//!   exponential backoff + jitter constants the native worker uses, ingest
//!   frames into the kernel, route outbound back to the wire, and push a
//!   fresh snapshot to the JS host through the registered callback (if any).
//! - **(wasm32)** Signed writes use the ADR-0050 capability round-trip
//!   (`BeginSign` → `SignRequest` → `DeliverSignerResponse`): the main-thread
//!   broker calls `window.nostr.signEvent(...)` and re-enters the worker with
//!   the result. The reducer never awaits the world (D7/D8).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use nmp_core::substrate::ActionModule;
use nmp_core::{ActionRegistry, KernelReducer, OutboundMessage};

#[cfg(target_arch = "wasm32")]
use crate::relay_pool;
#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};

use crate::dispatch_routing::browser_driver_missing_reason;
use crate::protocol::{CapabilityFailure, RuntimeStatus, StartConfig, WorkerEvent, WorkerRequest};
use crate::snapshot::{build_snapshot_bytes, RuntimeMeta};
pub use error::WasmRuntimeError;

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

/// Browser-side runtime backed by a real `KernelReducer` plus the
/// snapshot-callback push channel.
///
/// `Default::default()` constructs the reducer eagerly — the kernel is cheap
/// to allocate (no I/O, no threads) and constructing it lazily would complicate
/// the snapshot path that runs before `Start` arrives.
pub struct WasmRuntime {
    /// Pure protocol kernel — the same reducer the native actor loop uses.
    /// Held behind `Rc<RefCell>` so the wasm32 relay-driver closures can
    /// share it without unsafe lifetime gymnastics.
    reducer: Rc<RefCell<KernelReducer>>,
    /// ADR-0054 Stage #5 — boot-time event-store injection slot.
    ///
    /// Future web persistence opens its platform backend before `Start`, then
    /// installs the ready synchronous [`EventStore`](nmp_store::EventStore)
    /// here. `Start` consumes this once and rebuilds the reducer before relay
    /// drivers/deadlines capture it. Empty means the default in-memory reducer
    /// stays in place.
    injected_store: Rc<RefCell<Option<Arc<dyn nmp_store::EventStore>>>>,
    /// Runtime metadata mirrored into every snapshot update. Shared with
    /// the relay-pool sink via `Rc<RefCell>` so the sink can build a fresh
    /// snapshot from kernel + meta without holding a reference to the
    /// runtime itself (which the sink, captured by JS event handlers,
    /// cannot).
    meta: Rc<RefCell<RuntimeMeta>>,
    /// V-01 Stage 3b — snapshot push callback. Wasm32 stores the JS
    /// `Function`; native carries `()`. The relay-pool sink reads this slot
    /// after every kernel-mutating inbound frame and pushes a fresh snapshot
    /// if a callback is installed. Unused on native (no JS to call into;
    /// `set_snapshot_callback` is a no-op shim), so silence the dead-code
    /// warning the symmetric struct layout otherwise triggers there.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    snapshot_callback: Rc<RefCell<Option<SnapshotCallback>>>,
    /// PR-4 post-event drain — fired AFTER the reducer maintenance borrow
    /// drops. Kept source-compatible with composition roots that installed the
    /// old post-tick hook; the production scheduler now invokes it from event
    /// and deadline wakes, not from a fixed interval.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    post_tick_drain: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    /// Live `web_sys::WebSocket` drivers — one per relay URL. Seeded from the
    /// bootstrap at `Start`, then grown on demand: a kernel frame targeting a
    /// not-yet-open URL spawns a driver via `fan_out_outbound`. `wasm32`-only:
    /// native tests never construct drivers.
    #[cfg(target_arch = "wasm32")]
    relays: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    /// The kernel-handler callback bag, shared back into the relay-pool fan-out
    /// so on-demand driver spawns wire up identical kernel callbacks. Populated
    /// in `spawn_relay_drivers` after `build_handlers` returns (the closures
    /// capture this slot while it is still empty — safe per the `spawn_drivers`
    /// ordering invariant) and cleared on `Stop`. `wasm32`-only.
    #[cfg(target_arch = "wasm32")]
    handlers_slot: Rc<RefCell<Option<BrowserKernelHandlers>>>,
    /// Event/deadline-driven runtime scheduler. Wasm32 stores a one-shot
    /// `setTimeout` handle inside the scheduler; native builds keep the same
    /// state for deterministic tests.
    maintenance_deadline: Rc<RefCell<crate::tick::RuntimeDeadline>>,
    /// ADR-0064 / S3 (#1751) — the typed action registry. The wasm twin of
    /// `NmpApp::action_registry`: it owns the per-namespace `ActionModule`
    /// values whose `start_bytes` runs the typed FlatBuffers `decode_payload`
    /// + the fail-closed `schema_version` gate before `start()`. Seeded with
    /// `default_registry()` (the kernel `PublishModule`, exactly like native);
    /// the composition root (`nmp-app-chirp-web`, which CAN depend on the NIP
    /// crates — `nmp-wasm` cannot, D0/layering) registers the NIP-02/NIP-25
    /// write modules through [`WasmRuntime::register_action`] /
    /// [`WasmRuntime::register_default_action`], mirroring the per-NIP
    /// `register_actions` entry points the native FFI app calls.
    action_registry: ActionRegistry,
}

impl WasmRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the post-event drain hook — fires AFTER the scheduler's reducer
    /// maintenance borrow is released, so the drain can safely call
    /// `reducer.borrow_mut()`. Subsequent calls replace the prior drain.
    pub fn install_post_tick_drain(&self, drain: Rc<dyn Fn()>) {
        *self.post_tick_drain.borrow_mut() = Some(drain);
    }

    /// Install a post-encode frame observer (issue #1767).
    ///
    /// The closure is invoked from `build_snapshot_bytes` with the
    /// just-encoded FlatBuffers frame bytes, on EVERY snapshot path
    /// (deadline wake, relay-push, synchronous `handle` return, publish
    /// fan-out) — because that builder is the single chokepoint all of them
    /// funnel through. The
    /// callback runs AFTER `make_update_frame` returns and receives only
    /// `&[u8]`, so it never re-enters the reducer.
    ///
    /// This is the wasm twin of nmp-ffi's listener-thread
    /// `update_embed_sidecar_from_frame` hook. A composition root that depends
    /// on `nmp-content` (e.g. `nmp-app-chirp-web`) installs an observer that
    /// decodes the `claimed_events` KCEV from the bytes, resolves each embed,
    /// and stores the resolved map in its own slot — keeping `nmp-wasm` itself
    /// policy-free (it owns the chokepoint, not the kind-dispatch).
    ///
    /// Subsequent calls replace the prior observer.
    pub fn install_frame_observer(&self, observer: Rc<dyn Fn(&[u8])>) {
        self.meta.borrow_mut().frame_observer = Some(observer);
    }

    /// Return an `Rc` clone of the reducer for composition-root closures.
    #[must_use]
    pub fn reducer_handle(&self) -> Rc<RefCell<KernelReducer>> {
        Rc::clone(&self.reducer)
    }

    /// Install an externally-opened event store to be consumed by the next
    /// `Start`.
    ///
    /// This is intentionally a boot-time seam. The store backend must already
    /// be open and synchronous; the kernel's [`EventStore`](nmp_store::EventStore)
    /// contract remains unchanged. Calling after `Start` is rejected because
    /// existing relay, publish, projection, and query handles would otherwise
    /// split across old and new stores.
    pub fn set_injected_store(
        &mut self,
        store: Arc<dyn nmp_store::EventStore>,
    ) -> Result<(), WasmRuntimeError> {
        if self.meta.borrow().started {
            return Err(WasmRuntimeError::InvalidConfig(
                "event store injection must happen before Start".to_string(),
            ));
        }
        *self.injected_store.borrow_mut() = Some(store);
        Ok(())
    }

    /// Register a typed [`ActionModule`] under its `NAMESPACE` into the runtime's
    /// action registry. The wasm twin of `NmpApp::register_action`.
    ///
    /// The composition root (`nmp-app-chirp-web`) calls this — directly or
    /// through the per-NIP `register_actions(&mut impl ActionRegistrar)` entry
    /// points — to populate the non-publish write namespaces (NIP-02 follow /
    /// unfollow / follow_many, NIP-25 react / unreact). A typed payload
    /// dispatched through `dispatch_bytes` for a registered namespace then
    /// reaches the module's typed `start_bytes` decode (S3 / #1751) instead of
    /// the generic write-path `CapabilityFailure`. `nmp-wasm` itself cannot
    /// depend on the NIP crates (D0 / layering), so registration is the
    /// composition root's job — exactly as the native FFI app delegates to each
    /// crate's `register_actions`.
    pub fn register_action<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        // Structured collision detection (#1724): the wasm path has no tracing;
        // propagate the Result so trait-boundary callers can observe it.
        self.action_registry.register(module)
    }

    /// Register a typed [`ActionModule`] **only if** its namespace is not
    /// already claimed (returns `true` on first registration). The wasm twin of
    /// `NmpApp::register_default_action`; the per-NIP `register_actions`
    /// helpers call this through the [`ActionRegistrar`] impl below.
    pub fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        self.action_registry.register_default(module)
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

    /// Return a shared reference to the snapshot-callback slot.
    ///
    /// Used by composition-root crates (`nmp-app-chirp-web`) that own the
    /// `#[wasm_bindgen]` entry point and need to route `UpdateBytes` through
    /// the same callback channel as `handle_json`. The slot is `Rc<RefCell>`
    /// so cloning it gives a shared handle with zero-copy semantics.
    #[cfg(target_arch = "wasm32")]
    pub fn snapshot_callback_handle(&self) -> &Rc<RefCell<Option<js_sys::Function>>> {
        &self.snapshot_callback
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
            WorkerRequest::ResolveRef(request) => self.resolve_ref(request),
            WorkerRequest::ReleaseRef(request) => self.release_ref(request),
            // ADR-0064 / S2 — the one binary write doorway. Decodes the
            // `DispatchEnvelope` and routes by `action_namespace` (same open
            // transport as the native FFI). Total — never returns `Err`.
            WorkerRequest::DispatchBytes(request) => Ok(self.dispatch_bytes(&request.bytes)),
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
            WorkerRequest::SetIdentity(request) => Ok(self.set_identity(request)),
            // #1753 S6 — the wasm signing capability round-trip (pure message
            // re-entry). `begin_sign` parks an op + emits the broker request;
            // `deliver_signer_response` drives the parked op once from this
            // message handler — no polling, no tick-dependence (D8). Both
            // delegate to the target-agnostic `KernelReducer` seam.
            WorkerRequest::BeginSign(request) => Ok(self.begin_sign(request)),
            WorkerRequest::DeliverSignerResponse(response) => {
                Ok(self.deliver_signer_response(response))
            }
            WorkerRequest::Stop { correlation_id } => self.stop(correlation_id),
        }
    }

    // `begin_sign` / `deliver_signer_response` — the #1753 S6 wasm signing
    // capability round-trip arms — live in the sibling `runtime/signer.rs`
    // module (LOC ceiling). They are still private methods on `WasmRuntime`.

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

        if let Some(store) = self.injected_store.borrow_mut().take() {
            self.reducer.borrow_mut().replace_store_for_start(store);
        }

        let relay_bootstrap =
            crate::protocol::relay_bootstrap_from_config(config.relays, config.relay_bootstrap);

        // Seed the kernel's configured-relay lanes so `make_update_frame`
        // emits real relay-status rows and the `configured_relays` typed
        // projection. Must run before the first snapshot (below) and before
        // spawning drivers (which read the same rows for their URLs).
        self.reducer.borrow_mut().set_configured_relays(
            relay_bootstrap
                .iter()
                .map(|e| (e.url.clone(), e.role.clone()))
                .collect(),
        );

        // #1008 — install the real OutboxResolver so publish actions reach the
        // wire. The resolver returns the configured relay URLs as
        // `LocalConfigRelay` targets, replacing the default
        // `NoopOutboxResolver` that resolved zero targets for every
        // `PublishTarget::Auto` and silently dropped every publish.
        {
            let relay_urls: Vec<String> = relay_bootstrap.iter().map(|e| e.url.clone()).collect();
            let resolver = crate::publish_path::build_wasm_outbox_resolver(relay_urls);
            self.reducer.borrow_mut().set_publish_resolver(resolver);
        }

        {
            let mut meta = self.meta.borrow_mut();
            meta.started = true;
            meta.relay_bootstrap = relay_bootstrap;
            meta.database_name = config.database_name;
        }

        // V-01 Stage 3 / #1937 — spawn relay drivers and arm one post-start
        // deadline wake. Native builds skip drivers; tests can fire the same
        // scheduler state explicitly.
        #[cfg(target_arch = "wasm32")]
        {
            self.spawn_relay_drivers()?;
        }
        self.request_event_drain();

        Ok(vec![
            WorkerEvent::RuntimeStatus {
                status: RuntimeStatus::Running,
                correlation_id: Some(config.correlation_id),
            },
            self.snapshot_event(),
        ])
    }

    fn stop(&mut self, correlation_id: String) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // Cancel deadline before relay teardown; no in-flight wake against a
        // partially closed pool.
        crate::tick::cancel_deadline(&self.maintenance_deadline);
        // Tear down every live relay driver — closing the JS sockets and
        // dropping the parked closures so the user-agent reclaims them.
        // Order matters: close sockets before clearing runtime metadata, so
        // callbacks settle against the state they were started from.
        #[cfg(target_arch = "wasm32")]
        relay_pool::close_drivers(&self.relays);
        // Drop the handler bag so any late callback cannot spawn a driver into
        // a stopped pool, and a subsequent `Start` rebuilds it cleanly.
        #[cfg(target_arch = "wasm32")]
        {
            *self.handlers_slot.borrow_mut() = None;
        }

        {
            let mut meta = self.meta.borrow_mut();
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
            Rc::clone(&self.handlers_slot),
            Rc::clone(&self.maintenance_deadline),
            Rc::clone(&self.post_tick_drain),
        );
        // Publish the handler bag so on-demand spawns (`fan_out_outbound`'s
        // spawn-on-miss) wire up identical callbacks. The bag's closures
        // captured `handlers_slot` while it was empty; populating it now is
        // safe because no JS callback can fire until control returns to the
        // event loop, after the bootstrap drivers below are installed.
        *self.handlers_slot.borrow_mut() = Some(handlers.clone());
        let drivers = relay_pool::spawn_drivers(&self.meta.borrow().relay_bootstrap, handlers)?;
        *self.relays.borrow_mut() = drivers;
        Ok(())
    }

    /// Arm one runtime maintenance deadline.
    fn request_maintenance_deadline(&self, policy: crate::tick::WakePolicy) {
        #[cfg(target_arch = "wasm32")]
        crate::tick::request_runtime_deadline(
            Rc::clone(&self.maintenance_deadline),
            policy,
            Rc::clone(&self.reducer),
            Rc::clone(&self.relays),
            Rc::clone(&self.handlers_slot),
            Rc::clone(&self.snapshot_callback),
            Rc::clone(&self.meta),
            Rc::clone(&self.post_tick_drain),
        );
        #[cfg(not(target_arch = "wasm32"))]
        crate::tick::request_deadline_for_test(&self.maintenance_deadline, policy);
    }

    fn request_event_drain(&self) {
        self.request_maintenance_deadline(crate::tick::WakePolicy::Event);
    }

    fn request_event_or_kernel_deadline(&self) {
        self.request_maintenance_deadline(crate::tick::event_or_kernel_policy(&self.reducer));
    }

    /// Fan `outbound` messages to live relay drivers (wasm32) or drop them
    /// (native — the driver pool does not exist in test builds).
    fn fan_outbound(&self, outbound: Vec<OutboundMessage>) {
        let has_outbound = !outbound.is_empty();
        #[cfg(target_arch = "wasm32")]
        crate::relay_pool::fan_out_outbound(&self.relays, &self.handlers_slot, &outbound);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = outbound;
        if has_outbound {
            self.request_event_or_kernel_deadline();
        }
    }

    /// Build a binary `WorkerEvent::UpdateBytes` from the current kernel +
    /// meta state. Delegates to `build_snapshot_bytes` which calls
    /// `make_update_frame` — the kernel is the sole author of the encoded
    /// frame (rev, relay statuses, typed projections).
    fn snapshot_event(&mut self) -> WorkerEvent {
        let bytes = build_snapshot_bytes(&mut self.reducer.borrow_mut(), &self.meta.borrow());
        WorkerEvent::UpdateBytes { bytes }
    }
}

// Routing diagnostics. Production code (all targets); the methods are public
// app-facing API on `WasmRuntime`.
#[path = "runtime/diagnostics.rs"]
mod diagnostics;

// Feed-declaration helpers. Production code (all targets); the methods are
// public app-facing API on `WasmRuntime`.
#[path = "runtime/feed.rs"]
mod feed;

// Signer installation helper. Production code (all targets); split out for the
// LOC ceiling without changing the runtime surface.
#[path = "runtime/signer.rs"]
mod signer;

// Action-dispatch routing arm of `handle` — split out for the LOC ceiling.
// Production code (all targets); the methods are defined on `impl WasmRuntime`.
#[path = "runtime/dispatch.rs"]
mod dispatch;

#[cfg(not(target_arch = "wasm32"))]
#[path = "runtime/test_support.rs"]
mod test_support;

#[path = "runtime/actions.rs"]
mod actions;

#[path = "runtime/default.rs"]
mod default;

#[path = "runtime/error.rs"]
mod error;
