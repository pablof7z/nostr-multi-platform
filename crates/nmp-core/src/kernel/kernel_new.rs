//! Kernel constructors + publish-resolver injection.
//!
//! Extracted from `kernel/mod.rs` (`impl Kernel`) to honour the 500-LOC ceiling.

use super::*;
#[cfg(not(any(test, feature = "test-support")))]
use crate::substrate::empty_profile_lookup;

impl Kernel {
    pub(crate) fn new(visible_limit: usize) -> Self {
        Self::with_storage_path(visible_limit, None)
    }

    /// Construct a Kernel, optionally backing the `EventStore` with a persistent LMDB path.
    pub fn with_storage_path(visible_limit: usize, storage_path: Option<&str>) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, None, storage_path)
    }

    /// Construct a Kernel from an externally-opened event store.
    ///
    /// This is the store-agnostic constructor used by non-native composition
    /// roots such as `nmp-browser-runtime`: async or platform-specific store open happens
    /// before the kernel exists, then the already-opened synchronous
    /// [`EventStore`](nmp_store::EventStore) is injected here. Native callers
    /// should keep using [`Self::with_storage_path`], which owns the LMDB
    /// path-resolution and degraded-open diagnostic contract.
    pub fn from_parts(
        visible_limit: usize,
        event_store: Arc<dyn nmp_store::EventStore>,
        store_open_failure: Option<String>,
    ) -> Self {
        Self::with_store_bundle_publish_store_path_and_account_slot(
            visible_limit,
            store_init::EventStoreBundle {
                store: event_store,
                relay_score_store: None,
            },
            store_open_failure,
            None,
            None,
            None,
        )
    }

    /// V-82 — like `with_storage_path` but threads in an externally-owned `ActiveAccountSlot`.
    #[must_use]
    pub fn with_storage_path_and_account_slot(
        visible_limit: usize,
        storage_path: Option<&str>,
        active_account_handle: ActiveAccountSlot,
    ) -> Self {
        Self::with_optional_publish_store_path_and_account_slot(
            visible_limit,
            None,
            storage_path,
            Some(active_account_handle),
        )
    }

    /// Inject the production outbox resolver on the `PublishEngine`.
    pub fn set_publish_resolver(&mut self, resolver: Arc<dyn crate::publish::OutboxResolver>) {
        self.publish_engine.set_outbox(resolver);
    }

    /// Test-support: construct with an externally-supplied publish store.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_publish_store(
        visible_limit: usize,
        publish_store: Arc<dyn crate::publish::PublishStore>,
    ) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, Some(publish_store), None)
    }

    fn with_optional_publish_store_and_path(
        visible_limit: usize,
        publish_store: Option<Arc<dyn crate::publish::PublishStore>>,
        storage_path: Option<&str>,
    ) -> Self {
        Self::with_optional_publish_store_path_and_account_slot(
            visible_limit,
            publish_store,
            storage_path,
            None,
        )
    }

    /// V-82 — innermost constructor; threads in an optional `ActiveAccountSlot`.
    fn with_optional_publish_store_path_and_account_slot(
        visible_limit: usize,
        publish_store: Option<Arc<dyn crate::publish::PublishStore>>,
        storage_path: Option<&str>,
        active_account_handle: Option<ActiveAccountSlot>,
    ) -> Self {
        let (store_bundle, store_open_failure) = store_init::build_event_store(storage_path);
        Self::with_store_bundle_publish_store_path_and_account_slot(
            visible_limit,
            store_bundle,
            store_open_failure,
            publish_store,
            storage_path,
            active_account_handle,
        )
    }

    fn with_store_bundle_publish_store_path_and_account_slot(
        visible_limit: usize,
        store_bundle: store_init::EventStoreBundle,
        store_open_failure: Option<String>,
        publish_store: Option<Arc<dyn crate::publish::PublishStore>>,
        storage_path: Option<&str>,
        active_account_handle: Option<ActiveAccountSlot>,
    ) -> Self {
        let store = store_bundle.store;
        let publish_store = publish_store
            .unwrap_or_else(|| store_init::resolve_publish_store(storage_path, &store));
        let publish_dispatcher = Arc::new(crate::publish::QueueDispatcher::new());
        // Typed-slot constructors so the slot's purpose is visible at
        // the call site and D14 does not fire on the field declaration.
        let indexer_relays_handle: IndexerRelaysSlot = new_indexer_relays_slot();
        let local_write_relays_handle: LocalWriteRelaysSlot = new_local_write_relays_slot();
        // V-82 — use the externally-supplied active-account slot when the actor
        // threads one in (so the FFI shell shares it); otherwise mint a fresh
        // one (every existing test / codegen caller). The local binding is the
        // single slot every downstream `Arc::clone` (the kernel field below, the
        // test-support outbox resolver) references — no divergent mirror.
        let active_account_handle: ActiveAccountSlot =
            active_account_handle.unwrap_or_else(new_active_account_slot);
        // Spec §271 (2026-05-25): `Nip65OutboxResolver` lives in
        // `nmp-router`, not `nmp-core`. The engine is built with the
        // in-crate `NoopOutboxResolver` default; production composition
        // (`explicit owner composition` → the
        // `set_publish_resolver_factory` slot the actor reads at
        // construction) swaps in the router-side resolver via
        // [`Kernel::set_publish_resolver`]. The `indexer_relays_handle`,
        // `local_write_relays_handle`, and `active_account_handle` slots
        // are still kernel-owned (the actor is the sole writer per D4) and
        // are surfaced through the kernel accessors below so the
        // router-side resolver constructor can wire them in.
        let publish_engine = publish_engine::build_engine(
            Arc::clone(&publish_dispatcher),
            Arc::clone(&publish_store),
        );

        // T129 + K3 (ADR-0072) — install the coverage-ledger since-floor resolver
        // on the subscription lifecycle. The closure (the ledger read) lives in
        // `coverage_ledger::build_watermark_fn` as the cohesive owner of the
        // floor logic (the file-size cap forbids growing this constructor).
        let watermark_fn: crate::subs::WatermarkFn =
            coverage_ledger::build_watermark_fn(Arc::clone(&store));
        let mut lifecycle = SubscriptionLifecycle::new();
        lifecycle.set_watermark_fn(watermark_fn);

        let clock: Arc<dyn Clock> = Arc::new(SystemClock);

        // V-51 phase 1 — construct the routing-trace projection. The kernel
        // hands this to production composition (via `routing_trace()` →
        // `RoutingSubstrateSlot` factory → `GenericOutboxRouter::with_trace_observer`)
        // so every routing decision the production router makes populates
        // the ring buffer the FFI snapshot surface + `chirp-repl routing-trace`
        // read from.
        //
        // Substrate-honest debt B (2026-05-24): the kernel's default
        // `outbox_router` slot used to hold an in-crate router that
        // duplicated `nmp_router::GenericOutboxRouter`'s algorithm
        // byte-for-byte (`nmp-core` could not depend on `nmp-router` so the
        // only way to keep a routing default was to copy the algorithm). The
        // duplicate is deleted: the default is now `EmptyOutboxRouter`
        // (always returns `Unroutable`). Every production composition
        // installs a real router via `NmpApp::set_routing_substrate` before
        // the kernel issues any routing decision; tests that exercise real
        // routing call `Kernel::set_routing` directly. The default `mailbox_cache`
        // is similarly `EmptyMailboxCache` in production and a
        // `TestInMemoryMailboxCache` under `cfg(any(test, feature = "test-support"))`
        // so in-tree routing/publish tests can seed parsed mailbox facts
        // without depending on downstream `nmp-router` (layering).
        let routing_trace = Arc::new(routing_trace::RoutingTraceProjection::with_clock(
            Arc::clone(&clock),
        ));
        let outbox_router: Arc<dyn OutboxRouter> = Arc::new(EmptyOutboxRouter::new());
        let content_parser: Arc<dyn crate::substrate::ContentParser> =
            Arc::new(crate::substrate::NoopContentParser::new());
        let draft_builders = Arc::new(crate::substrate::DraftBuilderRegistry::new());
        #[cfg(any(test, feature = "test-support"))]
        let test_mailbox_cache = Arc::new(TestInMemoryMailboxCache::new());
        #[cfg(any(test, feature = "test-support"))]
        let mailbox_cache: Arc<dyn MailboxCache> =
            Arc::clone(&test_mailbox_cache) as Arc<dyn MailboxCache>;
        #[cfg(not(any(test, feature = "test-support")))]
        let mailbox_cache: Arc<dyn MailboxCache> = Arc::new(EmptyMailboxCache::new());

        // Spec §271 (2026-05-25): under `cfg(test)` / `feature="test-support"`
        // the kernel auto-installs the in-crate `TestKind10002OutboxResolver`
        // (a minimal parsed-mailbox reader) so the dozens of in-tree publish
        // tests (`publish_engine_tests`, `outbox_tests`, `action_failure_tests`,
        // `publish_terminal_status_tests`, `eose_ok_notice_ingest_tests`,
        // `actor::commands::tests`, `kernel::test_support::seed_kind10002_for_test`
        // consumers) keep working without each test calling
        // `Kernel::set_publish_resolver` manually. Production builds use the
        // `NoopOutboxResolver` default the engine was built with above; the
        // production composition site (`explicit owner composition`)
        // installs the full router-side `nmp_router::Nip65OutboxResolver`
        // via `NmpApp::set_publish_resolver_factory` →
        // `Kernel::set_publish_resolver` (D0 — `nmp-core` does not name
        // `nmp-router` in its production graph; a dev-dep on `nmp-router`
        // would form a feature-incompatible cycle with `nmp-router`'s own
        // dep on `nmp-core`).
        #[cfg(any(test, feature = "test-support"))]
        let test_publish_resolver: Arc<dyn crate::publish::OutboxResolver> = Arc::new(
            crate::publish::TestKind10002OutboxResolver::new(Arc::clone(&mailbox_cache))
                .with_local_relays(
                    Arc::clone(&local_write_relays_handle),
                    Arc::clone(&active_account_handle),
                ),
        );
        #[cfg(any(test, feature = "test-support"))]
        let mut publish_engine = publish_engine;
        #[cfg(any(test, feature = "test-support"))]
        publish_engine.set_outbox(test_publish_resolver);

        // Test / test-support profile lookup seeded directly by core tests. NIP-01
        // parser/cache semantics live in nmp-nip01; core only needs a reader seam.
        #[cfg(any(test, feature = "test-support"))]
        let test_profile_lookup = Arc::new(crate::substrate::TestProfileLookup::new());
        #[cfg(any(test, feature = "test-support"))]
        let test_contact_list_reader = Arc::new(test_support::TestStoreContactListReader::new(
            Arc::clone(&store),
        )) as Arc<dyn crate::slots::ContactListReader>;

        let mut kernel = Self {
            store,
            clock,
            rev: 0,
            visible_limit,
            // ADR-0070 Rung 1: default (all counters 0, epoch 0); free on Reset.
            projection_rev_tracker: projection_rev::ProjectionRevTracker::default(),
            // ADR-0070 owns typed read output delivery; the ref-row tracker is
            // current glue for that path.
            ref_row_delta_tracker: crate::refs::RefRowDeltaTracker::default(),
            ref_row_last_identity: None,
            ref_row_last_permits: (false, false),
            #[cfg(any(test, feature = "test-support"))]
            projection_oracle: projection_rev::oracle::OracleState::default(),
            timing: TimingMilestones::default(),
            relays: RelayRole::all()
                .into_iter()
                .map(|role| (role, RelayHealth::default()))
                .collect(),
            transport_relays: RelayTransportMap::default(),
            // Production starts cold (empty lookup); apps inject
            // `nmp_nip01::ProfileCache` via `set_profile_lookup`. Test /
            // test-support builds default to a seed-only `TestProfileLookup` so
            // in-crate tests can seed + read profiles without depending on
            // `nmp-nip01`.
            #[cfg(not(any(test, feature = "test-support")))]
            profile_lookup: empty_profile_lookup(),
            #[cfg(any(test, feature = "test-support"))]
            profile_lookup: Arc::clone(&test_profile_lookup) as Arc<dyn ProfileLookup>,
            events: HashMap::new(),
            metric_note_events: 0,
            metric_duplicate_events: 0,
            metric_stored_events: 0,
            cached_estimated_store_bytes: std::cell::Cell::new(None),
            timeline: VecDeque::new(),
            diagnostic_firehose: DiagnosticFirehoseState::default(),
            deferred_outbound: VecDeque::new(),
            pending_backoff_hints: Vec::new(),
            mailbox_cache,
            #[cfg(any(test, feature = "test-support"))]
            test_mailbox_cache,
            outbox_router,
            router_composed: false,
            content_parser,
            draft_builders,
            routing_trace,
            dm_inbox_relays: empty_dm_inbox_relay_lookup(),
            #[cfg(any(test, feature = "test-support"))]
            contact_list_reader: test_contact_list_reader,
            #[cfg(not(any(test, feature = "test-support")))]
            contact_list_reader: crate::slots::empty_contact_list_reader(),
            blocked_relays: empty_blocked_relay_lookup(),
            external_id_validator: None,
            bootstrap_self_kinds_override: None,
            outbound_public_tags: Vec::new(),
            ingest_dispatcher: Arc::new(std::sync::RwLock::new(EventIngestDispatcher::new())),
            #[cfg(any(test, feature = "test-support"))]
            test_dm_inbox_cache: None,
            #[cfg(test)]
            test_profile_lookup,
            timeline_authors: BTreeSet::new(),
            profile_claims: HashMap::new(),
            live_profile_claims: HashMap::new(),
            live_event_claims: HashMap::new(),
            ref_profile_shapes: HashMap::new(),
            ref_event_shapes: HashMap::new(),
            auto_profile_refs_by_consumer: BTreeMap::new(),
            feed_author_reconciler: crate::trellis_reconciler::KeyedReconciler::new(
                feed_author_refs::FEED_AUTHOR_RECONCILER_SCOPE,
                feed_author_refs::feed_author_resource_key,
            )
            .expect("fresh KeyedReconciler construction over an empty graph cannot fail"), // doctrine-allow: D6 — construction over a brand-new empty graph before any transaction runs; `KeyedReconciler::new` can only fail on a Trellis-internal graph-build error, which is unreachable here (mirrors `nmp-read-session::open_read_demand_set`'s identical precedent, outside D6's enforced-crate scope)
            event_claims: HashMap::new(),
            event_claim_requested: BTreeSet::new(),
            event_claim_released: crate::substrate::BoundedRing::new(MAX_PROJECTION_MESSAGES),
            event_claim_released_observers: Vec::new(),
            pending_event_claims: Vec::new(),
            event_claim_drops_total: 0,
            timeline_requested: false,
            contacts_deadline: None,
            wire: WireSubscriptionState::default(),
            update_sequence: 0,
            last_payload_bytes: 0,
            last_make_update_us: 0,
            last_serialize_us: 0,
            update_frame_degradations_total: 0,
            events_since_last_update: 0,
            max_event_to_emit_ms: 0,
            max_events_per_update: 0,
            changed_since_emit: true,
            logs: VecDeque::new(),
            auth_drivers: RelayRole::all()
                .into_iter()
                .map(|role| (role, AuthDriverState::new()))
                .collect(),
            lifecycle,
            unknown_ids: UnknownIds::new(),
            oneshot: OneshotApi::new(),
            oneshot_subs: HashMap::new(),
            pending_discovery_oneshots: HashMap::new(),
            pending_claims: std::collections::BTreeMap::new(),
            claim_sub_index: std::collections::BTreeMap::new(),
            auth_signers: HashMap::new(),
            auth_remote_pubkeys: HashMap::new(),
            pending_auth_signs: Vec::new(),
            accounts: Vec::new(),
            active_account: None,
            signed_events: HashMap::new(),
            publish_queue: Vec::new(),
            last_error_toast: None,
            last_error_category: None,
            configured_relays: Vec::new(),
            action_ledger: action_ledger::ActionLedger::new(),
            publish_handle_correlation: handle_correlation::HandleCorrelationIndex::new(),
            captured_action_results: None,
            captured_signed_events: None,
            captured_action_stages: None,
            captured_action_lifecycle: None,
            captured_relay_diagnostics: None,
            publish_engine,
            publish_dispatcher,
            publish_store,
            event_provenance: provenance::EventProvenance::new(),
            claim_drops_total: 0,
            queue_depth: None,
            command_drops: None,
            relay_backlog_drops: None,
            external_event_sink_channel_overflow_drops: None,
            lifecycle_phase: LifecyclePhase::Inactive,
            event_observers: None,
            external_event_sink_dispatcher: None,
            snapshot_projections: None,
            configured_relays_handle: None,
            indexer_relays_handle,
            local_write_relays_handle,
            active_account_handle,
            relay_list_publish_support: crate::slots::empty_relay_list_publish_support(),
            relay_score_map: relay_score::RelayAuthorScoreMap::new(),
            relay_score_store: None,
            replaceable_ttl: replaceable_ttl::ReplaceableTtlConfig::default(),
            pending_reverify: VecDeque::new(),
            reverify_subs: HashMap::new(),
            pending_reverify_oneshots: HashMap::new(),
            store_open_failure,
            negentropy_sync_stats: types::NegentropySyncStats::default(),
            last_gc: None,
            last_gc_at_ms: None,
            #[cfg(any(test, feature = "test-support"))]
            gc_budget_ceiling: None,
            served_interest_shapes: HashSet::new(),
            pending_cache_serves: VecDeque::new(),
            store_wakeups: store_wakeup::StoreWakeups::new(),
            pull_cursor_registry: std::sync::Arc::new(std::sync::RwLock::new(
                pull_cursor::PullCursorRegistry::new(),
            )),
            snapshot_builder: flatbuffers::FlatBufferBuilder::new(), // ADR-0070 Rung 3 (D3-6)
            _not_send: PhantomData,
        };
        if let Some(store) = store_bundle.relay_score_store {
            kernel.set_relay_score_store(store);
        }
        kernel
    }
}
