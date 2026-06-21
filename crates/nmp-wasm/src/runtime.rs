//! Browser-side runtime (`WasmRuntime`) backed by `KernelReducer`, the
//! wasm32 relay pool, the signer slot, and the snapshot-callback push channel.
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
//! - **(wasm32, feature = "wasm" in nmp-signers)** `Nip07Signer::sign()`
//!   bridges into `window.nostr.signEvent(...)` via `spawn_local`.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use nmp_core::{KernelAction, KernelReducer, KernelUpdate, OutboundMessage};
use nmp_signers::Signer;

#[cfg(target_arch = "wasm32")]
use crate::relay_pool;
#[cfg(target_arch = "wasm32")]
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};

use crate::dispatch_routing::browser_driver_missing_reason;
use crate::protocol::{
    CapabilityFailure, RuntimeStatus, SetSigner, StartConfig, WorkerEvent, WorkerRequest,
};
// `AppAction` is only named as a type by the wasm32-only async-publish path
// (`start_publish_app_action`); the native `WorkerRequest::AppAction(_)` arm
// destructures the enum variant without naming the inner type.
#[cfg(target_arch = "wasm32")]
use crate::protocol::AppAction;
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
    /// PR-4 post-tick drain — fired AFTER `tick_once`'s `borrow_mut` drops.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    post_tick_drain: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
    /// ADR-0058 seq-ordered PULL scrolling — the host-owned feed registry, the
    /// wasm twin of `NmpApp::feed_registry`. The composition root registers a
    /// `PullFeedController` per feed key (e.g. `nmp.feed.home`) via
    /// [`Self::register_feed`]; a `LoadOlderFeed` request drains one older page
    /// through it (see [`Self::load_older_feed`]).
    feed_registry: nmp_feed::FeedRegistrySlot,
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
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self {
            reducer: Rc::new(RefCell::new(KernelReducer::new())),
            meta: Rc::new(RefCell::new(RuntimeMeta::new())),
            signer: None,
            snapshot_callback: Rc::new(RefCell::new(None)),
            post_tick_drain: Rc::new(RefCell::new(None)),
            feed_registry: nmp_feed::new_feed_registry_slot(),
            #[cfg(target_arch = "wasm32")]
            relays: Rc::new(RefCell::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            handlers_slot: Rc::new(RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            tick_interval: None,
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

    /// Register a reusable feed surface (ADR-0058). The wasm twin of
    /// `NmpApp::register_feed`: the controller owns ordering, viewport state,
    /// paging, and render-payload selection; the shell only renders the emitted
    /// projection and reports tail-reached via [`WorkerRequest::LoadOlderFeed`].
    ///
    /// Called by the composition root (`nmp_app_chirp_web`) to register the
    /// home-feed `PullFeedController` under `nmp.feed.home`. (The `LoadOlderFeed`
    /// drain handler lives in the sibling `runtime/dispatch.rs` LOC seam.)
    pub fn register_feed(
        &self,
        key: impl Into<String>,
        controller: Arc<dyn nmp_feed::FeedController>,
    ) {
        self.feed_registry.register(key, controller);
    }

    /// Return an `Rc` clone of the reducer for composition-root closures.
    #[must_use]
    pub fn reducer_handle(&self) -> Rc<RefCell<KernelReducer>> {
        Rc::clone(&self.reducer)
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
            WorkerRequest::LoadOlderFeed {
                feed_key,
                correlation_id,
            } => Ok(self.load_older_feed(&feed_key, correlation_id)),
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

    /// V-01 Stage 3b — install a signer from a [`SetSigner`] request.
    ///
    /// Pure: no I/O, no JS-event-loop interaction. Construction failure
    /// surfaces as `CapabilityFailure` with a stable code (e.g.
    /// `unsupported_signer_kind`, `invalid_signer_pubkey`); success
    /// surfaces as `ActionAccepted` with `action_type = "nmp.set_signer"`
    /// so the host can resolve a spinner the same way it does for any
    /// other dispatched action.
    ///
    /// PR-3 viewer-pubkey hand-off: on success the pubkey from the signer
    /// request is fed into the kernel via `set_active_account` so
    /// contact-feed resolution and bootstrap interests know whose follows
    /// to load without waiting for a separate `set_active_account` action.
    fn set_signer(&mut self, request: SetSigner) -> Vec<WorkerEvent> {
        match signer_slot::install_from_request(&request) {
            Ok((signer, canonical_pubkey)) => {
                self.signer = Some(signer);
                // Use the canonical (lowercase) hex from the parsed key, not
                // the raw wire string — guards against uppercase input that
                // would seed a non-canonical active_account (B2).
                let outbound =
                    self.reducer.borrow_mut().set_active_account(canonical_pubkey);
                self.fan_outbound(outbound);
                self.accepted_with_snapshot(
                    "nmp.set_signer".to_string(),
                    request.correlation_id,
                )
            }
            Err(error) => vec![WorkerEvent::CapabilityFailure(CapabilityFailure {
                capability: "nmp.set_signer".to_string(),
                correlation_id: request.correlation_id,
                reason: error.detail(),
            })],
        }
    }

    /// Fan `outbound` messages to live relay drivers (wasm32) or drop them
    /// (native — the driver pool does not exist in test builds).
    fn fan_outbound(&self, outbound: Vec<OutboundMessage>) {
        #[cfg(target_arch = "wasm32")]
        crate::relay_pool::fan_out_outbound(&self.relays, &self.handlers_slot, &outbound);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = outbound;
    }

    // `accepted_with_snapshot`, `app_action`, and `dispatch` — the
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

impl std::error::Error for WasmRuntimeError {}
