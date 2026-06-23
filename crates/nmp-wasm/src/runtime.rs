//! Browser-side runtime (`WasmRuntime`) backed by `KernelReducer`, the
//! wasm32 relay pool, and the snapshot-callback push channel.
//!
//! # Current capabilities
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
//! - **(wasm32)** Signed writes use the ADR-0050 capability round-trip
//!   (`BeginSign` → `SignRequest` → `DeliverSignerResponse`): the main-thread
//!   broker calls `window.nostr.signEvent(...)` and re-enters the worker with
//!   the result. The reducer never awaits the world (D7/D8).

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use nmp_core::substrate::{ActionModule, ActionRegistrar};
use nmp_core::{
    default_registry, ActionRegistry, KernelAction, KernelReducer, KernelUpdate, OutboundMessage,
};

#[cfg(target_arch = "wasm32")]
use crate::relay_pool;
#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};

use crate::dispatch_routing::browser_driver_missing_reason;
use crate::protocol::{
    CapabilityFailure, RuntimeStatus, StartConfig, WorkerEvent, WorkerRequest,
};
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
    /// PR-4 post-tick drain — fired AFTER `tick_once`'s `borrow_mut` drops.
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
    /// PR-2 — 1 Hz tick timer. Dropping cancels the JS `setInterval`.
    /// `start()` fills it; `stop()` clears it. `wasm32`-only.
    #[cfg(target_arch = "wasm32")]
    tick_interval: Option<gloo_timers::callback::Interval>,
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

impl Default for WasmRuntime {
    fn default() -> Self {
        Self {
            reducer: Rc::new(RefCell::new(KernelReducer::new())),
            meta: Rc::new(RefCell::new(RuntimeMeta::new())),
            snapshot_callback: Rc::new(RefCell::new(None)),
            post_tick_drain: Rc::new(RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            relays: Rc::new(RefCell::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            handlers_slot: Rc::new(RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            tick_interval: None,
            action_registry: default_registry(),
        }
    }
}

impl WasmRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the post-tick drain hook — fires AFTER `tick_once`'s
    /// `borrow_mut` is released, so the drain can safely call
    /// `reducer.borrow_mut()`. Subsequent calls replace the prior drain.
    pub fn install_post_tick_drain(&self, drain: Rc<dyn Fn()>) {
        *self.post_tick_drain.borrow_mut() = Some(drain);
    }

    /// Install a post-encode frame observer (issue #1767).
    ///
    /// The closure is invoked from `build_snapshot_bytes` with the
    /// just-encoded FlatBuffers frame bytes, on EVERY snapshot path (tick,
    /// relay-push, synchronous `handle` return, publish fan-out) — because
    /// that builder is the single chokepoint all of them funnel through. The
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
    pub fn register_action<M: ActionModule + 'static>(&mut self, module: M) {
        // Structured collision detection (#1724): log in both dev and release.
        // The wasm path does not have tracing — silently drop the error value;
        // the collision will appear as a last-writer-wins override.
        let _ = self.action_registry.register(module);
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
    pub fn snapshot_callback_handle(
        &self,
    ) -> &Rc<RefCell<Option<js_sys::Function>>> {
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
            WorkerRequest::Dispatch(action) => self.dispatch(action),
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

        // Drive the pure kernel through its `Start` action — same reducer
        // entry point `dispatch_kernel_action` calls on the native actor
        // thread, byte-for-byte.
        let started = self.reducer.borrow_mut().reduce(KernelAction::Start);
        match started {
            KernelUpdate::Started { .. } => {}
            other => {
                return Err(WasmRuntimeError::KernelContract(format!(
                    "expected Started after KernelAction::Start, got {other:?}"
                )));
            }
        };

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
            let relay_urls: Vec<String> =
                relay_bootstrap.iter().map(|e| e.url.clone()).collect();
            let resolver = crate::publish_path::build_wasm_outbox_resolver(relay_urls);
            self.reducer.borrow_mut().set_publish_resolver(resolver);
        }

        {
            let mut meta = self.meta.borrow_mut();
            meta.started = true;
            meta.relay_bootstrap = relay_bootstrap;
            meta.database_name = config.database_name;
        }

        // V-01 Stage 3 / PR-2 — spawn relay drivers and start the 1 Hz tick
        // timer on wasm32. Native builds skip both.
        #[cfg(target_arch = "wasm32")]
        {
            self.spawn_relay_drivers()?;
            self.tick_interval = Some(crate::tick::start_tick_interval(
                Rc::clone(&self.reducer),
                Rc::clone(&self.relays),
                Rc::clone(&self.handlers_slot),
                Rc::clone(&self.snapshot_callback),
                Rc::clone(&self.meta),
                Rc::clone(&self.post_tick_drain),
            ));
        }

        Ok(vec![
            WorkerEvent::RuntimeStatus {
                status: RuntimeStatus::Running,
                correlation_id: Some(config.correlation_id),
            },
            self.snapshot_event(),
        ])
    }

    fn stop(&mut self, correlation_id: String) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        // PR-2: cancel tick before relay teardown; no in-flight tick against partial pool.
        #[cfg(target_arch = "wasm32")]
        { self.tick_interval = None; }
        // Tear down every live relay driver — closing the JS sockets and
        // dropping the parked closures so the user-agent reclaims them.
        // Order matters: close sockets BEFORE driving the kernel `Stop`,
        // because the kernel's `Stop` arm resets the per-relay state we
        // want to settle observers on.
        #[cfg(target_arch = "wasm32")]
        relay_pool::close_drivers(&self.relays);
        // Drop the handler bag so any late callback cannot spawn a driver into
        // a stopped pool, and a subsequent `Start` rebuilds it cleanly.
        #[cfg(target_arch = "wasm32")]
        {
            *self.handlers_slot.borrow_mut() = None;
        }

        let stopped = self.reducer.borrow_mut().reduce(KernelAction::Stop);
        match stopped {
            KernelUpdate::Stopped { .. } => {}
            other => {
                return Err(WasmRuntimeError::KernelContract(format!(
                    "expected Stopped after KernelAction::Stop, got {other:?}"
                )));
            }
        };
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

    /// Fan `outbound` messages to live relay drivers (wasm32) or drop them
    /// (native — the driver pool does not exist in test builds).
    fn fan_outbound(&self, outbound: Vec<OutboundMessage>) {
        #[cfg(target_arch = "wasm32")]
        crate::relay_pool::fan_out_outbound(&self.relays, &self.handlers_slot, &outbound);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = outbound;
    }

    // `accepted_with_snapshot`, `dispatch_bytes`, and `dispatch` — the
    // action-namespace routing arm of `handle` — live in the sibling
    // `runtime/dispatch.rs` module (LOC ceiling). They are still private
    // methods of `WasmRuntime` (`impl WasmRuntime` in that file).

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

/// Lets the per-NIP `register_actions(&mut impl ActionRegistrar)` entry points
/// (e.g. `nmp_nip02::register_actions`, `nmp_nip25::register_actions`) register
/// straight into the runtime's typed action registry — the wasm twin of the
/// `impl ActionRegistrar for NmpApp` the native FFI app provides.
impl ActionRegistrar for WasmRuntime {
    fn register_action<M: ActionModule + 'static>(&mut self, module: M) {
        WasmRuntime::register_action(self, module);
    }

    fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        WasmRuntime::register_default_action(self, module)
    }
}

impl std::error::Error for WasmRuntimeError {}
