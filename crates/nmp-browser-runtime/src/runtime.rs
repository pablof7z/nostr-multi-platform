//! `BrowserRuntime` — single-writer reducer loop driven by explicit `pump()`
//! calls (issue #2046 / PR-B of the browser-runtime epic #2045).
//!
//! # Design (D4 single-writer, issue #2058)
//!
//! `BrowserRuntime` owns the `KernelReducer` and is the **sole** writer —
//! no other path mutates the kernel after `start()`. The JS host drives it
//! via `BrowserRuntimeHandle::pump()`, which drains the inbox (bounded) and
//! applies every pending `ActorCommand`.
//!
//! `BrowserRuntimeHandle` is the **only** public type (#2058): it exposes
//! the narrow dispatch / snapshot / sign surface and does NOT expose the raw
//! `KernelReducer` or `BrowserRuntime` fields, nor any mutable post-start
//! kernel handle.
//!
//! # Relay text / connected hooks
//!
//! On browser the relay pool is WebSocket-based and driven from JS land.
//! `relay_text_interceptors` and `relay_connected_hooks` are stored here and
//! invoked by the browser relay driver, which lands in #2050 (bounded
//! transport-only adapter). They are wired into the driver at that seam.

use std::collections::HashMap;
use std::sync::{mpsc, Arc};

use nmp_core::actor::ActorMail;
use nmp_core::publish::PublishTarget;
use nmp_core::substrate::{
    PublishTrace, RelayConnectedHook, RelayTextInterceptor, RoutedRelaySet, RoutingTraceObserver,
    SubscriptionTrace,
};
use nmp_core::{
    ActionRegistry, AppRelayList, CommandSender, KernelReducer, OutboundMessage, UpdateFrameBytes,
};

use crate::builder::{BrowserBuilderInner, BrowserRunConfig};

mod event;
mod pump;

pub use event::BrowserRuntimeEvent;

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

// ── Parked sign continuation ────────────────────────────────────────────────

/// A publish parked between `NeedsSign` and the signed-response delivery.
///
/// Mirrors `nmp-wasm`'s `PendingSignedPublish`. The runtime stores one of these
/// keyed on the sign `correlation_id` whenever a publish command needs an async
/// signature. The broker that delivers the signature back and re-publishes lands
/// in #2049; this struct is the runtime-side parking contract that exists now.
#[derive(Debug, Clone)]
pub(crate) struct PendingSignedPublish {
    /// The action-level correlation id the host uses to settle the result, or
    /// `None` when the command carried none.
    #[allow(dead_code)] // read by the #2049 broker when it settles the action.
    pub(crate) action_correlation_id: Option<String>,
    /// The publish target resolved when the command was interpreted.
    #[allow(dead_code)] // consumed by the #2049 broker on signed re-publish.
    pub(crate) target: PublishTarget,
}

// ── Pump outcome ─────────────────────────────────────────────────────────────

/// The result of one `BrowserRuntimeHandle::pump()` turn.
///
/// Carries the outbound relay frames produced, the host-facing events (sign
/// requests, command failures), and a `yielded` flag set when the per-turn
/// command budget was exhausted (the host should pump again).
#[derive(Debug, Default)]
pub struct PumpOutcome {
    /// Outbound relay frames the host's relay driver must send.
    pub outbound: Vec<OutboundMessage>,
    /// Host events emitted this turn (sign requests, command failures).
    pub events: Vec<BrowserRuntimeEvent>,
    /// True when the command drain budget was hit and more mail may remain —
    /// the host should call `pump()` again.
    pub yielded: bool,
}

// ── BrowserRuntime ────────────────────────────────────────────────────────────

/// Single-writer runtime owning the `KernelReducer`. Not public — callers
/// use `BrowserRuntimeHandle`.
struct BrowserRuntime {
    reducer: KernelReducer,
    /// Action registry kept so the #2049 signer-provider broker can resolve
    /// action modules when settling parked publishes. Not yet read on the pump
    /// path (`apply_actor_command` embeds action execution in the kernel).
    #[allow(dead_code)]
    action_registry: ActionRegistry,
    inbox_rx: mpsc::Receiver<ActorMail>,
    /// Publishes parked awaiting an async signature, keyed on sign correlation
    /// id. Populated on `NeedsSign`; drained by the #2049 broker.
    pending_signed_publishes: HashMap<String, PendingSignedPublish>,
    /// Relay-text interceptors — invoked by the browser relay driver (#2050)
    /// when it receives inbound relay messages.
    #[allow(dead_code)]
    relay_text_interceptors: Vec<Arc<dyn RelayTextInterceptor>>,
    /// Relay-connected hooks — invoked by the browser relay driver (#2050)
    /// when a relay socket opens.
    #[allow(dead_code)]
    relay_connected_hooks: Vec<Arc<dyn RelayConnectedHook>>,
    /// Identity-change callbacks. Fired when the active account changes; the
    /// account-switch commands that trigger them on the browser path arrive via
    /// the signer-provider registry (#2049), so they are dormant until that seam
    /// lands. Stored (not dropped) so the wiring is in place.
    #[allow(dead_code)]
    identity_change_observers: Vec<Box<dyn Fn(Option<String>) + Send + Sync + 'static>>,
    #[allow(dead_code)]
    run_config: Option<BrowserRunConfig>,
}

impl BrowserRuntime {
    fn pump(&mut self) -> PumpOutcome {
        let drain = pump::drain_inbox(
            &mut self.reducer,
            &self.inbox_rx,
            &mut self.pending_signed_publishes,
        );
        PumpOutcome {
            outbound: drain.outbound,
            events: drain.events,
            yielded: drain.yielded,
        }
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
/// commands through a `CommandSender` (from [`Self::command_sender`]) and drive
/// the event loop by calling [`Self::pump`] on each timer tick.
pub struct BrowserRuntimeHandle {
    runtime: BrowserRuntime,
    /// Sender clone kept alive so [`Self::command_sender`] can hand out fresh
    /// `CommandSender`s post-start.
    inbox_tx: mpsc::Sender<ActorMail>,
    /// Shared relay slot. Read-only to callers — see [`Self::configured_relays`].
    /// The kernel is the sole writer (D4); relay-list mutations flow through the
    /// command inbox, never through this handle.
    configured_relays: nmp_core::AppRelaySlot,
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
            pending_signed_publishes: HashMap::new(),
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
    /// Drains up to the per-turn command budget from the inbox and applies each
    /// to the kernel. Returns a [`PumpOutcome`] with the outbound relay frames,
    /// host events (sign requests / command failures), and a `yielded` flag set
    /// when more mail may remain (the host should pump again).
    ///
    /// D4 (single-writer): no other code path mutates the `KernelReducer`
    /// while `pump()` runs.
    pub fn pump(&mut self) -> PumpOutcome {
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
    /// exposed to callers (#2058). Relay-list edits, identity changes, and all
    /// other state mutations go through this lane — never through a raw kernel
    /// handle.
    pub fn command_sender(&self) -> CommandSender {
        CommandSender::new(self.inbox_tx.clone())
    }

    /// Read a snapshot of the configured relay list.
    ///
    /// Returns an owned clone (read-only). The kernel is the sole writer of the
    /// relay slot (D4); to change the relay list, send the appropriate command
    /// through [`Self::command_sender`]. This handle never exposes a mutable
    /// relay handle (#2058 — no post-start mutable state escape). On a poisoned
    /// slot it returns an empty list rather than panicking (D6).
    #[must_use]
    pub fn configured_relays(&self) -> AppRelayList {
        self.configured_relays
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Number of publishes currently parked awaiting a signature.
    ///
    /// Each `pump()` that yields a [`BrowserRuntimeEvent::SignRequest`] parks one
    /// entry here until the #2049 broker delivers the signature. Exposed for
    /// tests and host diagnostics; never leaks the parked payloads.
    #[must_use]
    pub fn pending_sign_count(&self) -> usize {
        self.runtime.pending_signed_publishes.len()
    }
}

#[cfg(test)]
mod tests;
