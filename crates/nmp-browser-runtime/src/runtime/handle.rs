//! `BrowserRuntimeHandle` — public-facing runtime handle (#2058/#2051).
//!
//! Extracted from `runtime.rs` to keep both files under the 500-LOC ceiling
//! (AGENTS.md). The handle exposes the narrow dispatch / snapshot / sign /
//! diagnostics surface WITHOUT exposing the raw `KernelReducer` or
//! `BrowserRuntime` fields.
//!
//! # New surface (#2051 / #2073 / #2074 / #2075 / #2076)
//!
//! - `next_frame(running) -> SnapshotOutcome` — fail-closed snapshot (#2073)
//! - `set_signer_state(model)` — write the browser signer-state slot (#2074)
//! - `diagnostics() -> BrowserRuntimeDiagnostics` — log-safe diagnostics (#2075)
//! - Clock injection applied in `from_builder_inner` (#2076)

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc};

use nmp_core::substrate::{ObservedProjectionCommandHandle, PreferredRelaySource};
use nmp_core::{AppRelayList, CommandSender, SignerStateModel, UpdateFrameBytes};
use nmp_signers::Signer;

use super::diagnostics::BrowserRuntimeDiagnostics;
use super::event::BrowserRuntimeEvent;
use super::signer_state::{
    new_signer_state_slot, ready_model, register_signer_state_projection, update_signer_state,
    BrowserSignerStateSlot,
};
use super::snapshot::{BrowserSnapshotCache, SnapshotOutcome};
use super::{BrowserRuntime, PumpOutcome};
use crate::builder::BrowserBuilderInner;
use crate::relay::RelayPool;
use crate::signer::{
    enqueue_completion, CapabilityEnvelope, CapabilityProviderRegistry, SignerCompletion,
};

use super::NoopRoutingTrace;
use super::{
    BrowserGroupDiscoverySession, BrowserGroupTimelineSession, BrowserNotificationsSession,
    BrowserSearchSession,
};

/// Public-facing handle to the browser runtime (issue #2058 — hides raw
/// reducer/runtime handles).
///
/// Returned by `BrowserAppBuilder<ProvidersDecided>::start()`. Callers send
/// commands through a `CommandSender` (from [`Self::command_sender`]) and drive
/// the event loop by calling [`Self::pump`] on each timer tick.
pub struct BrowserRuntimeHandle {
    pub(super) runtime: BrowserRuntime,
    /// Sender clone kept alive so [`Self::command_sender`] can hand out fresh
    /// `CommandSender`s post-start.
    pub(super) inbox_tx: mpsc::Sender<nmp_core::actor::ActorMail>,
    /// Shared relay slot. Read-only to callers — see [`Self::configured_relays`].
    pub(super) configured_relays: nmp_core::AppRelaySlot,

    // ── #2073 — fail-closed snapshot cache ───────────────────────────────────
    pub(super) snapshot_cache: BrowserSnapshotCache,

    // ── #2074 — Rust-owned signer-state slot ─────────────────────────────────
    pub(super) signer_state_slot: BrowserSignerStateSlot,

    pub(super) preferred_relay_source: Option<Arc<dyn PreferredRelaySource>>,
    pub(super) observed_projection_registrar: ObservedProjectionCommandHandle,
    pub(super) search_sessions: HashMap<String, BrowserSearchSession>,
    pub(super) group_discovery_sessions: HashMap<String, BrowserGroupDiscoverySession>,
    pub(super) group_timeline_sessions: HashMap<String, BrowserGroupTimelineSession>,
    pub(super) notifications_sessions: HashMap<String, BrowserNotificationsSession>,
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

        // #2076 — apply the injected clock (if any) before the first tick.
        if let Some(clock) = inner.clock.take() {
            inner.reducer.set_clock(clock);
        }

        // #1007 PR-8 — thread the degraded OPFS open reason onto the kernel so it
        // surfaces through the Tier-3 `store_open_failure` snapshot, the same
        // channel native LMDB uses at init. Only set on the in-memory fallback
        // path (a successful inject_store never carries a reason), and the kernel
        // for that path is the default reducer kernel (never rebuilt), so this is
        // never clobbered by a later store swap.
        if let Some(reason) = inner.store_open_failure.take() {
            inner.reducer.set_store_open_failure(reason);
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
        let (signer_completion_tx, signer_completion_rx) = mpsc::channel::<SignerCompletion>();

        // ── #2074 — Rust-owned signer-state slot + typed projection ──────────
        let signer_state_slot = new_signer_state_slot();
        register_signer_state_projection(&inner.reducer, Arc::clone(&signer_state_slot));
        // Seed signer-state readiness from the SOLE registered provider so the
        // projection reflects reality (a signer is available + ready) rather
        // than silently empty. Multi-provider per-account selection and the
        // full sign-success/failure/reconnecting lifecycle are deferred (#2068).
        if let Some(backend) = signer_registry.sole_backend() {
            update_signer_state(&signer_state_slot, Some(ready_model(&backend)));
        }

        // ── Extract the receiver and build the runtime ────────────────────────

        let preferred_relay_source = inner.preferred_relay_source.take();
        let inbox_rx = inner.inbox_rx;
        let inbox_tx = inner.inbox_tx.clone();
        let observed_projection_registrar = inner.reducer.observed_projection_command_handle(
            Arc::clone(&inner.observed_projection_sessions),
            CommandSender::new(inbox_tx.clone()),
        );

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
            snapshot_cache: BrowserSnapshotCache::new(),
            signer_state_slot,
            preferred_relay_source,
            observed_projection_registrar,
            search_sessions: HashMap::new(),
            group_discovery_sessions: HashMap::new(),
            group_timeline_sessions: HashMap::new(),
            notifications_sessions: HashMap::new(),
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

    /// Build the current update frame from the kernel's state (raw bytes, no
    /// merge-cache applied).
    ///
    /// Prefer [`Self::next_frame`] for the fail-closed merged form (#2073).
    /// This method remains for consumers that want the raw kernel output.
    pub fn make_update_frame(&mut self, running: bool) -> UpdateFrameBytes {
        self.runtime.make_update_frame(running)
    }

    /// Produce the next merged snapshot frame — fail-closed (#2073).
    ///
    /// Calls `KernelReducer::make_update_frame`, runs the result through
    /// the `BrowserSnapshotCache`, and returns a [`SnapshotOutcome`]:
    ///
    /// - `Frame(bytes)`: valid merged frame — ship to the host.
    /// - `Degraded { last_good, reason }`: transient decode error; `last_good`
    ///   is the previous valid frame (or `None`). A
    ///   [`BrowserRuntimeEvent::SnapshotDecodeFailed`] event is also pushed
    ///   so the host can observe the error (D6 — no silent drop).
    /// - `Panic(msg)`: the kernel emitted a panic frame — terminal.
    ///
    /// D4 (single-writer): `make_update_frame` mutates the reducer and is
    /// only called here (never concurrently with `pump`).
    pub fn next_frame(&mut self, running: bool) -> SnapshotOutcome {
        let raw = self.runtime.make_update_frame(running);
        let outcome = self.snapshot_cache.apply_frame(&raw);
        // D6: emit SnapshotDecodeFailed on degraded so the host can observe it.
        if let SnapshotOutcome::Degraded { ref reason, .. } = outcome {
            self.runtime
                .pending_startup_events
                .push(BrowserRuntimeEvent::SnapshotDecodeFailed {
                    reason: reason.clone(),
                });
        }
        outcome
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
    pub fn deliver_signer_response(
        &mut self,
        correlation_id: String,
        result: Result<String, String>,
    ) {
        let completion = SignerCompletion {
            correlation_id,
            result,
        };
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

    /// Install or replace a signer provider and publish signer readiness.
    ///
    /// D4: this mutates the provider registry and signer-state slot only through
    /// the runtime handle, outside `pump()`. The caller still owns setting the
    /// active account so relay/follow projections update through the normal
    /// kernel reducer path.
    pub(crate) fn install_signer_provider(&mut self, signer: Arc<dyn Signer>) -> String {
        let pubkey = signer.pubkey().to_hex();
        let backend = signer.backend();
        self.runtime.signer_registry.insert(signer);
        self.set_signer_state(Some(ready_model(&backend)));
        pubkey
    }

    // ── #2074 — signer-state slot writer ─────────────────────────────────────

    /// Write a new signer-state model into the Rust-owned slot (#2074).
    ///
    /// The next `make_update_frame` / `next_frame` tick will pick up the new
    /// value and emit it in the `"signer_state"` typed sidecar.
    ///
    /// Pass `None` to clear the slot (matches the native actor's idle state —
    /// no `"signer_state"` sidecar entry when no signer is active).
    ///
    /// D4 (single-writer via this method): only call from the main runtime
    /// path, never from inside a `pump()` turn or a typed-projection closure.
    pub fn set_signer_state(&mut self, model: Option<SignerStateModel>) {
        update_signer_state(&self.signer_state_slot, model);
    }

    // ── #2075 — log-safe diagnostics ─────────────────────────────────────────

    /// Build a log-safe [`BrowserRuntimeDiagnostics`] snapshot (#2075).
    ///
    /// Uses the last successfully merged frame (from `next_frame`), the
    /// configured relay list, the pending sign count, and the signer-state slot.
    /// All fields are redacted: no secret material, no DM content, identity is
    /// an 8-char npub prefix only.
    ///
    /// D6 — total: a poisoned signer-state slot or any decode error degrades
    /// the affected field to its `Default`; the method never panics.
    #[must_use]
    pub fn diagnostics(&self) -> BrowserRuntimeDiagnostics {
        let configured_relay_count = self
            .configured_relays
            .lock()
            .map(|g| g.as_slice().len())
            .unwrap_or(0);
        let active_pubkey = self.runtime.reducer.active_account_pubkey();
        BrowserRuntimeDiagnostics::build(
            self.snapshot_cache.last_good(),
            self.runtime.pending_signed_publishes.len(),
            configured_relay_count,
            &self.signer_state_slot,
            active_pubkey,
        )
    }

    // ── Test-support ──────────────────────────────────────────────────────────

    /// Test-support only: seed the active account without a full identity
    /// round-trip. Mirrors `KernelReducer::set_active_account_for_test`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.runtime.reducer.set_active_account_for_test(pubkey);
    }

    /// Test-support only: borrow the kernel's `EventStore` handle.
    ///
    /// Lets the native injection-identity test assert that the store passed to
    /// `BrowserAppBuilder::inject_store` is the exact `Arc` the reducer ends up
    /// holding — the live-path analog of the nmp-wasm hook test (#1007 PR-7).
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn event_store_handle(&self) -> std::sync::Arc<dyn nmp_store::EventStore> {
        self.runtime.reducer.event_store_handle()
    }

    /// Read the degraded store-open failure reason recorded on the kernel, if any
    /// (#1007 PR-8). `None` for a healthy durable open / in-memory start. The
    /// native, always-runnable analog of asserting the wasm OPFS degraded session
    /// reports a Tier-3 `store_open_failure` diagnostic.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn store_open_failure(&self) -> Option<String> {
        self.runtime.reducer.store_open_failure()
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
