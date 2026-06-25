//! Internal ABI adapter (`RawWasmAbiAdapter`) backed by `KernelReducer`, the
//! wasm32 relay pool, and the snapshot-callback push channel. This is NOT a
//! composition API — do not use directly. The browser runtime is
//! `nmp-browser-runtime` (Wave 3).
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
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use nmp_core::slots::PublishResolverFactory;
use nmp_core::substrate::ActionModule;
use nmp_core::{ActionRegistry, KernelReducer};

#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};

use crate::dispatch_routing::browser_driver_missing_reason;
use crate::protocol::{CapabilityFailure, RuntimeStatus, WorkerEvent, WorkerRequest};
use crate::snapshot::RuntimeMeta;
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

/// Internal ABI adapter backed by a real `KernelReducer` plus the
/// snapshot-callback push channel. Not a composition API — do not use
/// directly. The browser runtime is `nmp-browser-runtime`.
///
/// `Default::default()` constructs the reducer eagerly — the kernel is cheap
/// to allocate (no I/O, no threads) and constructing it lazily would complicate
/// the snapshot path that runs before `Start` arrives.
pub struct RawWasmAbiAdapter {
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
    /// App composition publish-resolver factory.
    ///
    /// `nmp-wasm` owns browser transport and protocol framing, not relay
    /// routing policy. App composition roots that can depend on `nmp-router`
    /// install the same resolver factory native uses; `Start` invokes it
    /// against the fresh kernel slots after store/configured-relay setup.
    publish_resolver_factory: Rc<RefCell<Option<Arc<PublishResolverFactory>>>>,
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
    /// NIP-07 publish continuations parked between `SignRequest` and
    /// `DeliverSignerResponse`.
    pending_signed_publishes: HashMap<String, PendingSignedPublish>,
    /// Boot-time composition hooks that must observe the final event-store
    /// handle, after store injection and before relay drivers start.
    before_start_hooks: Vec<Box<dyn FnOnce(&mut RawWasmAbiAdapter)>>,
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

#[derive(Clone, Debug)]
struct PendingSignedPublish {
    action_namespace: String,
    action_correlation_id: String,
    target: nmp_core::publish::PublishTarget,
}

impl RawWasmAbiAdapter {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the post-event drain hook — fires AFTER the scheduler's reducer
    /// maintenance borrow is released, so the drain can safely call
    /// `reducer.borrow_mut()`. Internal API; composition lives in
    /// nmp-browser-runtime.
    pub(crate) fn install_post_tick_drain(&self, drain: Rc<dyn Fn()>) {
        *self.post_tick_drain.borrow_mut() = Some(drain);
    }

    /// Install a post-encode frame observer (issue #1767).
    ///
    /// Internal API; composition lives in nmp-browser-runtime.
    pub(crate) fn install_frame_observer(&self, observer: Rc<dyn Fn(&[u8])>) {
        self.meta.borrow_mut().frame_observer = Some(observer);
    }

    /// Return an `Rc` clone of the reducer for internal use only.
    /// Internal API; composition lives in nmp-browser-runtime.
    #[must_use]
    pub(crate) fn reducer_handle(&self) -> Rc<RefCell<KernelReducer>> {
        Rc::clone(&self.reducer)
    }

    /// Install an externally-opened event store to be consumed by the next
    /// `Start`. Internal API; composition lives in nmp-browser-runtime.
    ///
    /// This is intentionally a boot-time seam. The store backend must already
    /// be open and synchronous; the kernel's [`EventStore`](nmp_store::EventStore)
    /// contract remains unchanged. Calling after `Start` is rejected because
    /// existing relay, publish, projection, and query handles would otherwise
    /// split across old and new stores.
    pub(crate) fn set_injected_store(
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

    /// Install the app composition publish resolver factory. Internal API;
    /// composition lives in nmp-browser-runtime.
    ///
    /// The factory receives the kernel-owned event store, indexer relay slot,
    /// local-write relay slot, and active-account slot every time `Start`
    /// installs a resolver. This mirrors the native `NmpApp` composition seam
    /// without making `nmp-wasm` depend on `nmp-router`.
    pub(crate) fn set_publish_resolver_factory<F>(&mut self, factory: F) -> Result<(), WasmRuntimeError>
    where
        F: Fn(
                Arc<dyn nmp_store::EventStore>,
                nmp_core::slots::IndexerRelaysSlot,
                nmp_core::slots::LocalWriteRelaysSlot,
                nmp_core::slots::ActiveAccountSlot,
            ) -> Arc<dyn nmp_core::publish::OutboxResolver>
            + Send
            + Sync
            + 'static,
    {
        if self.meta.borrow().started {
            return Err(WasmRuntimeError::InvalidConfig(
                "publish resolver factory must be installed before Start".to_string(),
            ));
        }
        *self.publish_resolver_factory.borrow_mut() = Some(Arc::new(factory));
        Ok(())
    }

    /// Register a boot-time composition hook. Internal API; composition lives
    /// in nmp-browser-runtime.
    ///
    /// Hooks run at the top of `Start`, after any injected store has rebuilt
    /// the reducer and before relay drivers, publish routing, or app
    /// projections capture reducer/store handles. This keeps ADR-0054 store
    /// injection honest: app composition that needs `event_store_handle()` must
    /// observe the final backend, not the default in-memory store.
    pub(crate) fn install_before_start_hook(
        &mut self,
        hook: impl FnOnce(&mut RawWasmAbiAdapter) + 'static,
    ) -> Result<(), WasmRuntimeError> {
        if self.meta.borrow().started {
            return Err(WasmRuntimeError::InvalidConfig(
                "before-start hooks must be installed before Start".to_string(),
            ));
        }
        self.before_start_hooks.push(Box::new(hook));
        Ok(())
    }

    /// Register a typed [`ActionModule`] under its `NAMESPACE` into the runtime's
    /// action registry. Internal API; composition lives in nmp-browser-runtime.
    ///
    /// The composition root calls this — directly or through the per-NIP
    /// `register_actions(&mut impl ActionRegistrar)` entry points — to populate
    /// the non-publish write namespaces (NIP-02 follow / unfollow / follow_many,
    /// NIP-25 react / unreact). A typed payload dispatched through `dispatch_bytes`
    /// for a registered namespace then reaches the module's typed `start_bytes`
    /// decode (S3 / #1751) instead of the generic write-path `CapabilityFailure`.
    pub(crate) fn register_action<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        // Structured collision detection (#1724): the wasm path has no tracing;
        // propagate the Result so trait-boundary callers can observe it.
        self.action_registry.register(module)
    }

    /// Register a typed [`ActionModule`] **only if** its namespace is not
    /// already claimed (returns `true` on first registration). Internal API;
    /// composition lives in nmp-browser-runtime.
    pub(crate) fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        self.action_registry.register_default(module)
    }

    /// Install (or clear, with `None`) the snapshot push callback. Internal API.
    /// Wasm32 only — native targets have no `js_sys::Function` to install.
    ///
    /// Calling this with `Some(f)` replaces any previously-installed
    /// callback atomically (the slot is swapped under a single `RefMut`
    /// borrow). Calling with `None` clears the slot; subsequent relay
    /// frames will not push, and the host falls back to pull-by-dispatch.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn set_snapshot_callback(&mut self, callback: Option<js_sys::Function>) {
        *self.snapshot_callback.borrow_mut() = callback;
    }

    /// Return a shared reference to the snapshot-callback slot. Internal API.
    ///
    /// The slot is `Rc<RefCell>` so cloning it gives a shared handle with
    /// zero-copy semantics.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn snapshot_callback_handle(&self) -> &Rc<RefCell<Option<js_sys::Function>>> {
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
}

// Routing diagnostics. Production code (all targets); the methods are public
// app-facing API on `WasmRuntime`.
#[path = "runtime/diagnostics.rs"]
mod diagnostics;

// Feed-declaration helpers. Production code (all targets); the methods are
// public app-facing API on `WasmRuntime`.
#[path = "runtime/feed.rs"]
mod feed;

// Runtime lifecycle and scheduler helpers. Production code (all targets).
#[path = "runtime/lifecycle.rs"]
mod lifecycle;

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
