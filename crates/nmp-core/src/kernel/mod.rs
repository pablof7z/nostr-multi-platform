//! Kernel — the actor-owned event-processing core.

pub(crate) mod action_ledger;
pub(crate) mod action_registry;
pub(crate) mod action_stages;
mod composition_accessors;
pub mod composition_ledger;
mod composition_seams;
pub(crate) mod handle_correlation; // handle ↔ dispatch-correlation_id (S7, #1754)
include!("test_modules.rs");
mod active_timeline_authors;
mod auth;
mod auth_sign_state;
pub(crate) mod cache_serve;
pub(crate) mod claim_expansion;
mod claim_expansion_helpers;
pub(crate) mod clock;
pub(crate) mod closed_reason;
mod coverage_ledger;
mod dependent_interests;
mod diagnostic_counters;
mod discovery;
pub(crate) mod event_claim_released; // V-59 rung 1 — event-claim released observer ring.
mod event_observer;
/// ADR-0054 §X — KernelPorts facade: 10 typed port newtypes (#1721 slice 1).
pub mod kernel_ports;
mod observer_replay;
pub(crate) mod pull;
pub mod pull_cursor; // ADR-0058 §3a — non-durable pull-cursor registry + actor commands.
pub(crate) mod pull_wake;
mod ram_eviction;
mod store_wakeup;
/// ADR-0052 §D5 — `&mut Kernel` → narrow wallet/zap capability adapter.
pub mod wallet_access; // ADR-0062 — observer-scoped read-model catch-up.
pub use dependent_interests::DependentInterestChild;
pub(crate) use observer_replay::ObserverReplayRequest;
mod external_event_sink;
mod identity_state;
mod ingest;
mod lifecycle;
mod lifecycle_drain;
mod mailboxes;
mod negentropy_types;
mod nostr;
mod provenance;
mod publish_cmd;
mod publish_cmd_contact_accessors;
mod publish_engine;
mod publish_engine_wire;
mod publish_outbox;
mod publish_verify;
mod relay_diagnostics;
mod relay_frame;
mod relay_projection;
pub mod relay_score;
mod relay_score_flush;
mod relay_score_lookup_impl;
mod relay_score_record;
mod relay_transport;
pub mod replaceable_ttl;
mod replay;
mod requests;
pub mod routing_trace; // V-51 — bounded ring-buffer projection of recent routing decisions.
pub mod routing_trace_dto; // V-51 — JSON DTO renderer for the routing-trace projection.
pub use requests::ProfileLiveness;
pub(crate) mod refs; // ADR-0063 (#1671) — kernel RefResolver.
pub use refs::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefResolveMetadata, RefShape};
mod feed_author_refs;
/// ADR-0055 Rung 1 — kernel-owned per-projection revision manifest.
pub(crate) mod projection_rev;
mod ref_row_source;
pub(crate) mod snapshot_registry;
mod status;
mod store_init;
mod tier3_encode;
#[cfg(test)]
mod tier3_envelope_tests;
#[cfg(test)]
mod tier3_negentropy_tests;
#[cfg(test)]
mod timeline_order_tests;
#[cfg(test)]
mod timeline_perf_tests;
/// Tier-2 kernel-owned typed-projection codecs + `make_update` wiring (ADR-0037).
mod typed_projections;
#[cfg(test)]
mod typed_projections_tests;
#[cfg(test)]
mod typed_projections_wave_c_diagnostics_tests;
#[cfg(test)]
mod typed_projections_wave_c_tests;
// #962 — the former `types.rs` god-file split into cohesive per-owner modules.
// `types.rs` remains as a re-export facade so `types::…` paths are unchanged.
mod claimed_event_dto; // `refs.event` claimed-event row payload.
mod kernel_snapshot; // Per-tick host update envelope + metrics/timing sub-state.
mod profile_card; // Raw kind:0 profile card.
mod publish_outbox_dto; // Publish-outbox projection DTOs.
mod read_cache; // Timeline read-cache entry (`StoredEvent`).
mod relay_health; // Per-relay transport health + wire-sub state + their projections.
mod types;
mod update;
mod wire_sub; // `WireSub` row (moved out of `types.rs` for the LOC cap).
pub use update::KERNEL_BUILTIN_PROJECTION_KEYS;
#[cfg(any(test, feature = "test-support"))]
pub use update::{PROCESS_PROJECTIONS_CHANGED, PROCESS_PROJECTIONS_SERIALIZED};

/// Process-lifetime LRU-eviction counter for the durable store (test-support only).
#[cfg(any(test, feature = "test-support"))]
pub static PROCESS_STORE_LRU_EVICTED: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Re-export the RAM-tier eviction counter from `ram_eviction`.
#[cfg(any(test, feature = "test-support"))]
pub use ram_eviction::PROCESS_RAM_EVENTS_EVICTED;
#[cfg(test)]
mod v66_no_configured_relays_tests;
#[cfg(test)]
mod v67_store_open_failure_tests;
pub(crate) mod wire_log;
#[cfg(test)]
mod wire_log_callsite_tests;
#[cfg(test)]
mod wire_log_tests;

#[cfg(test)]
mod auth_fail_closed_tests;
#[cfg(test)]
mod auth_test_helpers;
#[cfg(test)]
mod auth_tests;
#[cfg(test)]
mod auth_url_threading_tests;
#[cfg(test)]
mod bookmark_cold_start_tests;
#[cfg(test)]
mod mute_cold_start_tests;

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

use crate::relay::{CanonicalRelayUrl, OutboundMessage, DEFAULT_EMIT_HZ};
use crate::time::SystemTime;
use crate::time::{Duration, Instant, UNIX_EPOCH};
#[cfg(feature = "native")]
use chrono::{DateTime, Local};
use nmp_network::role::RelayRole;
pub use relay_frame::RelayFrame;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;

/// Public decode surface for the typed-projection sidecar (re-exported at the crate root as `nmp_core::typed_projections`).
pub mod public_typed_projections;

#[cfg(feature = "native")]
use nostr::now_hms;
pub use nostr::{is_hex_id, is_hex_pubkey};
use nostr::{ratio, short_hex, truncate, NostrEvent};

use crate::store::EventStore;
use crate::subs::{OneshotApi, SubscriptionLifecycle, UnknownIds};
#[cfg(not(any(test, feature = "test-support")))]
use crate::substrate::EmptyMailboxCache;
#[cfg(any(test, feature = "test-support"))]
use crate::substrate::TestInMemoryMailboxCache;
use crate::substrate::{
    empty_blocked_relay_lookup, empty_dm_inbox_relay_lookup, BlockedRelayLookup,
    DmInboxRelayLookup, EmptyOutboxRouter, EventIngestDispatcher, MailboxCache, OutboxRouter,
    ProfileLookup, MAX_PROJECTION_MESSAGES,
};
use crate::util::sort_dedup;
pub use action_registry::{default_registry, ActionRegistry, RegistrationError};
#[cfg(feature = "native")]
pub use action_registry::{ActionExecuteFailure, ActionFailureKind};
use auth::AuthDriverState;
pub use auth::AuthSignerFn;
pub use auth_sign_state::PendingAuthSign;
pub use clock::Clock;
#[cfg(any(test, feature = "test-support"))]
pub use clock::MonotonicSecondClock;
use clock::SystemClock;
pub use composition_ledger::{
    CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION,
};
pub use identity_state::{new_active_account_slot, ActiveAccountSlot};
pub use identity_state::{read_eligible_relay_urls, AppRelay};
pub(crate) use identity_state::{AccountSummary, PublishQueueEntry, RelayAckOutcome};
pub use lifecycle::LifecyclePhase;
pub(crate) use lifecycle::LifecycleTransition;
#[cfg(feature = "native")]
pub use relay_projection::new_app_relay_slot;
pub use relay_projection::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
pub use relay_projection::{AppRelayList, AppRelaySlot};
use relay_transport::RelayTransportMap;
pub use snapshot_registry::new_snapshot_projection_slot;
pub use snapshot_registry::SnapshotProjectionSlot;
pub use snapshot_registry::{record_emitted_feed_authors, EmittedFeedAuthorsSlot}; // ADR-0063 D7
use std::sync::atomic::AtomicU64;
pub(crate) use types::KernelSnapshot;
use types::{
    Counters, DiagnosticFirehoseState, LogicalInterestStatus, Metrics, NoticeEntry,
    OutboxSummarySnapshot, ProfileCard, PublishOutboxItem, PublishOutboxRelay, RelayHealth,
    RelayStatus, StoredEvent, TimingMilestones, WireSub, WireSubscriptionState,
    WireSubscriptionStatus, MAX_NOTICE_LOG,
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
    /// NIP-65 kind:10002 mailbox cache substrate (see crate-boundaries.md §3).
    mailbox_cache: Arc<dyn MailboxCache>,
    /// Test-only concrete mailbox cache handle for fixture seeding.
    #[cfg(any(test, feature = "test-support"))]
    test_mailbox_cache: Arc<TestInMemoryMailboxCache>,
    /// Outbox router substrate (see crate-boundaries.md §3).
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
    /// Substrate-generic outbound public tags appended by the publish policy
    /// to `PublicRoutable` events (the kernel names no NIP-89 noun — D0).
    outbound_public_tags: Vec<Vec<String>>,
    /// Per-NIP ingest parser registry (ADR-0057, V-40).
    ingest_dispatcher: Arc<std::sync::RwLock<EventIngestDispatcher>>,
    /// Test-only handle to `TestDmInboxRelayCache`.
    #[cfg(any(test, feature = "test-support"))]
    test_dm_inbox_cache: Option<Arc<crate::substrate::TestDmInboxRelayCache>>,
    /// Test-only handle to `TestProfileLookup` (backs `profile_lookup` in test builds).
    #[cfg(test)]
    test_profile_lookup: Arc<crate::substrate::TestProfileLookup>,
    pub(crate) timeline_authors: BTreeSet<String>,
    /// Source owner -> complete current set of child interests it produced.
    dependent_interest_sets: BTreeMap<
        crate::subs::SubOwnerKey,
        BTreeMap<crate::subs::SubIdentity, crate::planner::LogicalInterest>,
    >,
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
    publish_store: Arc<dyn crate::publish::PublishStore>,
    /// T131 per-URL novelty counters fed at the ingest chokepoint.
    pub(in crate::kernel) event_provenance: provenance::EventProvenance,
    /// Count of `resolve_ref` drops due to `MAX_CLAIMS_PER_PUBKEY` (T114b).
    claim_drops_total: u64,
    /// Actor command-channel depth (G-S4 backpressure metric; `None` outside the actor).
    queue_depth: Option<Arc<AtomicU64>>,
    /// Current iOS scenePhase (T118/G3).
    lifecycle_phase: LifecyclePhase,
    /// Declared observed-projection sink slot.
    event_observers: Option<crate::actor::ObservedProjectionSinkSlot>,
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
