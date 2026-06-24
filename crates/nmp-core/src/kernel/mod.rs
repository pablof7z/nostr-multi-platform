//! Kernel — the actor-owned event-processing core.

pub(crate) mod action_registry;
mod composition_accessors;
pub mod composition_ledger;
mod composition_seams;
#[cfg(test)] mod action_failure_tests;
#[cfg(test)] mod action_terminal_correctness_tests;
pub(crate) mod action_ledger;
#[cfg(test)] mod action_lifecycle_kernel_tests;
pub(crate) mod action_stages;
#[cfg(test)] mod action_stages_tests;
#[cfg(test)] mod cancel_correlation_tests;
#[cfg(test)] mod publish_completion_forget_tests; // D8 — forget handle↔correlation on completion (S7/#1754)
pub(crate) mod handle_correlation; // handle ↔ dispatch-correlation_id (S7, #1754)
mod relay_list_substrate;
pub(crate) use relay_list_substrate::parse_relay_list_to_substrate;
#[cfg(test)] mod signed_events_return_tests;
mod active_timeline_authors;
#[cfg(test)] mod active_timeline_authors_tests;
mod auth;
mod auth_sign_state;
pub(crate) mod clock;
#[cfg(test)] mod clock_injection_tests;
#[cfg(test)] mod closed_classifier_tests;
#[cfg(test)] mod gc_step_tests;
mod ram_eviction;
#[cfg(test)] mod ram_eviction_tests;
#[cfg(test)] mod ram_eviction_view_pin_tests;
pub(crate) mod claim_expansion;
#[cfg(test)] mod claim_expansion_edge_tests;
mod claim_expansion_helpers;
#[cfg(test)] mod claim_expansion_ingest_tests;
#[cfg(any(test, feature = "test-support"))] mod claim_expansion_seam;
#[cfg(test)] mod claim_expansion_tests;
#[cfg(test)] mod claim_expansion_tick_tests;
#[cfg(test)] mod claimed_events_raw_author_tests;
pub(crate) mod cache_serve;
pub(crate) mod pull;
pub mod pull_cursor; // ADR-0058 §3a — non-durable pull-cursor registry + actor commands.
pub(crate) mod pull_wake;
/// ADR-0054 §X — KernelPorts facade: 10 typed port newtypes (#1721 slice 1).
pub mod kernel_ports;
#[cfg(test)] mod pull_cursor_wake_tests;
#[cfg(test)] mod pull_tests;
mod store_wakeup;
#[cfg(test)] mod cache_serve_all_kinds_dispatcher_tests;
#[cfg(test)] mod cache_serve_budget_tests;
#[cfg(test)] mod cache_serve_coverage_tests;
#[cfg(test)] mod cache_serve_tests;
#[cfg(test)] mod cache_serve_universal_tests;
#[cfg(test)] mod cache_serve_wakeup_tests;
pub(crate) mod closed_reason;
#[cfg(test)] mod pull_cursor_retention_tests;
#[cfg(test)] mod chokepoint_tests;
mod coverage_ledger;
#[cfg(test)] mod coverage_ledger_d1_tests;
#[cfg(test)] mod coverage_ledger_d2_tests;
mod diagnostic_counters;
mod discovery;
#[cfg(test)] mod discovery_tests;
/// ADR-0052 §D5 — `&mut Kernel` → narrow wallet/zap capability adapter.
pub mod wallet_access;
#[cfg(all(test, feature = "native"))] mod coverage_ledger_d2_journey_tests;
#[cfg(test)] mod eose_ok_notice_ingest_tests;
#[cfg(test)] mod event_claim_tests;
#[cfg(test)] mod event_claim_hint_tests;
#[cfg(any(test, feature = "test-support"))] mod interest_install_cache_serve_support;
#[cfg(test)] mod interest_install_cache_serve_tests;
pub(crate) mod event_claim_released; // V-59 rung 1 — event-claim released observer ring.
#[cfg(test)] mod event_claim_released_tests;
mod event_observer;
#[cfg(test)] mod event_observer_tests;
mod observer_replay; // ADR-0062 — observer-scoped read-model catch-up.
pub(crate) use observer_replay::ObserverReplayRequest;
#[cfg(test)] mod observer_replay_tests;
#[cfg(test)] mod observer_replay_store_tests;
mod identity_state;
mod ingest;
#[cfg(test)] mod ingest_pre_verified_dispatcher_tests;
#[cfg(test)] mod ingest_tests;
#[cfg(test)] mod ingest_timeline_dispatcher_tests;
mod lifecycle;
mod lifecycle_drain;
mod mailboxes;
#[cfg(any(test, feature = "test-support"))] mod negentropy_test_support;
mod negentropy_types;
mod nostr;
#[cfg(test)] mod outbox_tests;
#[cfg(test)] mod proactive_profile_fetch_tests;
#[cfg(test)] mod profile_claim_discovery_tests;
#[cfg(test)] mod profile_claim_test_support;
#[cfg(test)] mod profile_claim_tests;
mod provenance;
#[cfg(test)] mod provenance_wire_tests;
mod publish_cmd;
mod publish_cmd_contact_accessors;
mod publish_engine;
mod publish_verify;
#[cfg(test)] mod publish_engine_tests;
mod publish_engine_wire;
mod publish_outbox;
#[cfg(test)] mod publish_relay_identity_tests;
#[cfg(test)] mod publish_terminal_status_tests;
mod relay_diagnostics;
mod relay_transport;
pub mod routing_trace; // V-51 — bounded ring-buffer projection of recent routing decisions.
pub mod routing_trace_dto; // V-51 — JSON DTO renderer for the routing-trace projection.
mod relay_frame;
mod relay_projection;
pub mod relay_score;
#[cfg(test)] mod relay_score_tests;
pub mod replaceable_ttl;
mod external_event_sink;
mod relay_score_flush;
mod relay_score_lookup_impl;
mod relay_score_record;
#[cfg(test)] mod replaceable_ttl_gate_tests;
mod replay;
#[cfg(test)] mod replay_tests;
mod requests;
pub use requests::ProfileLiveness;
pub(crate) mod refs; // ADR-0063 (#1671) — kernel RefResolver.
pub use refs::{
    EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape,
};
mod ref_row_source;
mod feed_author_refs;
#[cfg(test)] mod refs_tests;
#[cfg(test)] mod retention_tests;
#[cfg(test)] mod d1_offline_bootstrap_tests;
#[cfg(test)] mod dm_inbox_routing_tests;
#[cfg(test)] mod perf_tests;
/// ADR-0055 Rung 1 — kernel-owned per-projection revision manifest.
pub(crate) mod projection_rev;
pub(crate) mod snapshot_registry;
#[cfg(test)] mod snapshot_registry_tests;
#[cfg(test)] mod state_projection_tests;
mod status;
mod store_init;
#[cfg(test)] mod t140_m1_retirement_tests;
#[cfg(test)] mod t140_m2_follow_feed_tests;
#[cfg(test)] mod t142_drain_lifecycle_tick_tests;
#[cfg(test)] mod t170_relay_scoped_keying_tests;
#[cfg(test)] mod t171_planner_error_projection_tests;
#[cfg(test)] mod test_router;
#[cfg(any(test, feature = "test-support"))] mod test_support;
#[cfg(test)] mod tests;
mod tier3_encode;
#[cfg(test)] mod tier3_envelope_tests;
#[cfg(test)] mod tier3_negentropy_tests;
#[cfg(test)] mod timeline_order_tests;
#[cfg(test)] mod timeline_perf_tests;
/// Tier-2 kernel-owned typed-projection codecs + `make_update` wiring (ADR-0037).
mod typed_projections;
#[cfg(test)] mod typed_projections_tests;
#[cfg(test)] mod typed_projections_wave_c_diagnostics_tests;
#[cfg(test)] mod typed_projections_wave_c_tests;
mod types;
mod update;
mod wire_sub; // `WireSub` row (moved out of `types.rs` for the LOC cap).
pub use update::KERNEL_BUILTIN_PROJECTION_KEYS;
#[cfg(any(test, feature = "test-support"))] pub use update::{PROCESS_PROJECTIONS_CHANGED, PROCESS_PROJECTIONS_SERIALIZED};

/// Process-lifetime LRU-eviction counter for the durable store (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub static PROCESS_STORE_LRU_EVICTED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Re-export the RAM-tier eviction counter from `ram_eviction`.
#[cfg(any(test, feature = "test-support"))] pub use ram_eviction::PROCESS_RAM_EVENTS_EVICTED;
#[cfg(test)] mod v66_no_configured_relays_tests;
#[cfg(test)] mod v67_store_open_failure_tests;
pub(crate) mod wire_log;
#[cfg(test)] mod wire_log_callsite_tests;
#[cfg(test)] mod wire_log_tests;

#[cfg(test)] mod auth_fail_closed_tests;
#[cfg(test)] mod auth_test_helpers;
#[cfg(test)] mod auth_tests;
#[cfg(test)] mod auth_url_threading_tests;
#[cfg(test)] mod bookmark_cold_start_tests;
#[cfg(test)] mod contacts_chokepoint_pr3_tests;
#[cfg(test)] mod contacts_fanout_tests;
#[cfg(test)] mod mute_cold_start_tests;

mod kernel_misc;
pub(crate) use kernel_misc::{
    hex_to_pubkey_bytes, BackoffHint, RelayAuthCredentials, INFLIGHT_GUARD_MS,
    MAX_CLAIMS_PER_PUBKEY, MAX_EVENT_CLAIMS_PER_KEY,
};
mod kernel_clock_gc;
mod kernel_handles;
mod kernel_interest_api;
mod kernel_new;
mod kernel_relay_config;
mod relay_score_kernel;
mod replaceable_ttl_kernel;

use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole, DEFAULT_EMIT_HZ};
#[cfg(feature = "native")] use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;
use crate::time::{Duration, Instant, UNIX_EPOCH};
use crate::time::SystemTime;
pub use relay_frame::RelayFrame;

/// Public decode surface for the typed-projection sidecar (re-exported at the crate root as `nmp_core::typed_projections`).
pub mod public_typed_projections;

use nostr::{ratio, short_hex, truncate, NostrEvent};
#[cfg(feature = "native")] use nostr::now_hms;
pub use nostr::{is_hex_id, is_hex_pubkey};

use crate::store::EventStore;
use crate::subs::{CompileTrigger, OneshotApi, SubscriptionLifecycle, UnknownIds};
use auth::AuthDriverState;
pub use auth::AuthSignerFn;
pub use auth_sign_state::PendingAuthSign;
use clock::SystemClock;
pub use clock::Clock;
#[cfg(any(test, feature = "test-support"))] pub use clock::MonotonicSecondClock;
pub use action_registry::{default_registry, ActionRegistry, RegistrationError};
#[cfg(feature = "native")] pub use action_registry::{ActionExecuteFailure, ActionFailureKind};
pub use composition_ledger::{
    CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION,
};
pub(crate) use identity_state::{AccountSummary, PublishQueueEntry, RelayAckOutcome};
pub use identity_state::{new_active_account_slot, ActiveAccountSlot};
#[cfg(feature = "codegen-schema")] pub(crate) use types::LogicalInterestStatus as LogicalInterestStatusForCodegen;
#[cfg(feature = "codegen-schema")] pub(crate) use types::Metrics as MetricsForCodegen;
#[cfg(feature = "codegen-schema")] pub(crate) use types::RelayStatus as RelayStatusForCodegen;
pub use identity_state::{read_eligible_relay_urls, AppRelay};
#[cfg(feature = "codegen-schema")] pub(crate) use types::TimelineItem as TimelineItemForCodegen;
#[cfg(feature = "codegen-schema")] pub(crate) use types::WireSubscriptionStatus as WireSubscriptionStatusForCodegen;
pub use snapshot_registry::new_snapshot_projection_slot;
pub use snapshot_registry::SnapshotProjectionSlot;
pub use snapshot_registry::{record_emitted_feed_authors, EmittedFeedAuthorsSlot}; // ADR-0063 D7
pub use relay_projection::{AppRelayList, AppRelaySlot};
pub use relay_projection::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
#[cfg(feature = "native")] pub use relay_projection::new_app_relay_slot;
pub use lifecycle::LifecyclePhase;
pub(crate) use lifecycle::LifecycleTransition;
#[cfg(not(any(test, feature = "test-support")))] use crate::substrate::EmptyMailboxCache;
#[cfg(any(test, feature = "test-support"))] use crate::substrate::TestInMemoryMailboxCache;
use crate::substrate::{
    empty_blocked_relay_lookup, empty_dm_inbox_relay_lookup, BlockedRelayLookup, ContactsLookup,
    DmInboxRelayLookup, EmptyOutboxRouter, EventIngestDispatcher, MailboxCache, OutboxRouter,
    ParsedRelayList, ProfileLookup, MAX_PROJECTION_MESSAGES,
};
#[cfg(not(any(test, feature = "test-support")))] use crate::substrate::empty_contacts_lookup;
#[cfg(not(any(test, feature = "test-support")))] use crate::substrate::empty_profile_lookup;
use crate::util::sort_dedup;
use relay_transport::RelayTransportMap;
use std::sync::atomic::AtomicU64;
pub(crate) use types::KernelSnapshot;
#[cfg(test)] use types::TimelineItem;
use types::{
    ClaimedEventDto, Counters, DiagnosticFirehoseState, LogicalInterestStatus,
    Metrics, NoticeEntry, OutboxSummarySnapshot, ProfileCard,
    PublishOutboxItem, PublishOutboxRelay, RelayHealth, RelayStatus, StoredEvent, TimingMilestones,
    WireSub, WireSubscriptionState, WireSubscriptionStatus, MAX_NOTICE_LOG,
};

/// The kernel owns all Nostr protocol state for the active app session.
///
/// Driven by the actor loop through `handle_message` / `open_*` / `close_*` / `emit`.
/// `EventStore` (`self.store`) is the single authoritative writer for persisted events (D4).
pub struct Kernel {
    /// Pluggable event store (D4 single writer; `Arc` for sharing with the outbox resolver).
    store: Arc<dyn EventStore>,
    /// Injectable wall-clock; tests swap in `FixedClock` for determinism (D9).
    clock: Arc<dyn Clock>,
    rev: u64,
    visible_limit: usize,
    /// ADR-0055 per-projection revision tracker (Rung 2/3 stamp/omit).
    pub(crate) projection_rev_tracker: projection_rev::ProjectionRevTracker,
    /// ADR-0063 row-delta producer state for `typed_projections::builtins_refs`.
    ref_row_delta_tracker: crate::refs::RefRowDeltaTracker,
    ref_row_last_identity: Option<(u64, u64)>,
    ref_row_last_permits: (bool, bool),
    /// ADR-0055 biconditional completeness oracle (test/test-support only; zero cost in production).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) projection_oracle: projection_rev::oracle::OracleState,
    /// Test-support GC budget ceiling; `None` = production default (LRU disabled).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) gc_budget_ceiling: Option<usize>,
    /// FFI diagnostic timing milestones.
    timing: TimingMilestones,
    relays: HashMap<RelayRole, RelayHealth>,
    transport_relays: RelayTransportMap,
    /// Kind:0 profile lookup substrate (D0, ADR-0057 PR 2).
    profile_lookup: Arc<dyn ProfileLookup>,
    events: HashMap<String, StoredEvent>,
    /// Count of cached kind:1 events ever inserted into `events`.
    metric_note_events: u64,
    /// Count of events whose `relay_count` transitioned 1→>1 (duplicate delivery).
    metric_duplicate_events: u64,
    /// Tracks `events.len()`; incremented on insert, decremented on eviction.
    metric_stored_events: u64,
    /// Memoized store-byte estimate; invalidated on each store mutation (`Cell` for `&self`).
    cached_estimated_store_bytes: std::cell::Cell<Option<usize>>,
    timeline: VecDeque<String>,
    /// Diagnostic firehose tracking state.
    diagnostic_firehose: DiagnosticFirehoseState,
    deferred_outbound: VecDeque<OutboundMessage>,
    /// V-58 one-shot backoff hints drained by the actor after each `handle_message`.
    pending_backoff_hints: Vec<(String, BackoffHint)>,
    /// Kind:3 contact-list lookup substrate (D0, ADR-0057 PR 3).
    contacts_lookup: Arc<dyn ContactsLookup>,
    /// NIP-65 kind:10002 mailbox cache substrate (crate-boundaries step 3).
    mailbox_cache: Arc<dyn MailboxCache>,
    /// Outbox router substrate (crate-boundaries step 3).
    outbox_router: Arc<dyn OutboxRouter>,
    /// Injected content parser (D0 — no NIP noun in `nmp-core`).
    content_parser: Arc<dyn crate::substrate::ContentParser>,
    /// V-51 routing-trace ring-buffer projection.
    routing_trace: Arc<routing_trace::RoutingTraceProjection>,
    /// NIP-17 DM-inbox relay lookup substrate (D0, V-40).
    dm_inbox_relays: Arc<dyn DmInboxRelayLookup>,
    /// Blocked-relay lookup substrate (D0, V-40).
    blocked_relays: Arc<dyn BlockedRelayLookup>,
    /// Per-app override for the active-account bootstrap self-kinds list.
    bootstrap_self_kinds_override: Option<Vec<u32>>,
    /// Per-NIP ingest parser registry (ADR-0057, V-40).
    ingest_dispatcher: Arc<std::sync::RwLock<EventIngestDispatcher>>,
    /// Test-only handle to `TestDmInboxRelayCache`.
    #[cfg(any(test, feature = "test-support"))]
    test_dm_inbox_cache: Option<Arc<crate::substrate::TestDmInboxRelayCache>>,
    /// Test-only handle to `TestProfileCache` (backs `profile_lookup` in test builds).
    #[cfg(any(test, feature = "test-support"))]
    test_profile_cache: Arc<crate::substrate::TestProfileCache>,
    /// Test-only handle to `TestContactsCache` (backs `contacts_lookup` in test builds).
    #[cfg(any(test, feature = "test-support"))]
    test_contacts_cache: Arc<crate::substrate::TestContactsCache>,
    pub(crate) timeline_authors: BTreeSet<String>,
    /// T140 M2 — currently-registered follow-feed interest IDs.
    pub(crate) follow_feed_interest_ids: BTreeSet<crate::planner::InterestId>,
    /// Compiled acquisition kinds for the active-follows subscription.
    pub(crate) follow_feed_kinds: BTreeSet<u32>,
    /// pubkey → consumer-id refcount (profile claims).
    profile_claims: HashMap<String, BTreeSet<String>>,
    /// ADR-0063 pubkey → Live-liveness consumer-id set.
    live_profile_claims: HashMap<String, BTreeSet<String>>,
    /// ADR-0063 primary_id → Live-liveness consumer-id set.
    live_event_claims: HashMap<String, BTreeSet<String>>,
    /// ADR-0063 pubkey → per-consumer demanded `ProfileShape`.
    ref_profile_shapes: HashMap<String, BTreeMap<String, refs::ProfileShape>>,
    /// ADR-0063 primary_id → per-consumer demanded `EventShape`.
    ref_event_shapes: HashMap<String, BTreeMap<String, refs::EventShape>>,
    auto_profile_refs_by_consumer: BTreeMap<String, BTreeSet<String>>, // ADR-0063 D7 (Lane H)
    /// primary_id → consumer-id refcount (event claims, F-CR-06 / ADR-0034).
    event_claims: HashMap<String, BTreeSet<String>>,
    /// primary_ids with an in-flight `OneshotApi` interest.
    event_claim_requested: BTreeSet<String>,
    /// V-59 ring of primary_ids whose claim resolved to EOSE-without-match.
    event_claim_released: crate::substrate::BoundedRing<String>,
    /// In-process observers notified on each `event_claim_released` push.
    event_claim_released_observers: Vec<Arc<dyn event_claim_released::EventClaimReleasedObserver>>,
    /// Cold-start parking queue for event refs awaiting a relay connection.
    pub(in crate::kernel) pending_event_claims: Vec<requests::PendingEventClaim>,
    /// Counter for event-ref drops due to `MAX_EVENT_CLAIMS_PER_KEY`.
    event_claim_drops_total: u64,
    timeline_requested: bool,
    contacts_deadline: Option<Instant>,
    /// Wire subscription bookkeeping (keyed by `(relay_url, sub_id)`).
    wire: WireSubscriptionState,
    update_sequence: u64,
    /// Serialized length of the previous tick's snapshot (diagnostic, lags one tick).
    last_payload_bytes: usize,
    last_make_update_us: u128,
    last_serialize_us: u128,
    update_frame_degradations_total: u64,
    events_since_last_update: u64,
    max_event_to_emit_ms: u128,
    max_events_per_update: u64,
    changed_since_emit: bool,
    logs: VecDeque<String>,
    /// Per-relay NIP-42 AUTH driver state.
    auth_drivers: HashMap<RelayRole, AuthDriverState>,
    /// Subscription lifecycle (compile / registry / wire-diff machinery).
    lifecycle: SubscriptionLifecycle,
    unknown_ids: UnknownIds,
    oneshot: OneshotApi,
    /// T82 discovery wire sub_id → (token, kind) map.
    oneshot_subs: HashMap<String, (crate::subs::OneshotToken, discovery::OneshotKind)>,
    /// PD-033-C Stage 1 bridge: `InterestId` → `OneshotToken` for discovery oneshots.
    pending_discovery_oneshots: HashMap<crate::planner::InterestId, crate::subs::OneshotToken>,
    /// W5 per-claim Phase 1/2/3 state machine entries keyed by `InterestId`.
    pending_claims:
        std::collections::BTreeMap<crate::planner::InterestId, claim_expansion::PendingClaim>,
    /// W5 reverse index: wire sub_id → `InterestId`.
    claim_sub_index: std::collections::BTreeMap<String, crate::planner::InterestId>,
    /// Per-role NIP-42 signer credentials.
    auth_signers: HashMap<RelayRole, RelayAuthCredentials>,
    /// V-06 per-role remote-signer (NIP-46/NIP-55) pubkey.
    auth_remote_pubkeys: HashMap<RelayRole, String>,
    /// V-06 AUTH events awaiting a remote signature.
    pending_auth_signs: Vec<PendingAuthSign>,
    accounts: Vec<AccountSummary>,
    active_account: Option<String>,
    /// Sign-and-return results keyed by `correlation_id`; drain-on-emit.
    signed_events: HashMap<String, Result<String, String>>,
    publish_queue: Vec<PublishQueueEntry>,
    last_error_toast: Option<String>,
    /// Machine-readable category for `last_error_toast`.
    last_error_category: Option<String>,
    configured_relays: Vec<AppRelay>,
    /// Per-`correlation_id` publish/sign/cancel stage ledger (T117 / resolves #1684).
    action_ledger: action_ledger::ActionLedger,
    /// Durable handle→`correlation_id` index for cancel-by-id (S7/#1754, PD-036).
    publish_handle_correlation: handle_correlation::HandleCorrelationIndex,
    /// Per-tick captured drain-on-emit / wall-clock-sensitive projection values (Wave C).
    captured_action_results: Option<serde_json::Value>,
    captured_signed_events: Option<serde_json::Value>,
    captured_action_stages: Option<serde_json::Value>,
    captured_action_lifecycle: Option<serde_json::Value>,
    captured_relay_diagnostics: Option<relay_diagnostics::RelayDiagnosticsSnapshot>,
    /// Per-(event, relay) retry FSM for published events (T117).
    publish_engine: crate::publish::PublishEngine,
    /// Buffered `(relay_url, frame)` pairs produced by the publish engine.
    publish_dispatcher: Arc<crate::publish::QueueDispatcher>,
    /// Durable publish-state store.
    #[allow(dead_code)]
    publish_store: Arc<dyn crate::publish::PublishStore>,
    /// T131 per-URL novelty counters fed at the ingest chokepoint.
    pub(in crate::kernel) event_provenance: provenance::EventProvenance,
    /// Count of `resolve_ref` drops due to `MAX_CLAIMS_PER_PUBKEY` (T114b).
    claim_drops_total: u64,
    /// Actor command-channel depth (G-S4 backpressure metric; `None` outside the actor).
    queue_depth: Option<Arc<AtomicU64>>,
    /// Current iOS scenePhase (T118/G3).
    lifecycle_phase: LifecyclePhase,
    /// T146 kernel event observer slot.
    event_observers: Option<crate::actor::KernelEventObserverSlot>,
    /// External event sink dispatcher (D0 generic capability).
    external_event_sink_dispatcher: Option<crate::substrate::ExternalEventSinkDispatcher>,
    /// Host-extensible snapshot output slot.
    snapshot_projections: Option<SnapshotProjectionSlot>,
    /// Shared relay-edit rows slot for the FFI layer.
    configured_relays_handle: Option<AppRelaySlot>,
    /// Shared indexer relay URL list (D4 sole-writer: kernel actor).
    indexer_relays_handle: IndexerRelaysSlot,
    /// Shared local-write relay URLs for the active account.
    local_write_relays_handle: LocalWriteRelaysSlot,
    /// Shared active-account pubkey (D4 sole-writer: kernel actor).
    active_account_handle: ActiveAccountSlot,
    /// W2 in-memory relay-author score map (D4 sole writer).
    relay_score_map: relay_score::RelayAuthorScoreMap,
    /// W2 pluggable relay-author-score persistence store.
    relay_score_store: Option<Box<dyn crate::substrate::RelayAuthorScoreStore>>,
    /// F-TTL replaceable event freshness policy.
    replaceable_ttl: replaceable_ttl::ReplaceableTtlConfig,
    /// F-TTL re-verification queue for due replaceable identities.
    pending_reverify: VecDeque<crate::store::ReplaceableKey>,
    /// F-TTL in-flight reverification sub-id → keys map.
    reverify_subs: HashMap<String, Vec<crate::store::ReplaceableKey>>,
    /// F-TTL reverify-oneshot bridge: `InterestId` → `ReplaceableKey`s.
    pending_reverify_oneshots:
        HashMap<crate::planner::InterestId, Vec<crate::store::ReplaceableKey>>,
    /// V-67 store open failure reason (`None` = healthy or in-memory).
    store_open_failure: Option<String>,
    /// GAP-5 negentropy session statistics.
    negentropy_sync_stats: types::NegentropySyncStats,
    last_gc: Option<crate::store::GcReport>,
    last_gc_at_ms: Option<u64>,
    /// ADR-0045 E1 completion set for store-cache serve (one-shot per interest shape).
    pub(in crate::kernel) served_interest_shapes: HashSet<u64>,
    /// ADR-0045 §5 continuation queue for chunked store-cache serves.
    pub(in crate::kernel) pending_cache_serves: VecDeque<cache_serve::PendingCacheServe>,
    /// ADR-0058 §10 actor-owned store-wakeup subsystem (cache-serve re-arm + pull-cursor wakes).
    pub(in crate::kernel) store_wakeups: store_wakeup::StoreWakeups,
    /// ADR-0058 §3 non-durable pull-cursor registry.
    pub(in crate::kernel) pull_cursor_registry: pull_cursor::PullCursorRegistrySlot,
    snapshot_builder: flatbuffers::FlatBufferBuilder<'static>, // Rung 3 D3-6: reset+to_vec pattern
    /// Kernel must not cross thread boundaries — D4 single-writer enforced at type level.
    _not_send: PhantomData<*const ()>,
}
