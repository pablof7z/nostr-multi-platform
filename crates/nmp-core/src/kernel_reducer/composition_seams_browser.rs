//! PR-B (#2046) `KernelReducer` seams for `BrowserAppBuilder`.
//!
//! Factored out of `composition_seams.rs` to stay under the 500-LOC ceiling
//! (AGENTS.md). The methods here satisfy the full `AppHost` trait surface for
//! `BrowserAppBuilder` — allowing it to act as a composition root with
//! `nmp-defaults::register_defaults` without depending on `nmp-ffi`.
//!
//! # Doctrine
//!
//! * **D0** — all types crossing this surface are substrate-level.
//! * **D6** — poisoned mutex / lock is a silent no-op; caller never panics.
//! * **D8** — all methods are O(1) slot installs; no I/O, no blocking.

use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use crate::actor::unregister_observer_internal as unregister_observer;
use crate::relay::OutboundMessage;
use crate::substrate::{
    BlockedRelayLookup, DmInboxRelayLookup, IngestParser, RelayTextInterceptor, ReqFrameInterceptor,
};
use crate::substrate::IncrementalApplyError;
use crate::{AppRelaySlot, KernelEventObserverId};

impl super::KernelReducer {
    // ── Browser relay-interceptor composition seams (#2050) ───────────────────

    /// Run all registered [`RelayTextInterceptor`]s against a single inbound
    /// text frame, returning the union of their outbound messages.
    ///
    /// This is the browser-relay path equivalent of what the native actor loop
    /// does inside `dispatch::relay_events::handle_relay_event`. Calling
    /// interceptors through this seam preserves D4 (single-writer): the
    /// browser relay handlers only enqueue raw text, and the drain path calls
    /// this method from within `pump()` under the sole mutable `KernelReducer`
    /// borrow.
    ///
    /// D0: substrate-generic — `KernelReducer` names no app/protocol nouns here.
    pub fn run_relay_text_interceptors(
        &mut self,
        interceptors: &[Arc<dyn RelayTextInterceptor>],
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        let mut outbound = Vec::new();
        for interceptor in interceptors {
            outbound.extend(interceptor.on_relay_text(&mut self.kernel, relay_url, text));
        }
        outbound
    }

    /// Run the `on_idle_tick` hook on all registered [`RelayTextInterceptor`]s,
    /// returning the union of their outbound messages.
    ///
    /// Mirrors the native actor loop's idle-section sweep (D8: no sleep inside
    /// hooks; compare kernel timestamps and emit failures for expired entries).
    pub fn run_relay_idle_tick(
        &mut self,
        interceptors: &[Arc<dyn RelayTextInterceptor>],
    ) -> Vec<OutboundMessage> {
        let mut outbound = Vec::new();
        for interceptor in interceptors {
            outbound.extend(interceptor.on_idle_tick(&mut self.kernel));
        }
        outbound
    }

    // ── Deferred &mut seams (applied at start()) ──────────────────────────────

    /// Install the subscription-plan coverage hook (ADR-0053 diagnostics seam).
    ///
    /// `BrowserAppBuilder` forwards `CoverageHookRegistrar::set_coverage_hook`
    /// here, deferred to `start()` so the reducer exists before the call.
    pub fn set_coverage_hook(&mut self, hook: crate::subs::PlanCoverageHook) {
        self.kernel.lifecycle_mut().set_coverage_hook(hook);
    }

    /// Install the REQ-frame interceptor (subscription-plan rewrite seam).
    ///
    /// `BrowserAppBuilder` forwards `ReqFrameInterceptorRegistrar` here.
    pub fn set_req_frame_interceptor(&mut self, interceptor: Arc<dyn ReqFrameInterceptor>) {
        self.kernel.lifecycle_mut().set_req_frame_interceptor(interceptor);
    }

    /// Install the DM-inbox relay lookup. Deferred to start() from the builder.
    pub fn set_dm_inbox_relay_lookup(&mut self, lookup: Arc<dyn DmInboxRelayLookup>) {
        self.kernel.set_dm_inbox_relay_lookup(lookup);
    }

    /// Install the blocked-relay lookup. Deferred to start() from the builder.
    pub fn set_blocked_relay_lookup(&mut self, lookup: Arc<dyn BlockedRelayLookup>) {
        self.kernel.set_blocked_relay_lookup(lookup);
    }

    /// Set substrate-generic outbound public tags (Flow B, D0). Deferred to
    /// start() from the builder.
    pub fn set_outbound_public_tags(&mut self, tags: Vec<Vec<String>>) {
        self.kernel.set_outbound_public_tags(tags);
    }

    /// Install the shared configured-relays slot — called once at start() so
    /// `HostCapabilities::configured_relays_handle()` and relay drivers read
    /// the same `Arc<Mutex<AppRelayList>>` the actor writes.
    pub fn set_app_relay_slot(&mut self, slot: AppRelaySlot) {
        self.kernel.set_app_relay_slot(slot);
    }

    // ── Snapshot-slot bridges (&self, via Arc<Mutex<SnapshotRegistry>>) ──────

    /// ADR-0053 — declare the static set of Tier-2 built-in projection keys
    /// this host consumes. Bridges `SnapshotProjectionRegistrar::declare_consumed_projections`.
    pub fn declare_consumed_projections<I, K>(&self, keys: I)
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.declare_consumed_projections(keys);
        }
        // D6 — poisoned mutex: silent drop.
    }

    /// ADR-0053 / Workstream-E4 — declare the explicit "I consume every Tier-2
    /// built-in" intent (`DeclaredProjections::All`).
    ///
    /// The browser builder's `consume_all_builtin_projections()` gate forwards
    /// here — the visible, greppable "I want everything" choice that advances the
    /// typestate without narrowing. D6 — poisoned mutex: silent drop.
    pub fn consume_all_builtin_projections(&self) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.consume_all_builtin_projections();
        }
    }

    /// ADR-0055 Rung 3 — declare incremental-apply capability.
    /// Bridges `SnapshotProjectionRegistrar::declare_incremental_apply`.
    pub fn declare_incremental_apply(&self) -> Result<(), IncrementalApplyError> {
        match self.snapshot_slot.lock() {
            Ok(mut guard) => {
                guard.declare_incremental_apply();
                Ok(())
            }
            Err(_) => Err(IncrementalApplyError::RegistryUnavailable),
        }
    }

    /// ADR-0055 R6-S1 — clone of the incremental-apply flag handle.
    /// Bridges `SnapshotProjectionRegistrar::incremental_apply_handle`.
    pub fn incremental_apply_handle(&self) -> Arc<AtomicBool> {
        self.snapshot_slot
            .lock()
            .map(|guard| guard.incremental_apply_handle())
            .unwrap_or_else(|_| Arc::new(AtomicBool::new(false)))
    }

    /// ADR-0055 R6-S1 — clones of the `(session_id, snapshot_epoch)` handles.
    /// Bridges `SnapshotProjectionRegistrar::frame_identity_handles`.
    pub fn frame_identity_handles(&self) -> (Arc<AtomicU64>, Arc<AtomicU64>) {
        self.snapshot_slot
            .lock()
            .map(|guard| guard.frame_identity_handles())
            .unwrap_or_else(|_| (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0))))
    }

    /// Remove the typed projection registered under `key` (emit Cleared).
    /// Bridges `SnapshotProjectionRegistrar::remove_snapshot_projection`.
    pub fn remove_snapshot_projection(&self, key: &str) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.remove(key);
        }
    }

    /// Register a per-tick observer (no-result side-effect callback).
    /// Bridges `SnapshotProjectionRegistrar::register_snapshot_tick_observer`.
    pub fn register_snapshot_tick_observer<F>(&self, f: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.register_tick_observer(f);
        }
    }

    // ── Observer-slot bridges (&self, via Arc<Mutex<ObserverInner>>) ──────────

    /// Unregister an event observer by id (Rust or C-ABI).
    /// Bridges `EventObserverRegistrar::unregister_event_observer`.
    pub fn unregister_event_observer(&self, id: KernelEventObserverId) {
        unregister_observer(&self.observer_slot, id);
    }

    // ── Ingest-dispatcher bridges (&self, via Arc<RwLock<EventIngestDispatcher>>) ──

    /// Slot-keyed replace for a kind's ingest parser.
    /// Bridges `IngestParserRegistrar::replace_ingest_parser`.
    pub fn replace_ingest_parser(
        &self,
        kind: u32,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        let slot = self.kernel.ingest_dispatcher_slot();
        let Ok(mut d) = slot.write() else {
            return None;
        };
        d.replace_kind_parser(kind, slot_key, parser)
    }

    /// Remove the ingest parser registered under `slot_key` for `kind`.
    /// Bridges `IngestParserRegistrar::unregister_ingest_parser`.
    pub fn unregister_ingest_parser(&self, kind: u32, slot_key: &'static str) {
        let slot = self.kernel.ingest_dispatcher_slot();
        let Ok(mut d) = slot.write() else {
            return;
        };
        d.remove_kind_parser_slot(kind, slot_key);
    }

    /// Slot-keyed replace for a kind range's ingest parser.
    /// Bridges `IngestParserRegistrar::replace_ingest_parser_range`.
    pub fn replace_ingest_parser_range(
        &self,
        range: Range<u32>,
        slot_key: &'static str,
        parser: Arc<dyn IngestParser>,
    ) -> Option<Arc<dyn IngestParser>> {
        let slot = self.kernel.ingest_dispatcher_slot();
        let Ok(mut d) = slot.write() else {
            return None;
        };
        d.replace_range_parser(range, slot_key, parser)
    }

    /// Remove the range-parser registered under `slot_key`.
    /// Bridges `IngestParserRegistrar::unregister_ingest_parser_range`.
    pub fn unregister_ingest_parser_range(&self, slot_key: &'static str) {
        let slot = self.kernel.ingest_dispatcher_slot();
        let Ok(mut d) = slot.write() else {
            return;
        };
        d.remove_range_parser_slot(slot_key);
    }
}
