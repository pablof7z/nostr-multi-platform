//! PR-4 composition seams for `KernelReducer`.
//!
//! These methods let a wasm32 composition root wire feed engines and protocol
//! projections into the kernel without depending on `NmpApp` (which lives in
//! `nmp-ffi`, not available on wasm32) or the native actor thread:
//!
//! * `open_observed_projection` — wire a declared scoped read-model sink.
//! * `register_typed_snapshot_projection` — wire a typed FlatBuffers projection.
//! * `register_feed_author_provider` — wire a feed's rendered-author provider.
//! * `active_account_handle` — read the active-account pubkey slot.
//! * `event_store_handle` — read the kernel event-store `Arc`.
//! * `indexer_relays_handle` / `local_write_relays_handle` — read the relay
//!   slots shared with the publish resolver.
//!
//! PR-B (#2046) AppHost-surface seams are in `composition_seams_browser.rs`
//! (factored out to stay under the 500-LOC ceiling).
//!
//! All methods delegate either to `self.kernel` (for slot handles that are
//! already `pub` there) or to `self.observer_slot` / `self.snapshot_slot`
//! (the per-reducer slots initialised in `KernelReducer::new`).
//!
//! # Doctrine
//!
//! * **D0** — surface types are all substrate-level: `Arc<dyn EventStore>`,
//!   `ActiveAccountSlot`, `ObservedProjection`, `TypedProjectionData`.
//!   No NIP or app nouns.
//! * **D6** — poisoned mutex on register/lookup is a silent no-op; the
//!   caller never panics.
//! * **D8** — all methods are O(n-observers) at worst; no I/O, no blocking.

use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use crate::actor::{register_rust_observer_muted, unregister_observer_internal};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::slots::{ActiveAccountSlot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use crate::store::EventStore;
use crate::substrate::{
    ContactsLookup, IngestParser, ObservedProjectionCommandHandle, ObservedProjectionSessionMap,
    ProfileLookup,
};
use crate::{EmittedFeedAuthorsSlot, ObservedProjectionId, TypedProjectionData};

impl super::KernelReducer {
    /// Construct a reducer around an externally-opened event store.
    ///
    /// The `EventStore` trait stays synchronous and unchanged; platform
    /// composition opens any platform-specific backend first, then injects the
    /// ready handle here. This is the accepted ADR-0054 Stage #5 seam for the
    /// future OPFS-SQLite wasm backend.
    #[must_use]
    pub fn with_store(store: Arc<dyn EventStore>) -> Self {
        use crate::actor::new_event_observer_slot_headless;
        use crate::kernel::new_snapshot_projection_slot;

        let observer_slot = new_event_observer_slot_headless();
        let snapshot_slot = new_snapshot_projection_slot();
        let mut kernel = Kernel::from_parts(DEFAULT_VISIBLE_LIMIT, store, None);
        kernel.set_event_observers_handle(Arc::clone(&observer_slot));
        kernel.set_snapshot_projection_handle(Arc::clone(&snapshot_slot));
        Self {
            kernel,
            observer_slot,
            snapshot_slot,
            observed_projection_sessions: std::collections::HashMap::new(),
            sign_roundtrip: super::wasm_signing::SignRoundTripState::default(),
        }
    }

    /// Rebuild the wrapped kernel around `store` while preserving the reducer's
    /// headless observer/projection slots.
    ///
    /// Called by `nmp-browser-runtime` at the top of `Start`, before relay
    /// drivers and runtime deadlines capture the reducer. The caller must only use this as
    /// a boot-time seam: swapping stores mid-session would fork publish,
    /// coverage, and query state across two backends.
    pub fn replace_store_for_start(&mut self, store: Arc<dyn EventStore>) {
        let mut kernel = Kernel::from_parts(DEFAULT_VISIBLE_LIMIT, store, None);
        kernel.set_event_observers_handle(Arc::clone(&self.observer_slot));
        kernel.set_snapshot_projection_handle(Arc::clone(&self.snapshot_slot));
        self.kernel = kernel;
        self.sign_roundtrip = super::wasm_signing::SignRoundTripState::default();
    }

    /// Record a degraded store-open failure reason onto the wrapped kernel so it
    /// surfaces through the Tier-3 `store_open_failure` snapshot channel (#1007
    /// PR-8 — the browser-durable analog of the native LMDB degraded-open path).
    ///
    /// Browser composition opens the OPFS-SQLite store asynchronously before the
    /// kernel exists; when that open fails the host falls back to in-memory and
    /// threads the stable reason here (via `BrowserAppBuilder::with_store_open_failure`)
    /// so the same diagnostic native emits at init reaches the snapshot.
    pub fn set_store_open_failure(&mut self, reason: impl Into<String>) {
        self.kernel.set_store_open_failure(reason);
    }

    /// Read the kernel's recorded store-open failure reason, if any (#1007 PR-8).
    #[cfg(any(test, feature = "test-support"))]
    pub fn store_open_failure(&self) -> Option<String> {
        self.kernel.store_open_failure().map(str::to_owned)
    }

    // ── Observed-projection seam ─────────────────────────────────────────

    /// Open a declared observed projection on the reducer/browser path.
    ///
    /// Mirrors `NmpApp::open_observed_projection`: register the sink muted,
    /// open the declared interest, replay matching cached rows, then activate
    /// future delivery scoped to the declaration's replay shapes.
    pub fn open_observed_projection(
        &mut self,
        decl: crate::substrate::ObservedProjection,
    ) -> ObservedProjectionId {
        if !decl.has_declared_shape() {
            return ObservedProjectionId(0);
        }
        let observer_id = register_rust_observer_muted(&self.observer_slot, decl.observer);
        if observer_id.0 == 0 {
            return observer_id;
        }
        let Some((identity, interest)) = crate::subs::interest_builder::build_interest_pair(
            &decl.filter_json,
            &decl.consumer_id,
            decl.scope,
            decl.relay_pin.as_deref(),
        ) else {
            unregister_observer_internal(&self.observer_slot, observer_id);
            return ObservedProjectionId(0);
        };
        self.observed_projection_sessions.insert(
            observer_id,
            (
                decl.filter_json.clone(),
                decl.consumer_id.clone(),
                decl.scope,
                decl.relay_pin.clone(),
            ),
        );
        let replay = crate::kernel::ObserverReplayRequest {
            observer_id,
            shapes: decl.replay_shapes,
            limit: decl.replay_limit,
        };
        let _ = self.kernel.open_interest_with_observer_replay(
            identity,
            interest,
            replay,
            "open-observed-projection",
        );
        let outbound = self.kernel.drain_lifecycle_outbound();
        let _ = self.kernel.partition_auth_paused(outbound);
        observer_id
    }

    /// Close a reducer/browser observed projection by id.
    pub fn close_observed_projection(&mut self, id: ObservedProjectionId) {
        let Some((filter_json, consumer_id, scope, relay_pin)) =
            self.observed_projection_sessions.remove(&id)
        else {
            return;
        };
        if let Some((identity, _interest)) = crate::subs::interest_builder::build_interest_pair(
            &filter_json,
            &consumer_id,
            scope,
            relay_pin.as_deref(),
        ) {
            let _ = self.kernel.close_interest_sub(&identity);
        }
        unregister_observer_internal(&self.observer_slot, id);
        let outbound = self.kernel.drain_lifecycle_outbound();
        let _ = self.kernel.partition_auth_paused(outbound);
    }

    /// Build a cloneable command-backed observed-projection registrar for
    /// post-start runtime controllers.
    #[must_use]
    pub fn observed_projection_command_handle(
        &self,
        sessions: ObservedProjectionSessionMap,
        sender: crate::CommandSender,
    ) -> ObservedProjectionCommandHandle {
        ObservedProjectionCommandHandle::new(Arc::clone(&self.observer_slot), sessions, sender)
    }

    // ── Typed snapshot-projection seam ───────────────────────────────────

    /// Register a typed FlatBuffers snapshot projection under `key`.
    ///
    /// The closure `f` is called once per `make_update_frame` tick (on the
    /// wasm32 path that is the 1 Hz timer + explicit snapshot pulls). It
    /// returns `Some(TypedProjectionData)` when there is a changed payload to
    /// emit, or `None` to omit an unchanged row for that tick. Under
    /// incremental apply, omission retains the host cache; unregistering the
    /// key emits an explicit `Cleared` row.
    ///
    /// This is the wasm32 equivalent of
    /// `NmpApp::register_typed_snapshot_projection`.
    pub fn register_typed_snapshot_projection(
        &self,
        key: impl Into<String>,
        f: impl Fn() -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.register_typed(key, f);
        }
        // Poisoned mutex: D6 silent fail. The projection simply never
        // appears in snapshots — same graceful-degrade as a missing
        // registration.
    }

    /// Register a typed projection closure that receives reducer/kernel time.
    pub fn register_typed_snapshot_projection_with_time(
        &self,
        key: impl Into<String>,
        f: impl Fn(u64) -> Option<TypedProjectionData> + Send + Sync + 'static,
    ) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.register_typed_with_time(key, f);
        }
    }

    /// Register the rendered-author provider for a feed projection.
    ///
    /// This is the reducer-owned twin of `NmpApp::register_feed_author_provider`.
    /// The caller should normally use a higher-level helper that pairs this
    /// provider with the feed's typed sidecar, so rendered author rows are always
    /// resolved through `refs.profile` in the same frame.
    pub fn register_feed_author_provider(
        &self,
        feed_key: impl Into<String>,
        f: impl Fn() -> Vec<String> + Send + Sync + 'static,
    ) {
        if let Ok(mut guard) = self.snapshot_slot.lock() {
            guard.register_feed_author_provider(feed_key, f);
        }
    }

    /// Return the registered feed-author-provider keys without running them.
    ///
    /// Used by composition tests to prove a typed feed sidecar is structurally
    /// paired with an author provider under the same key.
    #[must_use]
    pub fn registered_feed_author_provider_keys(&self) -> Vec<String> {
        self.snapshot_slot
            .lock()
            .map(|registry| {
                registry
                    .registered_feed_author_provider_keys()
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Run one feed-author provider by key for structural tests.
    #[must_use]
    pub fn run_feed_author_provider_for_test(&self, feed_key: &str) -> Vec<String> {
        self.snapshot_slot
            .lock()
            .map(|registry| registry.run_feed_author_provider(feed_key))
            .unwrap_or_default()
    }

    /// Return the registry handles needed by a paired feed render source.
    ///
    /// The typed producer records emitted authors through the sink while the
    /// registry mutex is held by `run_typed`, so it must capture the sink handle
    /// up front instead of re-locking the registry from inside the closure.
    #[must_use]
    pub fn feed_render_source_handles(&self) -> Option<(Arc<AtomicU64>, EmittedFeedAuthorsSlot)> {
        self.snapshot_slot.lock().ok().map(|registry| {
            (
                registry.frame_tick_rev_handle(),
                registry.emitted_feed_authors_handle(),
            )
        })
    }

    // ── Kernel handle pass-throughs ───────────────────────────────────────

    /// Return the kernel's active-account pubkey slot.
    ///
    /// The returned [`ActiveAccountSlot`] is `Arc<Mutex<Option<String>>>`.
    /// Composition roots pass it to `ActiveFollowSet::new` (rung 4) so the
    /// follow-set producer can seed itself and respond to account switches
    /// without holding a reference to the full reducer.
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        self.kernel.active_account_handle()
    }

    /// Return the kernel's event-store handle.
    ///
    /// Used by the composition root to build an `EventLookup` closure
    /// (`Arc<dyn Fn(&str) -> Option<KernelEvent> + Send + Sync>`) for
    /// `register_op_feed`.  The returned `Arc<dyn EventStore>` is `Send +
    /// Sync`, so the closure can be stored across ticks without holding a
    /// `KernelReducer` borrow.
    #[must_use]
    pub fn event_store_handle(&self) -> Arc<dyn EventStore> {
        self.kernel.event_store_handle()
    }

    /// Return the kernel-owned indexer relay slot.
    ///
    /// Production publish resolver composition passes this to
    /// `nmp_router::Nip65OutboxResolver` so discovery-kind publish fanout reads
    /// the same slot `set_configured_relays` writes.
    #[must_use]
    pub fn indexer_relays_handle(&self) -> IndexerRelaysSlot {
        self.kernel.indexer_relays_handle()
    }

    /// Return the kernel-owned local write relay slot.
    ///
    /// Production publish resolver composition passes this to
    /// `nmp_router::Nip65OutboxResolver` so active-account cold-start publish
    /// fallback reads the same role-filtered write set as native.
    #[must_use]
    pub fn local_write_relays_handle(&self) -> LocalWriteRelaysSlot {
        self.kernel.local_write_relays_handle()
    }

    /// Install the profile lookup used by kernel profile readers.
    ///
    /// Wasm composition roots cannot go through the native `AppHost`
    /// `set_profile_lookup` seam, so they use this method to share one
    /// profile cache between the kind:0 ingest parser and kernel readers.
    pub fn set_profile_lookup(&mut self, lookup: Arc<dyn ProfileLookup>) {
        self.kernel.set_profile_lookup(lookup);
    }

    /// Install the contacts lookup used by kernel follow-feed readers.
    ///
    /// Wasm composition roots cannot go through the native `AppHost`
    /// `set_contacts_lookup` seam, so they use this method to share one
    /// contacts cache between the kind:3 ingest parser and kernel readers.
    pub fn set_contacts_lookup(&mut self, lookup: Arc<dyn ContactsLookup>) {
        self.kernel.set_contacts_lookup(lookup);
    }

    /// Register a post-store ingest parser against the wrapped kernel.
    ///
    /// Mirrors `NmpApp::register_ingest_parser` for reducer-owned wasm
    /// compositions. Parsers registered here fire on the same
    /// `project_accepted_event` path as native ingest.
    pub fn register_ingest_parser(&self, kind: u32, parser: Arc<dyn IngestParser>) {
        self.kernel.register_ingest_parser(kind, parser);
    }

    /// Snapshot the active account's timeline-author projection.
    ///
    /// This is a reducer-owned wrapper around the kernel accessor so wasm
    /// composition tests can assert the follow-feed planner projection without
    /// reaching through private kernel state.
    #[must_use]
    pub fn active_timeline_authors(&self) -> Vec<String> {
        self.kernel.active_timeline_authors()
    }

    // ── Composition-root wiring (moved here for the LOC ceiling, #1753) ──────

    /// Populate the kernel's configured-relay lanes from a caller-supplied
    /// list of `(url, role)` pairs.
    ///
    /// Each `role` string is canonicalised via the kernel's own
    /// `canonical_relay_role` pass (same normalisation the native actor
    /// applies on every relay-edit write). [`crate::kernel::AppRelay`] is
    /// `pub(crate)`; external callers (e.g. `nmp-browser-runtime`) pass raw string
    /// pairs and let this method build the typed rows internally.
    ///
    /// Calling this before the first `make_update_frame` ensures the
    /// `relay_statuses` Tier-3 rows and the `configured_relays` typed
    /// projection both carry real URLs rather than empty defaults.
    pub fn set_configured_relays(&mut self, rows: Vec<(String, String)>) {
        use crate::kernel::AppRelay;
        let relay_rows: Vec<AppRelay> = rows
            .into_iter()
            .map(|(url, role)| AppRelay::new(url, role))
            .collect();
        self.kernel.set_configured_relays(relay_rows);
    }

    /// Install the production outbox-routing substrate on the wrapped kernel.
    ///
    /// The wasm32 composition has no `AppHost` / actor `set_routing_substrate`
    /// seam (that path is native-only), so the chirp-web composition root calls
    /// this directly to swap the kernel's default no-op
    /// [`crate::substrate::EmptyOutboxRouter`] for the production
    /// `nmp_router::GenericOutboxRouter` + a `MailboxCache`. Without this every
    /// outbox-direction REQ (kind:0 profile claims, kind:3 contacts, kind:10002
    /// NIP-65) silently resolves to no relays. Must be called before `Start`.
    pub fn set_routing(
        &mut self,
        router: Arc<dyn crate::substrate::OutboxRouter>,
        cache: Arc<dyn crate::substrate::MailboxCache>,
    ) {
        self.kernel.set_routing(router, cache);
    }

    /// Install a content parser on the wrapped kernel (wasm composition seam).
    pub fn set_content_parser(&mut self, parser: Arc<dyn crate::substrate::ContentParser>) {
        self.kernel.set_content_parser(parser);
    }

    /// Install the publish outbox resolver on the wrapped kernel.
    ///
    /// App composition roots call this to swap the kernel's default
    /// `NoopOutboxResolver` (which resolves zero relay targets) for the
    /// production router resolver. Without this, every `PublishTarget::Auto`
    /// resolves to no relays and fails closed with `NoTargets`.
    pub fn set_publish_resolver(&mut self, resolver: Arc<dyn crate::publish::OutboxResolver>) {
        self.kernel.set_publish_resolver(resolver);
    }

    /// Publish a pre-signed event through the kernel's publish engine (#1008).
    ///
    /// The wasm `dispatch_bytes` execute arm calls this for
    /// `ActorCommand::PublishSignedEvent` — the one `ActorCommand` variant a
    /// wasm context can handle inline (the event is already signed; no
    /// BeginSign round-trip is needed). Returns the outbound frames the relay
    /// pool must send.
    ///
    /// D6 — total: a malformed event / empty target / no-targets resolver
    /// verdict surfaces as a kernel toast + `RecentFailure` row in the publish
    /// engine, never a panic.
    pub fn publish_pre_signed(
        &mut self,
        raw: crate::store::RawEvent,
        target: crate::publish::PublishTarget,
        correlation_id: Option<String>,
    ) -> Vec<crate::relay::OutboundMessage> {
        // Delegates to the shared Kernel::publish_externally_signed helper
        // (#2045 PR-A): target-validate → verify-sig (closes forged-event gap)
        // → D10 routing gate → publish. Then partitions auth-paused frames.
        let outbound = self
            .kernel
            .publish_externally_signed(raw, target, correlation_id);
        self.kernel.partition_auth_paused(outbound)
    }
}

#[cfg(any(test, feature = "test-support"))]
impl super::KernelReducer {
    /// Test-only: seed the active account directly (no Identity command).
    ///
    /// Lets headless/browser-runtime tests reach the `NeedsSign` publish path
    /// (which requires an active account) without the native actor thread's
    /// roster machinery. Mirrors `Kernel::set_active_account_for_test`.
    pub fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.kernel.set_active_account_for_test(pubkey);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn project_raw_event_for_test(
        &mut self,
        id: &str,
        pubkey: &str,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: &str,
    ) {
        self.kernel
            .project_raw_event_for_test(id, pubkey, created_at, kind, tags, content);
    }
}
