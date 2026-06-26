//! `BrowserRuntime` — single-writer reducer loop driven by explicit `pump()`
//! calls (issue #2046 / PR-B of the browser-runtime epic #2045).
//!
//! # Design (D4 single-writer, issue #2058)
//!
//! `BrowserRuntime` owns the `KernelReducer` and is the **sole** writer —
//! no other path mutates the kernel after `start()`. The JS host drives it
//! via `BrowserRuntimeHandle::pump()`, which drains the inbox and applies
//! every pending `ActorCommand`.
//!
//! `BrowserRuntimeHandle` is the **only** public type (#2058): it exposes
//! the narrow dispatch / snapshot surface and does NOT expose the raw
//! `KernelReducer` or `BrowserRuntime` fields.
//!
//! # Note on relay text/connected hooks
//!
//! On browser the relay pool is WebSocket-based and driven from JS land.
//! `relay_text_interceptors` and `relay_connected_hooks` are stored for
//! future integration with the browser relay driver (issue #2059).

use std::sync::{mpsc, Arc};

use nmp_core::actor::ActorMail;
use nmp_core::substrate::{
    RelayConnectedHook, RelayTextInterceptor, RoutedRelaySet, RoutingTraceObserver,
    PublishTrace, SubscriptionTrace,
};
use nmp_core::{
    ActionRegistry, AppRelaySlot, CommandSender, KernelReducer, OutboundMessage, UpdateFrameBytes,
};

use crate::builder::{BrowserRunConfig, BrowserBuilderInner};

// ── No-op routing trace observer ─────────────────────────────────────────────
//
// The browser path uses a no-op observer: the routing trace projection lives
// in the kernel's snapshot registry and is populated by the kernel itself;
// the external observer is only needed by the native FFI shell for diagnostics.

struct NoopRoutingTrace;

impl RoutingTraceObserver for NoopRoutingTrace {
    fn on_publish(&self, _summary: PublishTrace, _routed: &RoutedRelaySet) {}
    fn on_subscription(&self, _summary: SubscriptionTrace, _routed: &RoutedRelaySet) {}
}

impl NoopRoutingTrace {
    fn arc() -> Arc<dyn RoutingTraceObserver> {
        Arc::new(Self)
    }
}

mod pump;

// ── BrowserRuntime ────────────────────────────────────────────────────────────

/// Single-writer runtime owning the `KernelReducer`. Not public — callers
/// use `BrowserRuntimeHandle`.
struct BrowserRuntime {
    reducer: KernelReducer,
    /// Action registry kept for future use when browser-path action dispatch
    /// is wired (future #2057 follow-on). Currently unused since
    /// `KernelReducer::apply_actor_command` embeds action execution.
    #[allow(dead_code)]
    action_registry: ActionRegistry,
    inbox_rx: mpsc::Receiver<ActorMail>,
    // Stored for browser relay driver integration (future #2059).
    #[allow(dead_code)]
    relay_text_interceptors: Vec<Arc<dyn RelayTextInterceptor>>,
    #[allow(dead_code)]
    relay_connected_hooks: Vec<Arc<dyn RelayConnectedHook>>,
    /// Identity-change callbacks fired on account switch / logout.
    #[allow(dead_code)]
    identity_change_observers: Vec<Box<dyn Fn(Option<String>) + Send + Sync + 'static>>,
    #[allow(dead_code)]
    run_config: Option<BrowserRunConfig>,
}

impl BrowserRuntime {
    fn pump(&mut self) -> Vec<OutboundMessage> {
        pump::drain_inbox(&mut self.reducer, &self.inbox_rx)
    }

    fn make_update_frame(&mut self, running: bool) -> UpdateFrameBytes {
        self.reducer.make_update_frame(running)
    }
}

// ── BrowserRuntimeHandle ──────────────────────────────────────────────────────

/// Public-facing handle to the browser runtime (issue #2058 — hides raw
/// reducer/runtime handles).
///
/// Returned by `BrowserAppBuilder<ProvidersDecided>::start()`. Callers send
/// commands through a `CommandSender` (obtained from the builder BEFORE
/// `start()` via `actor_sender()`) and drive the event loop by calling
/// `pump()` on each timer tick.
pub struct BrowserRuntimeHandle {
    runtime: BrowserRuntime,
    /// Sender clone kept alive so `BrowserRuntimeHandle::command_sender()`
    /// can hand out fresh `CommandSender`s post-start.
    inbox_tx: mpsc::Sender<ActorMail>,
    /// Shared relay slot (caller-facing read accessor).
    configured_relays: AppRelaySlot,
}

impl BrowserRuntimeHandle {
    /// Called by `BrowserAppBuilder<ProvidersDecided>::start()`.
    ///
    /// Consumes the builder's inner state, applies all deferred `&mut`-kernel
    /// settings, installs registries, and returns the running handle.
    pub(crate) fn from_builder_inner(mut inner: BrowserBuilderInner) -> Self {
        // ── Apply deferred &mut-kernel settings ───────────────────────────────

        // Install the configured-relays slot.
        let relay_slot = Arc::clone(&inner.configured_relays_slot);
        inner.reducer.set_app_relay_slot(Arc::clone(&relay_slot));

        // Apply routing substrate factory.
        if let Some(factory) = inner.routing_substrate_factory.take() {
            let trace_observer = NoopRoutingTrace::arc();
            let (router, cache) = factory(trace_observer);
            inner.reducer.set_routing(router, cache);
        }

        // Apply publish resolver factory.
        if let Some(factory) = inner.publish_resolver_factory.take() {
            let resolver = factory(
                inner.reducer.event_store_handle(),
                inner.reducer.indexer_relays_handle(),
                inner.reducer.local_write_relays_handle(),
                inner.reducer.active_account_handle(),
            );
            inner.reducer.set_publish_resolver(resolver);
        }

        // Apply profile / contacts lookups.
        if let Some(l) = inner.profile_lookup.take() {
            inner.reducer.set_profile_lookup(l);
        }
        if let Some(l) = inner.contacts_lookup.take() {
            inner.reducer.set_contacts_lookup(l);
        }

        // Apply DM-inbox and blocked-relay lookups.
        if let Some(l) = inner.dm_inbox_relay_lookup.take() {
            inner.reducer.set_dm_inbox_relay_lookup(l);
        }
        if let Some(l) = inner.blocked_relay_lookup.take() {
            inner.reducer.set_blocked_relay_lookup(l);
        }

        // Apply coverage hook + REQ-frame interceptor.
        if let Some(hook) = inner.coverage_hook.take() {
            inner.reducer.set_coverage_hook(hook);
        }
        if let Some(interceptor) = inner.req_frame_interceptor.take() {
            inner.reducer.set_req_frame_interceptor(interceptor);
        }

        // Apply outbound public tags.
        if !inner.outbound_public_tags.is_empty() {
            inner
                .reducer
                .set_outbound_public_tags(std::mem::take(&mut inner.outbound_public_tags));
        }

        // Install search-scope registry into the event store.
        inner
            .search_scope_registry
            .install_into(&*inner.reducer.event_store_handle());

        // Apply relay bootstrap list.
        if !inner.relay_bootstrap.is_empty() {
            inner
                .reducer
                .set_configured_relays(std::mem::take(&mut inner.relay_bootstrap));
        }

        // ── Extract the receiver and build the runtime ────────────────────────

        let inbox_rx = inner
            .inbox_rx
            .take()
            .expect("BrowserRuntimeHandle: inbox_rx already consumed");
        let inbox_tx = inner.inbox_tx.clone();

        let runtime = BrowserRuntime {
            reducer: inner.reducer,
            action_registry: inner.action_registry,
            inbox_rx,
            relay_text_interceptors: inner.relay_text_interceptors,
            relay_connected_hooks: inner.relay_connected_hooks,
            identity_change_observers: inner.identity_change_observers,
            run_config: inner.run_config,
        };

        Self {
            runtime,
            inbox_tx,
            configured_relays: relay_slot,
        }
    }

    // ── Public API (narrow surface, #2058) ────────────────────────────────────

    /// Drive one turn of the event loop.
    ///
    /// Drains every pending `ActorCommand` from the inbox and applies them to
    /// the kernel. Returns the outbound relay messages produced this turn.
    /// The JS host should call this on each timer tick or after enqueuing a
    /// command.
    ///
    /// D4 (single-writer): no other code path mutates the `KernelReducer`
    /// while `pump()` runs.
    pub fn pump(&mut self) -> Vec<OutboundMessage> {
        self.runtime.pump()
    }

    /// Build the current update frame from the kernel's state.
    ///
    /// Runs all registered typed projection closures and returns the serialised
    /// `UpdateFrameBytes`. Callers invoke this after `pump()` to read updated
    /// projections.
    ///
    /// Pass `running = true` while the runtime is active (normal operation) and
    /// `running = false` when shutting down so the frame's `running` flag is set
    /// correctly.
    pub fn make_update_frame(&mut self, running: bool) -> UpdateFrameBytes {
        self.runtime.make_update_frame(running)
    }

    /// Return a `CommandSender` for this runtime's inbox.
    ///
    /// Enqueue `ActorCommand`s through this sender; they are applied on the
    /// next `pump()` call. This is the narrow command-injection surface
    /// exposed to callers (#2058).
    pub fn command_sender(&self) -> CommandSender {
        CommandSender::new(self.inbox_tx.clone())
    }

    /// Return the configured-relays slot (shared with the kernel).
    ///
    /// Callers may read or update the relay list through this handle; the
    /// kernel reads it on each tick.
    pub fn configured_relays_handle(&self) -> AppRelaySlot {
        Arc::clone(&self.configured_relays)
    }
}
