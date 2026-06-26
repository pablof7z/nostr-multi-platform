//! `BrowserRuntime` — single-writer reducer loop driven by explicit `pump()`
//! calls (issue #2046 / PR-B of the browser-runtime epic #2045).
//!
//! # Design (D4 single-writer, issue #2058)
//!
//! `BrowserRuntime` owns the `KernelReducer` and is the **sole** writer —
//! no other path mutates the kernel after `start()`. The JS host drives it
//! via `BrowserRuntimeHandle::pump()`, which drains the inbox (bounded) and
//! applies every pending `ActorCommand`, then drains the inbound relay queue,
//! runs a maintenance tick, and fans outbound frames to relay drivers.
//!
//! `BrowserRuntimeHandle` is the **only** public type (#2058): it exposes
//! the narrow dispatch / snapshot / sign surface and does NOT expose the raw
//! `KernelReducer` or `BrowserRuntime` fields, nor any mutable post-start
//! kernel handle.
//!
//! # Relay transport (#2050)
//!
//! `BrowserRuntime` owns a `RelayPool` driving WebSocket transport.
//! Inbound frames are enqueued (D4) and drained on each `pump()`.
//! Maintenance (`tick_at`) and next-deadline scheduling also run per turn.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc};

use nmp_core::actor::ActorMail;
use nmp_core::publish::PublishTarget;
use nmp_core::substrate::{
    PublishTrace, RelayConnectedHook, RelayTextInterceptor, RoutedRelaySet, RoutingTraceObserver,
    SubscriptionTrace,
};
use nmp_core::time::Instant;
use nmp_core::{
    ActionRegistry, AppRelayList, CommandSender, KernelReducer, OutboundMessage, UpdateFrameBytes,
};

use crate::builder::{BrowserBuilderInner, BrowserRunConfig};
use crate::relay::RelayPool;
use crate::signer::{
    enqueue_completion, CapabilityEnvelope, CapabilityProviderRegistry, SignerCompletion,
    SignerCompletionRx, SignerCompletionTx,
};

mod event;
mod pump;
mod signer_delivery;

pub use event::BrowserRuntimeEvent;

// ── No-op routing trace observer ─────────────────────────────────────────────

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
/// signature. Consumed by `signer_delivery::deliver_one_completion` when the
/// signed response arrives (broker side: `signer/completion.rs`).
#[derive(Debug, Clone)]
pub(crate) struct PendingSignedPublish {
    /// The action-level correlation id the host uses to settle the result, or
    /// `None` when the command carried none.
    pub(crate) action_correlation_id: Option<String>,
    /// The publish target resolved when the command was interpreted.
    pub(crate) target: PublishTarget,
}

// ── Pump outcome ─────────────────────────────────────────────────────────────

/// The result of one `BrowserRuntimeHandle::pump()` turn.
///
/// Carries the outbound relay frames produced, the host-facing events (sign
/// requests, command failures, relay budget exceeded), and a `yielded` flag
/// set when the per-turn command or inbound budget was exhausted (the host
/// should pump again).
#[derive(Debug, Default)]
pub struct PumpOutcome {
    /// Outbound relay frames the host's relay driver must send (already fanned
    /// to the pool's WebSocket drivers on wasm32; exposed for diagnostics /
    /// non-wasm32 consumers).
    pub outbound: Vec<OutboundMessage>,
    /// Host events emitted this turn (sign requests, command failures, budget).
    pub events: Vec<BrowserRuntimeEvent>,
    /// True when the command drain budget OR the inbound relay drain budget was
    /// hit and more mail may remain — the host should call `pump()` again.
    pub yielded: bool,
}

// ── BrowserRuntime ────────────────────────────────────────────────────────────

/// Single-writer runtime owning the `KernelReducer`. Not public — callers
/// use `BrowserRuntimeHandle`.
struct BrowserRuntime {
    reducer: KernelReducer,
    /// Action registry kept for future broker resolution of action modules.
    #[allow(dead_code)]
    action_registry: ActionRegistry,
    inbox_rx: mpsc::Receiver<ActorMail>,
    /// A sender clone stored here so relay connected-hooks can post follow-up
    /// commands back through the inbox during `pump()`.
    inbox_tx: mpsc::Sender<ActorMail>,
    /// Publishes parked awaiting an async signature, keyed on sign correlation
    /// id. Populated on `NeedsSign`; drained by `signer_delivery` on completion.
    pending_signed_publishes: HashMap<String, PendingSignedPublish>,
    /// Capability/signer-provider registry (#2049 / #2065). Populated from
    /// `BrowserBuilderInner::capability_providers` at `start()`.
    signer_registry: CapabilityProviderRegistry,
    /// Sender end of the sign-completion channel. Cloned into `drain_inbox`
    /// so the broker can send completions; drained in `pump()` after the
    /// command drain.
    signer_completion_tx: SignerCompletionTx,
    /// Receiver end of the sign-completion channel. Drained in `pump()`.
    signer_completion_rx: SignerCompletionRx,
    /// Relay-text interceptors — invoked during the inbound relay drain (#2050)
    /// for every inbound text frame (D0: substrate-generic, no app nouns here).
    relay_text_interceptors: Vec<Arc<dyn RelayTextInterceptor>>,
    /// Relay-connected hooks — invoked during the inbound relay drain (#2050)
    /// when a relay socket opens. Hooks spawn async work and return immediately
    /// (D8 no-blocking).
    relay_connected_hooks: Vec<Arc<dyn RelayConnectedHook>>,
    /// Identity-change callbacks (stored for future identity-change wiring).
    #[allow(dead_code)]
    identity_change_observers: Vec<Box<dyn Fn(Option<String>) + Send + Sync + 'static>>,
    #[allow(dead_code)]
    run_config: Option<BrowserRunConfig>,
    /// Browser relay pool — WebSocket drivers + inbound queue + maintenance
    /// timer (#2050). On native test builds holds only the queue and timer
    /// (no actual sockets).
    relay_pool: RelayPool,
    /// Events produced by `spawn_bootstrap` at start() (socket-budget exceeded,
    /// spawn-failed) that have not yet been surfaced. Drained into the first
    /// `pump()`'s `PumpOutcome.events` so a bad bootstrap relay is observable
    /// (D6 — never a silent drop).
    pending_startup_events: Vec<BrowserRuntimeEvent>,
}

impl BrowserRuntime {
    /// Deliver one settled sign completion to the kernel (D4: called only from
    /// `pump()` or `BrowserRuntimeHandle::deliver_signer_response`).
    fn deliver_one_completion(
        &mut self,
        completion: SignerCompletion,
    ) -> (Vec<OutboundMessage>, Vec<BrowserRuntimeEvent>) {
        signer_delivery::deliver_one_completion(
            &mut self.reducer,
            &mut self.pending_signed_publishes,
            completion,
        )
    }

    fn pump(&mut self) -> PumpOutcome {
        // ── 1. Command inbox drain ────────────────────────────────────────────
        // Clone the shared wake cell first (cheap Rc clone) so the async NIP-07
        // broker path can fire it from a future JS task; cloning here avoids
        // holding a borrow on the pool across the &mut reducer drain.
        let wake = self.relay_pool.wake_cell();
        let cmd_sender = CommandSender::new(self.inbox_tx.clone());
        let cmd_drain = pump::drain_inbox(
            &mut self.reducer,
            &self.inbox_rx,
            &mut self.pending_signed_publishes,
            &self.signer_registry,
            &self.signer_completion_tx,
            &wake,
        );

        // ── 1.5. Drain sign completions (bounded — same budget as cmd drain) ───
        // LocalKey completions arrive here synchronously in the same turn.
        // NIP-07 async + host-brokered completions arrive in a future turn (the
        // wake scheduled the pump that drains them).
        let mut completion_outbound: Vec<OutboundMessage> = Vec::new();
        let mut completion_events: Vec<BrowserRuntimeEvent> = Vec::new();
        let mut completions_applied = 0usize;
        let mut completion_yielded = false;
        loop {
            if completions_applied >= pump::BROWSER_COMMAND_DRAIN_BUDGET {
                // Budget hit; remaining completions stay queued. Signal a re-pump
                // so they are not stranded (mirrors the command/relay drains).
                completion_yielded = true;
                break;
            }
            match self.signer_completion_rx.try_recv() {
                Ok(c) => {
                    completions_applied += 1;
                    let (o, e) = self.deliver_one_completion(c);
                    completion_outbound.extend(o);
                    completion_events.extend(e);
                }
                Err(_) => break,
            }
        }

        // ── 2. Inbound relay event drain ──────────────────────────────────────
        let relay_drain = self.relay_pool.drain_inbound(
            &mut self.reducer,
            &self.relay_text_interceptors,
            &self.relay_connected_hooks,
            &cmd_sender,
        );

        // ── 3. Relay idle-tick sweep ──────────────────────────────────────────
        // Run every registered interceptor's `on_idle_tick` (e.g. NWC expiry
        // sweeps). Mirrors the native actor loop's idle section; D8: hooks
        // compare kernel timestamps and emit, never sleep.
        let idle_outbound = self
            .reducer
            .run_relay_idle_tick(&self.relay_text_interceptors);

        // ── 4. Maintenance tick (tick_at + arm next deadline) ─────────────────
        let now = Instant::now();
        let tick_outbound = self.relay_pool.tick_and_arm(&mut self.reducer, now);

        // ── 5. Collect all outbound from this turn ────────────────────────────
        let mut all_outbound: Vec<OutboundMessage> = Vec::new();
        all_outbound.extend(cmd_drain.outbound);
        all_outbound.extend(completion_outbound);
        all_outbound.extend(relay_drain.outbound);
        all_outbound.extend(idle_outbound);
        all_outbound.extend(tick_outbound);

        // ── 6. Fan outbound to relay drivers (wasm32: actual sends; native: no-op)
        let mut events: Vec<BrowserRuntimeEvent> = Vec::new();
        // Bootstrap spawn events (budget-exceeded / spawn-failed) captured at
        // start() are surfaced on the first pump (never silently discarded — D6).
        events.append(&mut self.pending_startup_events);
        events.extend(cmd_drain.events);
        events.extend(completion_events);
        events.extend(relay_drain.events);
        let budget_events = self.relay_pool.fan_out_outbound(&all_outbound);
        events.extend(budget_events);

        PumpOutcome {
            outbound: all_outbound,
            events,
            yielded: cmd_drain.yielded || relay_drain.yielded || completion_yielded,
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
    configured_relays: nmp_core::AppRelaySlot,
}

impl BrowserRuntimeHandle {
    /// Called by `BrowserAppBuilder<ProvidersDecided>::start()`.
    ///
    /// Consumes the builder's inner state, applies all deferred `&mut`-kernel
    /// settings, installs registries, and returns the running handle.
    pub(crate) fn from_builder_inner(mut inner: BrowserBuilderInner) -> Self {
        // ── Apply deferred &mut-kernel settings ───────────────────────────────

        let relay_slot = Arc::clone(&inner.configured_relays_slot);
        inner.reducer.set_app_relay_slot(Arc::clone(&relay_slot));

        if let Some(factory) = inner.routing_substrate_factory.take() {
            let trace_observer = NoopRoutingTrace::arc();
            let (router, cache) = factory(trace_observer);
            inner.reducer.set_routing(router, cache);
        }

        if let Some(factory) = inner.publish_resolver_factory.take() {
            let resolver = factory(
                inner.reducer.event_store_handle(),
                inner.reducer.indexer_relays_handle(),
                inner.reducer.local_write_relays_handle(),
                inner.reducer.active_account_handle(),
            );
            inner.reducer.set_publish_resolver(resolver);
        }

        if let Some(l) = inner.profile_lookup.take() {
            inner.reducer.set_profile_lookup(l);
        }
        if let Some(l) = inner.contacts_lookup.take() {
            inner.reducer.set_contacts_lookup(l);
        }
        if let Some(l) = inner.dm_inbox_relay_lookup.take() {
            inner.reducer.set_dm_inbox_relay_lookup(l);
        }
        if let Some(l) = inner.blocked_relay_lookup.take() {
            inner.reducer.set_blocked_relay_lookup(l);
        }
        if let Some(hook) = inner.coverage_hook.take() {
            inner.reducer.set_coverage_hook(hook);
        }
        if let Some(interceptor) = inner.req_frame_interceptor.take() {
            inner.reducer.set_req_frame_interceptor(interceptor);
        }

        if !inner.outbound_public_tags.is_empty() {
            inner
                .reducer
                .set_outbound_public_tags(std::mem::take(&mut inner.outbound_public_tags));
        }

        inner
            .search_scope_registry
            .install_into(&*inner.reducer.event_store_handle());

        // Capture bootstrap list BEFORE consuming it into the kernel, so we can
        // open relay WebSockets from the same (url, role) pairs.
        let bootstrap_list: Vec<(String, String)> = inner.relay_bootstrap.clone();

        if !inner.relay_bootstrap.is_empty() {
            inner
                .reducer
                .set_configured_relays(std::mem::take(&mut inner.relay_bootstrap));
        }

        // ── Build relay pool ──────────────────────────────────────────────────
        let user_agent = inner.relay_user_agent.take();
        let relay_pool = RelayPool::new(user_agent);

        // ── Build signer registry from accumulated providers (#2049) ──────────
        let mut signer_registry = CapabilityProviderRegistry::new();
        for signer in std::mem::take(&mut inner.capability_providers) {
            signer_registry.insert(signer);
        }
        let (signer_completion_tx, signer_completion_rx) =
            mpsc::channel::<SignerCompletion>();

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
            inbox_tx: inbox_tx.clone(),
            pending_signed_publishes: HashMap::new(),
            signer_registry,
            signer_completion_tx,
            signer_completion_rx,
            relay_text_interceptors: inner.relay_text_interceptors,
            relay_connected_hooks: inner.relay_connected_hooks,
            identity_change_observers: inner.identity_change_observers,
            run_config: inner.run_config,
            relay_pool,
            pending_startup_events: Vec::new(),
        };

        let mut handle = Self {
            runtime,
            inbox_tx,
            configured_relays: relay_slot,
        };

        // ── Spawn relay drivers from bootstrap list (wasm32 only) ────────────
        // Opens one WebSocket per distinct relay URL. On native this is a no-op
        // (no actual sockets in native test builds).
        handle.spawn_relay_bootstrap(&bootstrap_list);

        handle
    }

    // ── Public API (narrow surface, #2058) ────────────────────────────────────

    /// Drive one turn of the event loop.
    ///
    /// Drains up to the per-turn budget from both the command inbox and the
    /// inbound relay queue, applies each to the kernel, runs a maintenance tick,
    /// and fans outbound frames to relay drivers. Returns a [`PumpOutcome`] with
    /// the outbound frames, host events, and a `yielded` flag when more mail
    /// may remain.
    ///
    /// D4 (single-writer): no other code path mutates the `KernelReducer`
    /// while `pump()` runs.
    pub fn pump(&mut self) -> PumpOutcome {
        self.runtime.pump()
    }

    /// Install the wake hook: the relay pool calls it when inbound events
    /// arrive to signal a `pump()` turn is needed. On wasm32 the nmp-wasm
    /// bridge sets this to a 0ms-timer; tests call `pump()` directly.
    /// Seam documented in the plan (#2050 O3).
    pub fn set_wake(&mut self, wake: Rc<dyn Fn()>) {
        self.runtime.relay_pool.set_wake(wake);
    }

    /// Build the current update frame from the kernel's state.
    pub fn make_update_frame(&mut self, running: bool) -> UpdateFrameBytes {
        self.runtime.make_update_frame(running)
    }

    /// Return a `CommandSender` for this runtime's inbox.
    pub fn command_sender(&self) -> CommandSender {
        CommandSender::new(self.inbox_tx.clone())
    }

    /// Read a snapshot of the configured relay list.
    #[must_use]
    pub fn configured_relays(&self) -> AppRelayList {
        self.configured_relays
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Number of publishes currently parked awaiting a signature.
    #[must_use]
    pub fn pending_sign_count(&self) -> usize {
        self.runtime.pending_signed_publishes.len()
    }

    /// Host-brokered sign delivery — re-entry without polling (D8).
    ///
    /// `result`: `Ok(flat-NIP-01 signed JSON)` on success or `Err(reason)` on
    /// failure. D4 (single-writer): this does NOT touch the reducer — it
    /// enqueues a `SignerCompletion` on the broker's channel and fires the
    /// shared wake (the same mechanism relay inbound uses), so `pump()` — the
    /// sole reducer write point — applies it and routes the signed event.
    pub fn deliver_signer_response(&mut self, correlation_id: String, result: Result<String, String>) {
        let completion = SignerCompletion { correlation_id, result };
        let wake = self.runtime.relay_pool.wake_cell();
        enqueue_completion(&self.runtime.signer_completion_tx, &wake, completion);
    }

    /// Return the capability envelope for `account_pubkey` (lowercase hex),
    /// or `None` if no provider is registered for that account.
    ///
    /// Callers can inspect this to determine what signing and encrypt/decrypt
    /// capabilities are available for an account without triggering a sign
    /// round-trip (introspection, #2065).
    #[must_use]
    pub fn capability_envelope(&self, account_pubkey: &str) -> Option<CapabilityEnvelope> {
        self.runtime
            .signer_registry
            .capability_envelope(account_pubkey)
            .cloned()
    }

    /// Test-support only: seed the active account without a full identity
    /// round-trip. Mirrors `KernelReducer::set_active_account_for_test`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.runtime.reducer.set_active_account_for_test(pubkey);
    }

    /// Spawn relay drivers from `bootstrap` (wasm32: opens WebSockets; native:
    /// no-op). Called once from `from_builder_inner` with the bootstrap list
    /// captured before it was consumed into the kernel.
    ///
    /// Any socket-budget-exceeded or spawn-failed events from bootstrap are
    /// parked in `pending_startup_events` and surfaced on the first `pump()`
    /// (D6 — a bad bootstrap relay is never silently dropped).
    fn spawn_relay_bootstrap(&mut self, bootstrap: &[(String, String)]) {
        #[cfg(target_arch = "wasm32")]
        {
            let events = self.runtime.relay_pool.spawn_bootstrap(bootstrap);
            self.runtime.pending_startup_events.extend(events);
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = bootstrap;
    }
}

#[cfg(test)]
mod tests;
