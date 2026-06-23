//! Kernel — the actor-owned event-processing core.

pub(crate) mod action_registry;
mod composition_accessors;
pub mod composition_ledger;
mod composition_seams;
#[cfg(test)]
mod action_failure_tests;
#[cfg(test)]
mod action_terminal_correctness_tests;
pub(crate) mod action_ledger;
#[cfg(test)]
mod action_lifecycle_kernel_tests;
pub(crate) mod action_stages;
#[cfg(test)]
mod action_stages_tests;
#[cfg(test)]
mod cancel_correlation_tests;
#[cfg(test)]
mod publish_completion_forget_tests; // D8 — forget handle↔correlation on completion (S7/#1754)
pub(crate) mod handle_correlation; // handle ↔ dispatch-correlation_id (S7, #1754)
mod relay_list_substrate;
pub(crate) use relay_list_substrate::parse_relay_list_to_substrate;
#[cfg(test)]
mod signed_events_return_tests;
mod active_timeline_authors;
#[cfg(test)]
mod active_timeline_authors_tests;
mod auth;
mod auth_sign_state;
pub(crate) mod clock;
#[cfg(test)]
mod clock_injection_tests;
#[cfg(test)]
mod closed_classifier_tests;
#[cfg(test)]
mod gc_step_tests;
mod ram_eviction;
#[cfg(test)]
mod ram_eviction_tests;
#[cfg(test)]
mod ram_eviction_view_pin_tests;
pub(crate) mod claim_expansion;
#[cfg(test)]
mod claim_expansion_edge_tests;
mod claim_expansion_helpers;
#[cfg(test)]
mod claim_expansion_ingest_tests;
#[cfg(any(test, feature = "test-support"))]
mod claim_expansion_seam;
#[cfg(test)]
mod claim_expansion_tests;
#[cfg(test)]
mod claim_expansion_tick_tests;
#[cfg(test)]
mod claimed_events_raw_author_tests;
pub(crate) mod cache_serve;
pub(crate) mod pull;
pub mod pull_cursor; // ADR-0058 §3a — non-durable pull-cursor registry + actor commands.
pub(crate) mod pull_wake;
/// ADR-0054 §X — KernelPorts facade: 10 typed port newtypes (#1721 slice 1).
pub mod kernel_ports;
#[cfg(test)]
mod pull_cursor_wake_tests;
#[cfg(test)]
mod pull_tests;
mod store_wakeup;
#[cfg(test)]
mod cache_serve_all_kinds_dispatcher_tests;
#[cfg(test)]
mod cache_serve_budget_tests;
#[cfg(test)]
mod cache_serve_coverage_tests;
#[cfg(test)]
mod cache_serve_tests;
#[cfg(test)]
mod cache_serve_universal_tests;
#[cfg(test)]
mod cache_serve_wakeup_tests;
pub(crate) mod closed_reason;
#[cfg(test)]
mod pull_cursor_retention_tests;
#[cfg(test)]
mod chokepoint_tests;
mod coverage_ledger;
#[cfg(test)]
mod coverage_ledger_d1_tests;
#[cfg(test)]
mod coverage_ledger_d2_tests;
mod diagnostic_counters;
mod discovery;
#[cfg(test)]
mod discovery_tests;
/// ADR-0052 §D5 — `&mut Kernel` → narrow wallet/zap capability adapter.
pub mod wallet_access;
#[cfg(all(test, feature = "native"))]
mod coverage_ledger_d2_journey_tests;
#[cfg(test)]
mod eose_ok_notice_ingest_tests;
#[cfg(test)]
mod event_claim_tests;
#[cfg(any(test, feature = "test-support"))]
mod interest_install_cache_serve_support;
#[cfg(test)]
mod interest_install_cache_serve_tests;
pub(crate) mod event_claim_released; // V-59 rung 1 — event-claim released observer ring.
#[cfg(test)]
mod event_claim_released_tests;
mod event_observer;
#[cfg(test)]
mod event_observer_tests;
mod observer_replay; // ADR-0062 — observer-scoped read-model catch-up.
pub(crate) use observer_replay::ObserverReplayRequest;
#[cfg(test)]
mod observer_replay_tests;
#[cfg(test)]
mod observer_replay_store_tests;
mod identity_state;
mod ingest;
#[cfg(test)]
mod ingest_pre_verified_dispatcher_tests;
#[cfg(test)]
mod ingest_tests;
#[cfg(test)]
mod ingest_timeline_dispatcher_tests;
mod lifecycle;
mod lifecycle_drain;
mod mailboxes;
#[cfg(any(test, feature = "test-support"))]
mod negentropy_test_support;
mod negentropy_types;
mod nostr;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod proactive_profile_fetch_tests;
#[cfg(test)]
mod profile_claim_discovery_tests;
#[cfg(test)]
mod profile_claim_test_support;
#[cfg(test)]
mod profile_claim_tests;
mod provenance;
#[cfg(test)]
mod provenance_wire_tests;
mod publish_cmd;
mod publish_cmd_contact_accessors;
mod publish_engine;
mod publish_verify;
#[cfg(test)]
mod publish_engine_tests;
mod publish_engine_wire;
mod publish_outbox;
#[cfg(test)]
mod publish_relay_identity_tests;
#[cfg(test)]
mod publish_terminal_status_tests;
mod relay_diagnostics;
mod relay_transport;
pub mod routing_trace; // V-51 — bounded ring-buffer projection of recent routing decisions.
pub mod routing_trace_dto; // V-51 — JSON DTO renderer for the routing-trace projection.
mod relay_frame;
mod relay_projection;
pub mod relay_score;
#[cfg(test)]
mod relay_score_tests;
pub mod replaceable_ttl;
mod external_event_sink;
mod relay_score_flush;
mod relay_score_lookup_impl;
mod relay_score_record;
#[cfg(test)]
mod replaceable_ttl_gate_tests;
mod replay;
#[cfg(test)]
mod replay_tests;
mod requests;
pub use requests::ProfileLiveness;
pub(crate) mod refs; // ADR-0063 (#1671) — kernel RefResolver.
pub use refs::{EventShape, ProfileShape, RefLiveness, RefNamespace, RefShape};
mod ref_row_source;
mod feed_author_refs;
#[cfg(test)]
mod refs_tests;
#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod d1_offline_bootstrap_tests;
#[cfg(test)]
mod dm_inbox_routing_tests;
#[cfg(test)]
mod perf_tests;
/// ADR-0055 Rung 1 — kernel-owned per-projection revision manifest.
pub(crate) mod projection_rev;
pub(crate) mod snapshot_registry;
#[cfg(test)]
mod snapshot_registry_tests;
#[cfg(test)]
mod state_projection_tests;
mod status;
mod store_init;
#[cfg(test)]
mod t140_m1_retirement_tests;
#[cfg(test)]
mod t140_m2_follow_feed_tests;
#[cfg(test)]
mod t142_drain_lifecycle_tick_tests;
#[cfg(test)]
mod t170_relay_scoped_keying_tests;
#[cfg(test)]
mod t171_planner_error_projection_tests;
#[cfg(test)]
mod test_router;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
#[cfg(test)]
mod tests;
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
mod contacts_chokepoint_pr3_tests;
#[cfg(test)]
mod contacts_fanout_tests;
#[cfg(test)]
mod mute_cold_start_tests;

use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole, DEFAULT_EMIT_HZ};
#[cfg(feature = "native")]
use chrono::{DateTime, Local};
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
#[cfg(feature = "native")]
use nostr::now_hms;
pub use nostr::{is_hex_id, is_hex_pubkey};

/// Decode a 64-char hex pubkey into `[u8; 32]`. Returns `None` on malformed input (D6).
pub(crate) fn hex_to_pubkey_bytes(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16)? as u8;
        let lo = (chunk[1] as char).to_digit(16)? as u8;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

use crate::store::EventStore;
use crate::subs::{CompileTrigger, OneshotApi, SubscriptionLifecycle, UnknownIds};
use auth::AuthDriverState;
pub use auth::AuthSignerFn;
pub use auth_sign_state::PendingAuthSign;
use clock::SystemClock;
pub use clock::Clock;
#[cfg(any(test, feature = "test-support"))]
pub use clock::MonotonicSecondClock;
pub use action_registry::{default_registry, ActionRegistry, RegistrationError};
#[cfg(feature = "native")]
pub use action_registry::{ActionExecuteFailure, ActionFailureKind};
pub use composition_ledger::{
    CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION,
};
pub(crate) use identity_state::{AccountSummary, PublishQueueEntry, RelayAckOutcome};
pub use identity_state::{new_active_account_slot, ActiveAccountSlot};
#[cfg(feature = "codegen-schema")]
pub(crate) use types::LogicalInterestStatus as LogicalInterestStatusForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::Metrics as MetricsForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::RelayStatus as RelayStatusForCodegen;
pub use identity_state::{read_eligible_relay_urls, AppRelay};
#[cfg(feature = "codegen-schema")]
pub(crate) use types::TimelineItem as TimelineItemForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::WireSubscriptionStatus as WireSubscriptionStatusForCodegen;
pub use snapshot_registry::new_snapshot_projection_slot;
pub use snapshot_registry::SnapshotProjectionSlot;
pub use snapshot_registry::{record_emitted_feed_authors, EmittedFeedAuthorsSlot}; // ADR-0063 D7
pub use relay_projection::{AppRelayList, AppRelaySlot};
pub use relay_projection::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
#[cfg(feature = "native")]
pub use relay_projection::new_app_relay_slot;
pub use lifecycle::LifecyclePhase;
pub(crate) use lifecycle::LifecycleTransition;
#[cfg(not(any(test, feature = "test-support")))]
use crate::substrate::EmptyMailboxCache;
#[cfg(any(test, feature = "test-support"))]
use crate::substrate::TestInMemoryMailboxCache;
use crate::substrate::{
    empty_blocked_relay_lookup, empty_dm_inbox_relay_lookup, BlockedRelayLookup, ContactsLookup,
    DmInboxRelayLookup, EmptyOutboxRouter, EventIngestDispatcher, MailboxCache, OutboxRouter,
    ParsedRelayList, ProfileLookup, MAX_PROJECTION_MESSAGES,
};
#[cfg(not(any(test, feature = "test-support")))]
use crate::substrate::empty_contacts_lookup;
#[cfg(not(any(test, feature = "test-support")))]
use crate::substrate::empty_profile_lookup;
use crate::util::sort_dedup;
use relay_transport::RelayTransportMap;
use std::sync::atomic::AtomicU64;
pub(crate) use types::KernelSnapshot;
#[cfg(test)]
use types::TimelineItem;
use types::{
    ClaimedEventDto, Counters, DiagnosticFirehoseState, LogicalInterestStatus,
    Metrics, NoticeEntry, OutboxSummarySnapshot, ProfileCard,
    PublishOutboxItem, PublishOutboxRelay, RelayHealth, RelayStatus, StoredEvent, TimingMilestones,
    WireSub, WireSubscriptionState, WireSubscriptionStatus, MAX_NOTICE_LOG,
};

/// Per-pubkey claim consumer-id retention cap (T114b — D8 guard against unbounded growth).
pub(crate) const MAX_CLAIMS_PER_PUBKEY: usize = 256;

/// Per-`primary_id` event-claim consumer-id retention cap (mirrors `MAX_CLAIMS_PER_PUBKEY`).
pub(crate) const MAX_EVENT_CLAIMS_PER_KEY: usize = 256;

/// F-TTL inflight REQ guard duration (unix milliseconds, 1 hour).
pub(crate) const INFLIGHT_GUARD_MS: u64 = 3_600_000;

/// Per-relay-role NIP-42 credentials used by the AUTH handshake.
pub(crate) struct RelayAuthCredentials {
    pub(crate) signer: AuthSignerFn,
    pub(crate) pubkey_hex: String,
}

/// V-58 — kernel-side backoff hint for a relay URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackoffHint {
    /// Relay issued `CLOSED ["rate-limited: …"]` — use `RELAY_RECONNECT_DELAY_RATE_LIMITED`.
    RateLimited,
}

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
    /// Counter for `claim_event` drops due to `MAX_EVENT_CLAIMS_PER_KEY`.
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

impl Kernel {
    pub(crate) fn new(visible_limit: usize) -> Self {
        Self::with_storage_path(visible_limit, None)
    }

    /// Construct a Kernel, optionally backing the `EventStore` with a persistent LMDB path.
    pub fn with_storage_path(visible_limit: usize, storage_path: Option<&str>) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, None, storage_path)
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

    /// W2 — inject and hydrate the relay-author-score persistence store.
    pub fn set_relay_score_store(
        &mut self,
        store: Box<dyn crate::substrate::RelayAuthorScoreStore>,
    ) {
        self.relay_score_map = relay_score::RelayAuthorScoreMap::new();
        // Hydrate the in-memory map from persistent state.
        match store.load_all() {
            Ok(cells) => {
                // Convert raw `([u8;32], String, u32, u32, u64)` tuples back
                // into substrate types.
                //
                // §8.10 / canonicalization-on-load: we canonicalize the URL
                // here even though `flush_relay_scores_if_dirty` already
                // canonicalized it before writing. This guards against old
                // rows written before a canonicalization rule change and is
                // more robust than relying on sub-db name bumps alone.
                // Duplicate `(pubkey, canonical_url)` pairs that arise from
                // a rule change are naturally deduplicated by
                // `BTreeMap::insert` in `bulk_load` (last-writer wins).
                let substrate_cells = cells.into_iter().filter_map(
                    |(pk_bytes, url, successes, failures, last_used_unix_s)| {
                        // Encode raw pubkey bytes → lowercase hex string.
                        let pk_hex: String = pk_bytes.iter().map(|b| format!("{b:02x}")).collect();
                        // crate::planner::Pubkey = String — just use the hex string directly.
                        let pk: crate::planner::Pubkey = pk_hex;
                        // Canonicalize the stored URL so that any trailing-slash
                        // split between old and new rows collapses to one cell.
                        let canonical_url =
                            crate::relay::CanonicalRelayUrl::parse_or_raw(&url).into_string();
                        Some((
                            pk,
                            canonical_url,
                            relay_score::RelayAuthorScore {
                                successes,
                                failures,
                                last_used_unix_s,
                            },
                        ))
                    },
                );
                self.relay_score_map.bulk_load(substrate_cells);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "relay-score store: load_all failed — starting with empty map"
                );
            }
        }
        self.relay_score_store = Some(store);
    }

    /// W3 — record a relay-author score outcome; marks the map dirty for the next idle flush.
    pub fn record_relay_score(
        &mut self,
        author: &str,
        relay_url: &str,
        outcome: relay_score::ClaimOutcome,
        now_unix_s: u64,
    ) {
        self.relay_score_map
            .record(&author.to_string(), relay_url, outcome, now_unix_s);
    }

    /// W4/W5 — look up the current `RelayAuthorScore` for `(author, relay_url)`.
    #[must_use]
    pub fn get_relay_score(&self, author: &str, relay_url: &str) -> relay_score::RelayAuthorScore {
        self.relay_score_map.get(&author.to_string(), relay_url)
    }

    /// Test-only: whether the score map has unsaved mutations.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_relay_score_dirty(&self) -> bool {
        self.relay_score_map.is_dirty()
    }

    /// Set the TTL policy for replaceable events (F-TTL).
    pub fn set_replaceable_ttl(&mut self, config: replaceable_ttl::ReplaceableTtlConfig) {
        self.replaceable_ttl = config;
    }

    /// F-TTL — enqueue a replaceable event for re-verification if its freshness has expired.
    pub(crate) fn claim_replaceable(
        &mut self,
        kind: u32,
        pubkey: [u8; 32],
        d_tag: Option<String>,
        force: bool,
    ) {
        // `is_parameterized_replaceable` is the NIP-01 addressable predicate
        // (30000..=39999) — only those identities carry a `d`-tag.
        let key = if crate::store::is_parameterized_replaceable(kind) {
            crate::store::ReplaceableKey::Parameterized {
                kind,
                pubkey,
                d_tag: d_tag.unwrap_or_default(),
            }
        } else {
            crate::store::ReplaceableKey::Regular { kind, pubkey }
        };

        let now = self.now_ms();
        // `force` zeroes the freshness stamp for the gate check below, so a
        // user-initiated refresh always reads as due (`now > 0`) and enqueues
        // a re-fetch even when the cached identity is still within its TTL.
        // No redundant store write: the enqueue path overwrites with
        // `now + INFLIGHT_GUARD_MS` anyway.
        let check_at = if force {
            0
        } else {
            self.store.get_check_again_after(&key).unwrap_or(0)
        };

        // Gate: still fresh, or already in flight → nothing to do.
        if now > check_at && !self.pending_reverify.contains(&key) {
            self.pending_reverify.push_back(key.clone());
            // In-flight guard: prevent re-enqueue until EOSE re-stamps with the
            // real per-kind TTL (or the guard window elapses on a lost EOSE).
            self.store
                .set_check_again_after(key, now + INFLIGHT_GUARD_MS);
        }
    }

    /// Test-only: number of replaceable identities currently queued for re-verification.
    #[cfg(test)]
    pub(crate) fn pending_reverify_len(&self) -> usize {
        self.pending_reverify.len()
    }

    /// Test-only: sub-ids currently tracked for reverify EOSE handling.
    #[cfg(test)]
    pub(crate) fn reverify_sub_ids_for_test(&self) -> Vec<String> {
        self.reverify_subs.keys().cloned().collect()
    }

    /// Test-only: seed a reverify sub_id → key mapping directly.
    #[cfg(test)]
    pub(crate) fn seed_reverify_sub_for_test(
        &mut self,
        sub_id: &str,
        keys: Vec<crate::store::ReplaceableKey>,
    ) {
        self.reverify_subs.insert(sub_id.to_string(), keys);
    }

    /// Borrow the kernel's `EventStore` handle.
    #[must_use]
    pub fn event_store_handle(&self) -> Arc<dyn EventStore> {
        Arc::clone(&self.store)
    }

    /// Borrow the kernel's pull-cursor registry handle (ADR-0058 §3, step 3b).
    #[must_use]
    pub fn pull_cursor_registry_handle(&self) -> pull_cursor::PullCursorRegistrySlot {
        Arc::clone(&self.pull_cursor_registry)
    }

    /// Borrow the kernel's indexer-relays slot.
    #[must_use]
    pub fn indexer_relays_handle(&self) -> IndexerRelaysSlot {
        Arc::clone(&self.indexer_relays_handle)
    }

    /// Borrow the kernel's local-write-relays slot.
    #[must_use]
    pub fn local_write_relays_handle(&self) -> LocalWriteRelaysSlot {
        Arc::clone(&self.local_write_relays_handle)
    }

    /// Borrow the kernel's active-account-pubkey slot.
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        Arc::clone(&self.active_account_handle)
    }

    /// Read the current active-account pubkey (lowercase hex), or `None`.
    #[must_use]
    pub(crate) fn active_account_pubkey(&self) -> Option<&str> {
        self.active_account.as_deref()
    }

    /// V-51 — borrow the kernel's routing-trace projection.
    #[must_use]
    pub fn routing_trace(&self) -> Arc<routing_trace::RoutingTraceProjection> {
        Arc::clone(&self.routing_trace)
    }

    /// Test-support: construct with an externally-supplied publish store.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_publish_store(
        visible_limit: usize,
        publish_store: Arc<dyn crate::publish::PublishStore>,
    ) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, Some(publish_store), None)
    }

    /// Inner constructor: externally-supplied publish store + optional persistent `storage_path`.
    #[allow(dead_code)]
    pub(crate) fn with_publish_store_and_path(
        visible_limit: usize,
        publish_store: Arc<dyn crate::publish::PublishStore>,
        storage_path: Option<&str>,
    ) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, Some(publish_store), storage_path)
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
        // (`nmp-defaults::register_defaults` → the
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

        // T129 + K3 (ADR-0056) — install the coverage-ledger since-floor resolver
        // on the subscription lifecycle. The closure (the ledger read) lives in
        // `coverage_ledger::build_watermark_fn` as the cohesive owner of the
        // floor logic (the file-size cap forbids growing this constructor).
        let watermark_fn: crate::subs::WatermarkFn =
            coverage_ledger::build_watermark_fn(Arc::clone(&store));
        let mut lifecycle = SubscriptionLifecycle::new();
        lifecycle.set_watermark_fn(watermark_fn);

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
        // so the dozens of in-tree kind:10002 ingest tests keep working
        // without each one having to inject `nmp_router::InMemoryMailboxCache`
        // from a downstream crate (which `nmp-core` cannot depend on —
        // layering).
        let routing_trace = Arc::new(routing_trace::RoutingTraceProjection::new());
        let outbox_router: Arc<dyn OutboxRouter> = Arc::new(EmptyOutboxRouter::new());
        let content_parser: Arc<dyn crate::substrate::ContentParser> =
            Arc::new(crate::substrate::NoopContentParser::new());

        // Spec §271 (2026-05-25): under `cfg(test)` / `feature="test-support"`
        // the kernel auto-installs the in-crate `TestKind10002OutboxResolver`
        // (a minimal kind:10002 reader) so the dozens of in-tree publish
        // tests (`publish_engine_tests`, `outbox_tests`, `action_failure_tests`,
        // `publish_terminal_status_tests`, `eose_ok_notice_ingest_tests`,
        // `actor::commands::tests`, `kernel::test_support::seed_kind10002_for_test`
        // consumers) keep working without each test calling
        // `Kernel::set_publish_resolver` manually. Production builds use the
        // `NoopOutboxResolver` default the engine was built with above; the
        // production composition site (`nmp-defaults::register_defaults`)
        // installs the full router-side `nmp_router::Nip65OutboxResolver`
        // via `NmpApp::set_publish_resolver_factory` →
        // `Kernel::set_publish_resolver` (D0 — `nmp-core` does not name
        // `nmp-router` in its production graph; a dev-dep on `nmp-router`
        // would form a feature-incompatible cycle with `nmp-router`'s own
        // dep on `nmp-core`).
        #[cfg(any(test, feature = "test-support"))]
        let test_publish_resolver: Arc<dyn crate::publish::OutboxResolver> = Arc::new(
            crate::publish::TestKind10002OutboxResolver::new(Arc::clone(&store)).with_local_relays(
                Arc::clone(&local_write_relays_handle),
                Arc::clone(&active_account_handle),
            ),
        );
        #[cfg(any(test, feature = "test-support"))]
        let mut publish_engine = publish_engine;
        #[cfg(any(test, feature = "test-support"))]
        publish_engine.set_outbox(test_publish_resolver);

        // ADR-0057 PR 2 — test / test-support profile cache (shared between the
        // `profile_lookup` reader default and the `test_profile_cache` writer
        // handle so in-crate tests can seed + read profiles).
        #[cfg(any(test, feature = "test-support"))]
        let test_profile_cache = Arc::new(crate::substrate::TestProfileCache::new());

        // ADR-0057 PR 3 — test / test-support contacts cache (shared between the
        // `contacts_lookup` reader default and the `test_contacts_cache` writer
        // handle so in-crate tests can seed + read contact lists).
        #[cfg(any(test, feature = "test-support"))]
        let test_contacts_cache = Arc::new(crate::substrate::TestContactsCache::new());

        let mut kernel = Self {
            store,
            clock: Arc::new(SystemClock),
            rev: 0,
            visible_limit,
            // ADR-0055 Rung 1: default (all counters 0, epoch 0); free on Reset.
            projection_rev_tracker: projection_rev::ProjectionRevTracker::default(),
            ref_row_delta_tracker: crate::refs::RefRowDeltaTracker::default(), // ADR-0063/0053 glue
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
            // ADR-0057 PR 2 — production starts cold (empty lookup); apps inject
            // `nmp_nip01::ProfileCache` via `set_profile_lookup`. Test / test-support
            // builds default to a `TestProfileCache` shared with `test_profile_cache`
            // so in-crate tests can seed + read profiles without depending on
            // `nmp-nip01` (mirrors the `mailbox_cache` test default).
            #[cfg(not(any(test, feature = "test-support")))]
            profile_lookup: empty_profile_lookup(),
            #[cfg(any(test, feature = "test-support"))]
            profile_lookup: Arc::clone(&test_profile_cache) as Arc<dyn ProfileLookup>,
            events: HashMap::new(),
            metric_note_events: 0,
            metric_duplicate_events: 0,
            metric_stored_events: 0,
            cached_estimated_store_bytes: std::cell::Cell::new(None),
            timeline: VecDeque::new(),
            diagnostic_firehose: DiagnosticFirehoseState::default(),
            deferred_outbound: VecDeque::new(),
            pending_backoff_hints: Vec::new(),
            // ADR-0057 PR 3 — production starts cold (empty lookup); apps inject
            // `nmp_nip01::ContactsCache` via `set_contacts_lookup`. Test /
            // test-support builds default to a `TestContactsCache` shared with
            // `test_contacts_cache` so in-crate tests can seed + read contact
            // lists without depending on `nmp-nip01`.
            #[cfg(not(any(test, feature = "test-support")))]
            contacts_lookup: empty_contacts_lookup(),
            #[cfg(any(test, feature = "test-support"))]
            contacts_lookup: Arc::clone(&test_contacts_cache) as Arc<dyn ContactsLookup>,
            #[cfg(any(test, feature = "test-support"))]
            mailbox_cache: Arc::new(TestInMemoryMailboxCache::new()),
            #[cfg(not(any(test, feature = "test-support")))]
            mailbox_cache: Arc::new(EmptyMailboxCache::new()),
            outbox_router,
            content_parser,
            routing_trace,
            dm_inbox_relays: empty_dm_inbox_relay_lookup(),
            blocked_relays: empty_blocked_relay_lookup(),
            bootstrap_self_kinds_override: None,
            ingest_dispatcher: Arc::new(std::sync::RwLock::new(EventIngestDispatcher::new())),
            #[cfg(any(test, feature = "test-support"))]
            test_dm_inbox_cache: None,
            #[cfg(any(test, feature = "test-support"))]
            test_profile_cache,
            #[cfg(any(test, feature = "test-support"))]
            test_contacts_cache,
            timeline_authors: BTreeSet::new(),
            follow_feed_interest_ids: BTreeSet::new(),
            follow_feed_kinds: BTreeSet::new(),
            profile_claims: HashMap::new(),
            live_profile_claims: HashMap::new(),
            live_event_claims: HashMap::new(),
            ref_profile_shapes: HashMap::new(),
            ref_event_shapes: HashMap::new(),
            auto_profile_refs_by_consumer: BTreeMap::new(),
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
            lifecycle_phase: LifecyclePhase::Inactive,
            event_observers: None,
            external_event_sink_dispatcher: None,
            snapshot_projections: None,
            configured_relays_handle: None,
            indexer_relays_handle,
            local_write_relays_handle,
            active_account_handle,
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
            snapshot_builder: flatbuffers::FlatBufferBuilder::new(), // ADR-0055 Rung 3 (D3-6)
            _not_send: PhantomData,
        };
        if let Some(store) = store_bundle.relay_score_store {
            kernel.set_relay_score_store(store);
        }
        // ADR-0057 PR 2 — in test / test-support builds, register a kind:0
        // parser writing the shared `TestProfileCache` on the kernel's own
        // dispatcher. This makes the real chokepoint path
        // (`verify_and_persist` → `EventIngestDispatcher` → parser) write the
        // profile cache exactly as production does (where
        // `nmp_defaults::register_substrate` registers `nmp_nip01::Kind0Parser`),
        // so read-your-writes for a locally published kind:0 works in-crate
        // without depending on `nmp-nip01`.
        #[cfg(any(test, feature = "test-support"))]
        {
            let parser: Arc<dyn crate::substrate::IngestParser> = Arc::new(
                crate::substrate::TestKind0Parser::new(Arc::clone(&kernel.test_profile_cache)),
            );
            kernel.register_ingest_parser(0, parser);
        }
        // ADR-0057 PR 3 — in test / test-support builds, register a kind:3
        // parser writing the shared `TestContactsCache` on the kernel's own
        // dispatcher. This makes the real chokepoint path (`verify_and_persist`
        // → `EventIngestDispatcher` → parser → the kernel's contacts-transition
        // effect signal) write the contacts cache exactly as production does
        // (where `nmp_defaults::register_substrate` registers
        // `nmp_nip01::Kind3Parser`), so read-your-writes for a locally published
        // kind:3 works in-crate without depending on `nmp-nip01`.
        #[cfg(any(test, feature = "test-support"))]
        {
            let parser: Arc<dyn crate::substrate::IngestParser> = Arc::new(
                crate::substrate::TestKind3Parser::new(Arc::clone(&kernel.test_contacts_cache)),
            );
            kernel.register_ingest_parser(3, parser);
        }
        kernel
    }

    /// Swap the kernel's wall-clock (test/replay seam; production never calls this).
    // `allow(dead_code)`: called from `#[cfg(test)]` code only in nmp-core;
    // external crate integration tests reach it via the `test-support` feature.
    #[allow(dead_code)]
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.clock = clock;
    }

    #[allow(dead_code)]
    #[cfg(not(any(test, feature = "test-support")))]
    pub(crate) fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.clock = clock;
    }

    /// Current wall-clock seconds since the Unix epoch via the injected `Clock` (D9).
    pub fn now_secs(&self) -> u64 {
        self.clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Current wall-clock milliseconds since the Unix epoch via the injected `Clock` (D9).
    pub(crate) fn now_ms(&self) -> u64 {
        self.clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Run one bounded GC pass; records the result for diagnostics.
    pub fn run_gc_step(&mut self) -> Option<crate::store::GcReport> {
        let now_secs = self.now_secs();
        // #1088 — RAM-tier eviction runs on every GC pass regardless of
        // whether the store pass succeeds.  This is a separate call site from
        // the LMDB-tier gc_step (#1085) so the two paths stay independent and
        // merge-clean.
        let ram_report = self.evict_ram_caches();
        if ram_report.events_evicted + ram_report.profiles_evicted + ram_report.contacts_evicted > 0
        {
            tracing::debug!(
                events_evicted = ram_report.events_evicted,
                profiles_evicted = ram_report.profiles_evicted,
                contacts_evicted = ram_report.contacts_evicted,
                "ram cache eviction pass",
            );
        }
        // #1090 Stage 1 / #1480 — derive the ephemeral store-tier pin set only
        // when a finite durable-retention budget needs it. With production's
        // default unbounded durable retention this returns empty pins and avoids
        // the store scan entirely.
        let (pins, gc_budget) = self.derive_store_gc_inputs();
        // K3 Stage D3 leg 2 — the eviction⇄ledger coherence backstop guards
        // (one per active covered `(filter_hash, relay)`). Passed alongside the
        // pins so the store can lower an over-claimed `covered_through` in the
        // SAME transaction as the below-floor delete that made it stale.
        let coverage_guards = if gc_budget.max_total_events < usize::MAX {
            self.derive_coverage_guards()
        } else {
            Vec::new()
        };
        match self.store.gc_step_with_pins_and_coverage(
            gc_budget,
            now_secs,
            &pins,
            &coverage_guards,
        ) {
            Ok(report) => {
                self.last_gc_at_ms = Some(self.now_ms());
                self.last_gc = Some(report.clone());
                #[cfg(any(test, feature = "test-support"))]
                if report.lru_evicted > 0 {
                    PROCESS_STORE_LRU_EVICTED.fetch_add(
                        report.lru_evicted as u64,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                Some(report)
            }
            Err(e) => {
                tracing::warn!(error = %e, "gc_step failed; skipping this pass");
                None
            }
        }
    }

    /// The last `GcReport` from `run_gc_step`, or `None` if no pass has run yet.
    pub fn last_gc(&self) -> Option<&crate::store::GcReport> {
        self.last_gc.as_ref()
    }

    /// Wall-clock time (Unix ms) of the last `run_gc_step`, or `None`.
    pub fn last_gc_at_ms(&self) -> Option<u64> {
        self.last_gc_at_ms
    }

    /// Test-support: set a durable LRU eviction ceiling for the GC budget.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_gc_budget_ceiling(&mut self, max_events: usize) {
        self.gc_budget_ceiling = Some(max_events);
    }

    /// Resolve configured relay URLs for a given `RelayRole`; empty when none are configured.
    pub(crate) fn bootstrap_urls_for_role(&self, role: RelayRole) -> Vec<String> {
        let matches = |row_role: &str| match role {
            RelayRole::Content => {
                crate::actor::has_role(row_role, "read")
                    || crate::actor::has_role(row_role, "write")
            }
            RelayRole::Indexer => crate::actor::has_role(row_role, "indexer"),
            RelayRole::Wallet => false,
        };
        self.configured_relays
            .iter()
            .filter(|r| matches(&r.role))
            .map(|r| r.url.clone())
            .collect()
    }

    /// Cold-start discovery seed (Indexer + Content URLs, sorted/deduped).
    pub(crate) fn bootstrap_discovery_relays(&self) -> Vec<String> {
        let mut urls: Vec<String> = self
            .bootstrap_urls_for_role(RelayRole::Indexer)
            .into_iter()
            .chain(self.bootstrap_urls_for_role(RelayRole::Content))
            .collect();
        sort_dedup(&mut urls);
        urls
    }

    /// Bind a per-role NIP-42 signer callback; replaces any previously-bound signer (D0).
    pub fn set_relay_auth_signer(
        &mut self,
        role: RelayRole,
        pubkey_hex: String,
        signer: AuthSignerFn,
    ) {
        self.auth_signers
            .insert(role, RelayAuthCredentials { signer, pubkey_hex });
    }

    /// Drop the signer for `role`; challenges from that role are then recorded but unanswered.
    pub fn clear_relay_auth_signer(&mut self, role: RelayRole) {
        self.auth_signers.remove(&role);
    }

    /// Bind the shared relay-edit rows slot so the FFI layer can read relay rows.
    pub(crate) fn set_app_relay_slot(&mut self, handle: AppRelaySlot) {
        self.configured_relays_handle = Some(handle);
    }

    /// Extract the relay-edit rows handle before a `Reset` replaces the kernel.
    pub(crate) fn take_app_relay_slot_for_reset(&mut self) -> Option<AppRelaySlot> {
        self.configured_relays_handle.take()
    }

    /// Test-only: clear `configured_relays` for the empty-bootstrap diagnostic test path.
    #[cfg(test)]
    pub(crate) fn clear_configured_relays_for_test(&mut self) {
        self.configured_relays.clear();
        if let Some(handle) = self.configured_relays_handle.as_ref() {
            if let Ok(mut guard) = handle.lock() {
                guard.replace(Vec::new());
            }
        }
    }

    /// Mark `(relay_url, sub_id)` as persistent — EOSE will not auto-CLOSE it.
    pub fn register_persistent_sub(
        &mut self,
        relay_url: impl Into<String>,
        sub_id: impl Into<String>,
    ) {
        let relay_url = relay_url.into();
        let key = CanonicalRelayUrl::parse_or_raw(&relay_url);
        self.wire.persistent.insert((key, sub_id.into()));
    }

    /// Remove `(relay_url, sub_id)` from the persistent set. Idempotent.
    pub fn unregister_persistent_sub(&mut self, relay_url: &str, sub_id: &str) {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire.persistent.remove(&(key, sub_id.to_string()));
    }

    /// True when `(relay_url, sub_id)` is registered as persistent.
    pub(crate) fn is_persistent_sub(&self, relay_url: &str, sub_id: &str) -> bool {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire.persistent.contains(&(key, sub_id.to_string()))
    }

    /// Single-writer insert into `self.wire.subs` (PD-033-C Stage 0).
    pub(crate) fn insert_wire_sub(
        &mut self,
        role: RelayRole,
        relay_url: CanonicalRelayUrl,
        sub_id: String,
        filter_summary: String,
        initial_state: &str,
        since_floor: Option<u64>,
    ) {
        self.wire.subs.insert(
            (relay_url.clone(), sub_id.clone()),
            WireSub {
                id: sub_id,
                role,
                relay_url,
                filter_summary,
                state: initial_state.to_string(),
                events_rx: 0,
                opened_at: Instant::now(),
                last_event_at: None,
                eose_at: None,
                close_reason: None,
                since_floor,
            },
        );
        self.changed_since_emit = true;
    }

    pub(crate) fn start(&mut self) {
        if self.timing.started_at.is_none() {
            self.timing.started_at = Some(Instant::now());
            self.timing.started_unix_ms = Some(self.now_ms()); // D9 wall anchor
        }
        self.changed_since_emit = true;
        self.log("starting role-aware nmp demo slice");
    }

    pub(crate) fn set_visible_limit(&mut self, limit: usize) {
        if self.visible_limit != limit {
            self.visible_limit = limit;
            self.changed_since_emit = true;
        }
    }

    pub(crate) fn visible_limit(&self) -> usize {
        self.visible_limit
    }

    pub(crate) fn changed_since_emit(&self) -> bool {
        self.changed_since_emit
    }

    /// Force the next due tick to emit a snapshot even if no kernel field changed.
    pub fn mark_changed_since_emit(&mut self) {
        self.changed_since_emit = true;
    }

    /// Mutable access to the subscription lifecycle (registry + trigger inbox).
    pub(crate) fn lifecycle_mut(&mut self) -> &mut SubscriptionLifecycle {
        &mut self.lifecycle
    }

    /// M2 (ADR-0042) — attach one owner to a generic feed interest; enqueues a recompile trigger.
    pub(crate) fn open_interest_sub(
        &mut self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    ) -> bool {
        // Unified front-door (EnsureAbsent = register-if-absent). Store-serve +
        // recompile trigger fire only when the interest is newly installed.
        let outcomes = self.register_interest(
            &[crate::kernel::cache_serve::InterestRegistration {
                identity,
                interest,
                policy: crate::kernel::cache_serve::InterestWrite::EnsureAbsent,
            }],
            "open-interest",
        );
        outcomes[0].newly_installed
    }

    /// M2 (ADR-0042) — detach one owner from a generic feed interest; enqueues a recompile trigger.
    pub(crate) fn close_interest_sub(&mut self, identity: &crate::subs::SubIdentity) -> bool {
        let removed = self.lifecycle.registry_mut().drop_owner(identity);
        if removed {
            self.lifecycle
                .enqueue_trigger(crate::subs::CompileTrigger::InvalidateCompile {
                    reason: crate::subs::InvalidateReason::External("close-interest".to_string()),
                });
        }
        removed
    }

    /// Pre-populate the contacts cache at sign-in without fabricating a kind:3 event.
    pub(crate) fn prepopulate_contacts(&mut self, pubkey: String, follows: Vec<String>) {
        let created_at = self.now_secs();
        // (1) Write the contacts cache directly — non-ingest writer seam, NO
        // fabricated event through the dispatcher / observer fan-out.
        self.contacts_lookup().upsert(
            pubkey.clone(),
            crate::substrate::ContactsView {
                // Maximal sentinel id: a real signed kind:3 (a 64-hex id, always
                // `< "f"*64`) supersedes this seed on a `created_at` tie.
                event_id: "f".repeat(64),
                created_at,
                follows: follows.clone(),
            },
        );
        self.cached_estimated_store_bytes.set(None);
        // (2) Drive the kernel-owned follow-feed effects directly (active-account
        // scoped, like the chokepoint transition that calls the same body) —
        // WITHOUT `notify_event_observers`.
        if self.active_account.as_deref() == Some(pubkey.as_str()) {
            self.on_active_contacts_changed(&pubkey, follows, created_at);
        }
    }

    /// Pre-populate the NIP-65 mailbox cache from a just-signed kind:10002 event.
    pub(crate) fn prepopulate_author_relay_list(
        &mut self,
        pubkey: String,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) {
        let parsed = parse_relay_list_to_substrate(&tags);
        let empty = parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty();
        if empty {
            self.mailbox_cache.remove(&pubkey);
        } else {
            self.mailbox_cache.upsert(pubkey.clone(), parsed);
        }
        self.lifecycle
            .enqueue_trigger(CompileTrigger::Nip65Arrived { pubkey, created_at });
    }

    /// Read-only access to the substrate `MailboxCache`.
    pub(crate) fn mailbox_cache(&self) -> &dyn MailboxCache {
        &*self.mailbox_cache
    }

    /// Test-only: push a NIP-65 cache entry without going through the kind:10002 ingest path.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn seed_mailbox_relay_list(
        &self,
        pubkey: &str,
        read: Vec<String>,
        write: Vec<String>,
        both: Vec<String>,
    ) {
        self.mailbox_cache
            .upsert(pubkey.to_string(), ParsedRelayList { read, write, both });
    }

    /// Test-only: shared handle to the substrate `MailboxCache`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mailbox_cache_arc(&self) -> Arc<dyn MailboxCache> {
        Arc::clone(&self.mailbox_cache)
    }

    /// Test-only: inject a `store_open_failure` string without requiring a real LMDB failure.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_store_open_failure_for_test(&mut self, reason: impl Into<String>) {
        self.store_open_failure = Some(reason.into());
    }

    /// Test-only: set `active_account` directly for diagnostic path tests.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.active_account = Some(pubkey.into());
    }

    /// Test-only: cache a kind:0 profile without going through the ingest chokepoint.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn seed_profile_kind0_for_test(
        &self,
        pubkey: &str,
        event_id: &str,
        created_at: u64,
        content: &str,
    ) -> bool {
        self.test_profile_cache
            .ingest_kind0(pubkey, event_id, created_at, content)
    }

    /// Read-only access to the injected `OutboxRouter`.
    #[allow(dead_code)] // Reserved for follow-on wiring of actual routing call sites.
    pub(crate) fn outbox_router(&self) -> &dyn OutboxRouter {
        &*self.outbox_router
    }
}
