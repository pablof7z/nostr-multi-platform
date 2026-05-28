//! Kernel — the actor-owned event-processing core.
//!
//! Sub-modules:
//! - `types`        — pure data types shared across the kernel
//! - `ingest`       — relay frame parsing, event dispatch, and kind-specific ingest
//! - `requests`     — relay state transitions, startup/view REQs, req/defer primitives
//! - `status`       — diagnostics, metrics, and update-payload assembly
//! - `update`       — diff/emit logic for the FFI update loop
//! - `nostr`        — `NostrEvent` deserialization + helper functions
//! - `test_support` — signature-free injection helpers (test / test-support feature)
//! - `tests`        — unit tests (cfg(test) only)

// M6 (first half) — the runtime that drives the `substrate::ActionModule`
// trait. `pub(crate)` so the crate-private `ffi` module can reach
// `ActionRegistry` / `default_registry` for the `nmp_app_dispatch_action`
// entry point.
pub(crate) mod action_registry;
// Actor-owned per-correlation_id stage tracker. `pub(crate)` so the
// FFI ack symbol (`crate::ffi::action::nmp_app_ack_action_stage`) and the
// dispatch handler (`actor::dispatch`) can reach the type aliases; the
// `Kernel`-attached API itself lives on `impl Kernel` (see `mod.rs` below).
#[cfg(test)]
mod action_failure_tests;
pub(crate) mod action_lifecycle;
#[cfg(test)]
mod action_lifecycle_tests;
pub(crate) mod action_stages;
#[cfg(test)]
mod action_stages_tests;
mod auth;
mod clock;
#[cfg(test)]
mod clock_injection_tests;
#[cfg(test)]
mod closed_classifier_tests;
// `pub(crate)` so the typed FFI error-category constants (`ERR_*`) are
// reachable from the `actor` module's command handlers, not just kernel-
// internal callsites.
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
pub(crate) mod closed_reason;
mod discovery;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod eose_ok_notice_ingest_tests;
#[cfg(test)]
mod event_claim_tests;
mod event_observer;
#[cfg(test)]
mod event_observer_tests;
mod identity_state;
mod ingest;
#[cfg(test)]
mod ingest_tests;
mod lifecycle;
mod lifecycle_drain;
mod local_publish_intent;
#[cfg(test)]
mod local_publish_intent_tests;
mod mailboxes;
mod nostr;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod profile_claim_tests;
mod provenance;
#[cfg(test)]
mod provenance_wire_tests;
mod publish_cmd;
mod publish_engine;
#[cfg(test)]
mod publish_engine_tests;
mod publish_engine_wire;
mod publish_outbox;
#[cfg(test)]
mod publish_relay_identity_tests;
#[cfg(test)]
mod publish_terminal_status_tests;
// Diagnostics-screen projection — pre-rolled relay/wire-sub roll-ups +
// pre-formatted display strings. Replaces the §4.5 / §6 anti-pattern #1
// derivations the three iOS diagnostics views used to do client-side. See
// the module doc for the bible references.
mod relay_diagnostics;
mod relay_transport;
// V-51 phase 1 — bounded ring-buffer projection of recent routing decisions
// fed by the `RoutingTraceObserver` substrate seam. Constructed by
// `Kernel::new` and held as `Arc<RoutingTraceProjection>` so the same
// allocation is shared with whichever `OutboxRouter` impl the kernel
// installs (the router stores `Arc<dyn RoutingTraceObserver>` — the
// projection is the only concrete impl).
//
// V-51 phase 4 (validation harness) needs the projection type reachable
// from `nmp-testing` and the chirp-repl, so the module is `pub` and the
// three projection types it owns (`RoutingTraceProjection`,
// `PublishTraceEntry`, `SubscriptionTraceEntry`) are re-exported below.
// This is not "widening the substrate" (substrate is `crate::substrate`,
// which carries the producer-side trait `RoutingTraceObserver`); the
// projection is the consumer-side observability primitive, naturally
// belongs to the kernel, and is the Rust-level read door the FFI surface
// (phase 2 proper) and the validation harness (phase 4) both consume.
pub mod routing_trace;
// V-51 phase 2 — JSON DTO renderer for the routing-trace projection. Pure
// consumer-side helper: walks `RoutingTraceProjection::snapshot_*` and
// produces a stable `serde_json::Value` the FFI / wasm snapshot surfaces
// hand back to Swift / TypeScript callers. Does NOT widen the substrate
// (`RoutingSource` et al. stay free of `serde::Serialize`).
pub mod routing_trace_dto;
// Typed slot wrappers for relay-shaped actor-owned caches. The bare
// `Arc<Mutex<Vec<String>>>` / `Arc<Mutex<Vec<RelayEditRow>>>` slots from the
// publish resolver and `NmpApp` move behind named types here so D14 can flag
// future regressions on the field shape.
mod relay_frame;
mod relay_projection;
// W1 — substrate-pure RelayAuthorScore type + per-author/relay scoring
// map. Consumed by W3 (score-update seams) and W4 (planner warm-relay
// preference). LMDB hydration/flush is W2. See
// `docs/design/relay-search-radius-impl-plan.md` §0/§W1 with §8.5/§8.10
// amendments applied.
pub mod relay_score;
#[cfg(test)]
mod relay_score_tests;
// W2 — flush dirty score cells to the injected `RelayAuthorScoreStore`.
// Called on actor idle; no-op when the map is clean or no store is set.
mod raw_event_observer;
#[cfg(test)]
mod raw_event_observer_tests;
mod relay_score_flush;
mod relay_score_lookup_impl;
// W3 — score-update seam: edge-triggered hooks translate wire-frame outcomes
// (EVENT = Hit, EOSE = EoseNoMatch, relay_failed = Failed) into score deltas.
// The author lookup is a test seam until W5 populates `claim_expansion_subs`.
mod relay_score_record;
mod replay;
#[cfg(test)]
mod replay_tests;
mod reply;
mod requests;
#[cfg(test)]
mod retention_tests;
// Host-extensible snapshot output — the `nmp_app_register_snapshot_projection`
// seam. `pub(crate)` so the crate-private `ffi` module can reach the registry
// + slot helpers for the C-ABI registration entry point.
#[cfg(test)]
mod dm_inbox_routing_tests;
#[cfg(test)]
mod perf_tests;
pub(crate) mod snapshot_registry;
#[cfg(test)]
mod snapshot_registry_tests;
#[cfg(test)]
mod state_projection_tests;
mod status;
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
#[cfg(test)]
mod timeline_order_tests;
#[cfg(test)]
mod timeline_perf_tests;
mod types;
mod update;
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
mod contacts_fanout_tests;

use crate::relay::{
    CanonicalRelayUrl, OutboundMessage, RelayRole, DEFAULT_EMIT_HZ, TIMELINE_AUTHOR_LIMIT,
};
// `chrono::Local` reads the OS-local wall clock; the `clock` feature it lives
// behind is gated to `native` in Cargo.toml. The wall-clock display helpers
// (`format_timestamp` / `now_hms` in `kernel/nostr.rs`) are themselves
// native-only — see the `#[cfg(feature = "native")]` gates on those two
// functions and the single use site in `kernel/update.rs::created_at_display`.
#[cfg(feature = "native")]
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::{Duration, Instant};
// `SystemTime` and `UNIX_EPOCH` are only consumed by native-gated functions in
// `kernel/nostr.rs` (format_timestamp/now_hms) and `kernel/ingest/auth_handlers.rs`.
// Both callers use `#[cfg(feature = "native")]` so these imports can also be gated.
#[cfg(feature = "native")]
use std::time::{SystemTime, UNIX_EPOCH};
// V-01 Phase 1c: the kernel no longer names `tungstenite`. The native
// `relay_worker` converts `tungstenite::Message` → [`RelayFrame`] before
// handing it to [`Kernel::handle_message`]; a non-native transport (wasm32)
// is responsible for its own equivalent conversion.
//
// V-01 Stage 3: re-exported `pub` (lib.rs surfaces it as `nmp_core::RelayFrame`)
// so the wasm32 `BrowserRelayDriver` in `nmp-wasm` can construct frames from
// `web_sys::MessageEvent` / `web_sys::CloseEvent` and hand them to
// `KernelReducer::handle_relay_frame`. Substrate-grade (D0).
pub use relay_frame::RelayFrame;

use nostr::{
    diff_items, event_references, first_event_ref, parse_profile, parse_relay_list, ratio,
    referenced_event_ids, root_event_id, short_hex, truncate, NostrEvent,
};
// V-01 Phase 1c follow-up: `format_timestamp` / `now_hms` are
// `#[cfg(feature = "native")]` in `kernel/nostr.rs` (they read the OS
// wall clock via `chrono::Local`). Importing them unconditionally breaks
// `--no-default-features` (wasm32) builds. The single call sites in
// `update.rs`, `status.rs`, and `publish_outbox.rs` are themselves
// already `#[cfg(feature = "native")]`, so the re-export is gated too.
#[cfg(feature = "native")]
use nostr::{format_timestamp, now_hms};
// `is_hex_id` / `is_hex_pubkey` reach `nmp-ffi` through
// `nmp_core::__ffi_internal::*` (the FFI surface uses them to validate
// `*const c_char` arguments for `open_thread` / `open_author` etc.).
pub use nostr::{is_hex_id, is_hex_pubkey};

/// Decode a 64-char lowercase/uppercase-hex pubkey into the store's
/// `[u8; 32]` `PubKey`. Returns `None` on any malformed input — callers
/// treat `None` as "no lookup" (never panics across the FFI boundary, D6).
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

use crate::store::{EventStore, MemEventStore};
use crate::subs::{CompileTrigger, OneshotApi, SubscriptionLifecycle, UnknownIds};
use auth::AuthDriverState;
pub use auth::AuthSignerFn;
use clock::{Clock, SystemClock};
// M6 — action-dispatch runtime, reachable from the `ffi` module for the
// `nmp_app_dispatch_action` entry point. V-01 Phase 1c: native FFI only.
// `default_registry` / `ActionRegistry` are reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::*` (the FFI surface owns the
// `nmp_app_dispatch_action` entry point).
#[cfg(feature = "native")]
pub use action_registry::{default_registry, ActionRegistry};
pub(crate) use identity_state::{
    AccountSummary, PublishQueueEntry, RelayAckOutcome, SettingsHubSummary,
};
// Re-exported `pub` (widened from `pub(crate)`) so `crate::slots` can
// re-export them into the public crate surface — `nmp-router::Nip65OutboxResolver`
// (spec §271) constructs slots through these. Direct consumers in nmp-core
// continue to import through `crate::kernel::{...}`.
pub use identity_state::{new_active_account_slot, ActiveAccountSlot};
// V6 Stage 1 — Swift codegen pilot. The four projection types below are
// `pub(crate) struct`s in `types` (widened from `pub(super)` so the
// re-export can lift them out of `kernel`); the `codegen-schema` build
// hands them to `schemars::schema_for!` from `crate::codegen_schema`.
// Feature-gated so non-codegen builds don't trip the unused-import lint
// (no in-crate consumer outside `codegen_schema`). Crate-private
// encapsulation is preserved either way — nothing outside `nmp-core`
// can name these types.
// V6 Stage 1's `codegen-schema` feature originally added a
// `pub(crate) use types::{LogicalInterestStatus, Metrics, RelayStatus,
// WireSubscriptionStatus}` re-export here so `crate::codegen_schema` could
// reach those types through `crate::kernel::*`. That re-export collided
// (E0252) with the always-on `use types::{...}` further down — fully
// breaking the `codegen-drift` CI workflow on master from #358 onward
// (every push since 2026-05-23 10:39 went red). The fix: use `pub(crate)
// use … as …` aliases instead. The aliases bind a different identifier
// than the plain `use` below, sidestepping E0252, and `codegen_schema`
// imports through the aliases. The module `kernel::types` itself is
// private to `kernel` (`mod types;` line 125), so we cannot import the
// types directly from their canonical path either — the re-export is
// the only path out.
#[cfg(feature = "codegen-schema")]
pub(crate) use types::LogicalInterestStatus as LogicalInterestStatusForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::Metrics as MetricsForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::RelayStatus as RelayStatusForCodegen;
// V6 Stage 3 — `TimelineItem` joins the Stage 1 alias set. Same E0252 reason
// as the four pilot types above: `mod types` is private to `kernel`, so the
// only way to reach `TimelineItem` from `crate::codegen_schema` is through
// this re-export, and the `as ForCodegen` rename sidesteps a collision with
// the plain `use types::{... TimelineItem ...}` at the bottom of the imports
// block in this file.
pub use identity_state::{read_eligible_relay_urls, RelayEditRow};
#[cfg(feature = "codegen-schema")]
pub(crate) use types::TimelineItem as TimelineItemForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::WireSubscriptionStatus as WireSubscriptionStatusForCodegen;
// Host-extensible snapshot output — reachable from the `ffi` module for the
// `nmp_app_register_snapshot_projection` C-ABI entry point.
// `SnapshotProjectionSlot` is a Kernel struct field type (always-compiled);
// `new_snapshot_projection_slot` is only called from native-only callers.
// `SnapshotProjectionSlot` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::SnapshotProjectionSlot` (the NmpApp struct
// field type); `new_snapshot_projection_slot` is called once from
// `nmp_app_new`.
#[cfg(feature = "native")]
pub use snapshot_registry::new_snapshot_projection_slot;
pub use snapshot_registry::SnapshotProjectionSlot;
// Typed slot wrappers + constructors. `RelayEditRowsSlot` /
// `RelayEditRowList` are re-exported below at `pub use` because per-app
// crates (e.g. `nmp-app-chirp`) consume the slot via
// `NmpApp::relay_edit_rows_handle()` and iterate via `guard.as_slice()`;
// without the public re-export Chirp could not name the returned slot type.
// `RelayUrls` and the URL-slot aliases stay kernel-internal: no external
// caller names them directly (the resolver constructs slots via the
// `new_*_slot()` helpers and reads through `as_slice()`).
pub use relay_projection::{RelayEditRowList, RelayEditRowsSlot};
// Re-exported `pub` (widened from `pub(crate)`) so `crate::slots` can
// surface them — `nmp-router::Nip65OutboxResolver` (spec §271) constructs
// resolver slots with handles shared by the kernel actor's reducer. Direct
// in-crate consumers continue to import through `crate::kernel::{...}`.
pub use relay_projection::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
// `new_relay_edit_rows_slot` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::new_relay_edit_rows_slot` (called once from
// `nmp_app_new` to construct the slot the actor and the per-app crate
// share).
#[cfg(feature = "native")]
pub use relay_projection::new_relay_edit_rows_slot;
// `LifecyclePhase` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::LifecyclePhase` (the C-ABI lifecycle
// background / foreground entry points construct it before sending the
// `ActorCommand::LifecycleEvent`).
pub use lifecycle::LifecyclePhase;
pub(crate) use lifecycle::LifecycleTransition;
// D0: NIP-47 NWC is an app noun. `WalletStatus` no longer lives in the kernel
// — it moved to the wallet command runtime (`actor::commands::wallet`) and is
// surfaced via the `projections["wallet"]` snapshot projection, NOT a typed
// `KernelSnapshot` field. The kernel never names the NWC noun.
#[cfg(not(any(test, feature = "test-support")))]
use crate::substrate::EmptyMailboxCache;
#[cfg(any(test, feature = "test-support"))]
use crate::substrate::TestInMemoryMailboxCache;
use crate::substrate::{
    empty_blocked_relay_lookup, empty_dm_inbox_relay_lookup, BlockedRelayLookup,
    DmInboxRelayLookup, EmptyOutboxRouter, EventIngestDispatcher, MailboxCache, OutboxRouter,
    ParsedRelayList,
};
use crate::util::sort_dedup;
use relay_transport::RelayTransportMap;
use std::sync::atomic::{AtomicU64, Ordering};
use types::{
    AuthorViewPayload, AuthorViewState, ClaimedEventDto, Counters, DiagnosticFirehoseState,
    KernelSnapshot, LogicalInterestStatus, MentionProfilePayload, Metrics, OutboxSummarySnapshot,
    Profile, ProfileAction, ProfileCard, ProfileDispatchSpec, ProfileRequestState,
    PublishOutboxItem, PublishOutboxRelay, RelayHealth, RelayStatus, StoredEvent,
    ThreadViewPayload, ThreadViewState, TimelineItem, TimingMilestones, ViewInterest, WireSub,
    WireSubscriptionState, WireSubscriptionStatus,
};

/// Per-pubkey claim consumer-id retention cap (T114b — per-dispatch retention audit).
///
/// `profile_claims[pk]: BTreeSet<consumer_id>` grows once per `claim_profile` call;
/// without a cap a long-lived process accumulates `consumer_ids` in proportion to
/// dispatch count rather than working-set size (a D8 violation — see PD-021
/// line-11 and `docs/perf/m10.5/s2-drain-analysis.md`). The S2 flood mix issues
/// unique `consumer_ids` per dispatch with no matching release, isolating this leak.
///
/// 256 is generous for legitimate UI: every concurrent view that
/// calls `ProfileInterestAvatar` carries its own `consumer_id`; real apps hold
/// at most a few dozen simultaneously (one per visible row in a list view).
/// Caps worst-case retention per pubkey at ~12 KiB (256 × ~50 B node + key);
/// across 50 pubkeys (S2's working set) that's ~600 KiB, well under the 1 MiB
/// D8 budget. The S2 30 s flood (60 k claims across 50 pubkeys → ~1.2 k per
/// pubkey) hits the cap by design — that is the audit's load-bearing test.
///
/// Drop-newest semantics: a claim attempt past the cap silently no-ops and
/// increments `claim_drops_total`; the per-pubkey claim set is capped via
/// `MAX_CLAIMS_PER_PUBKEY` — see the audit table in `retention_tests.rs`
/// for the per-structure rationale.
pub(crate) const MAX_CLAIMS_PER_PUBKEY: usize = 256;

/// Per-`primary_id` event-claim consumer-id retention cap.
///
/// Mirrors `MAX_CLAIMS_PER_PUBKEY` for the generic `claim_event` /
/// `release_event` primitive: every `consumer_id` that asserts interest
/// in the event identified by a `nostr:` URI is recorded in
/// `event_claims[primary_id]: BTreeSet<consumer_id>`. Without a cap the
/// set scales with dispatch count rather than working-set size — a D8
/// violation symmetric with the profile-claim audit.
///
/// 256 matches the profile cap: every concurrent renderer surfacing a
/// `NostrContentView`-style embed card holds its own `consumer_id`; real
/// apps hold at most a few dozen per visible row. Drop-newest semantics:
/// a claim attempt past the cap silently no-ops and increments
/// `event_claim_drops_total`.
pub(crate) const MAX_EVENT_CLAIMS_PER_KEY: usize = 256;

/// Per-relay-role NIP-42 credentials. The closure signs the kind:22242 with
/// whatever keypair is appropriate for that role (user identity for Content /
/// Indexer; NWC client secret for Wallet). `pubkey_hex` is stamped on the
/// unsigned template's `pubkey` field — NIP-42 requires the AUTH event to be
/// signed by the connecting client's key.
pub(crate) struct RelayAuthCredentials {
    pub(crate) signer: AuthSignerFn,
    pub(crate) pubkey_hex: String,
}

/// The kernel owns all Nostr protocol state for the active app session.
///
/// It is driven by the actor loop in `crate::relay` through a simple message-
/// passing interface: relay frames arrive via `handle_message`, view intents
/// arrive via `open_*` / `close_*`, and the actor reads snapshots via `emit`.
///
/// The `EventStore` (`self.store`) is the single authoritative writer for all
/// persisted events (D4).  The lightweight `events` read-cache is a derived
/// projection populated only after the store confirms insertion or replacement.
pub struct Kernel {
    /// Pluggable event store. D4: the single writer for all Nostr events.
    ///
    /// `MemEventStore` by default; replace with `LmdbEventStore` in M3 phase 2.
    /// `Arc` (not `Box`) so the `Nip65OutboxResolver` (D3) can share the same
    /// store without a second copy — `EventStore` is interior-mutable
    /// (`insert`/`scan` take `&self`), so the actor stays the only logical
    /// writer (D4) even though the handle is shared.
    store: Arc<dyn EventStore>,
    /// Injectable wall-clock for the ingest path. Production uses
    /// `SystemClock` (delegates to `SystemTime::now()`); tests and
    /// deterministic replay swap in a `FixedClock` via [`Kernel::set_clock`]
    /// so the reducer's timestamp output (`created_at`, `received_at_ms`)
    /// is reproducible. See `kernel/clock.rs` and `kernel/replay.rs`.
    clock: Arc<dyn Clock>,
    rev: u64,
    visible_limit: usize,
    /// FFI diagnostic timing milestones (D0 app-domain state). See
    /// [`TimingMilestones`].
    timing: TimingMilestones,
    relays: HashMap<RelayRole, RelayHealth>,
    transport_relays: RelayTransportMap,
    profiles: HashMap<String, Profile>,
    /// Locally-authored kind:0 publish intents that have not necessarily
    /// round-tripped from a relay yet. Kept separate from `profiles` so
    /// `metrics.profile_events` still counts only real relay/store ingest.
    local_profile_intents: HashMap<String, Profile>,
    events: HashMap<String, StoredEvent>,
    /// Incrementally-maintained diagnostic counters for the `Metrics` snapshot
    /// fields `note_events` / `duplicate_events` / `stored_events`. Maintained
    /// at the `events` ingest/mutation sites so `make_update` (up to 60 Hz)
    /// never has to walk the whole `events` `HashMap` to recompute them — see
    /// `docs/perf` and the O(events) snapshot-emit violation this replaced.
    ///
    /// `events` is insert-only today (no eviction path mutates the `HashMap`;
    /// `sort_timeline` truncates only the `timeline` `VecDeque`). The
    /// `stored_events` counter therefore only ever increments; should an
    /// eviction path be added, decrement it there to keep the invariant.
    ///
    /// Count of cached kind:1 events ever inserted into `events`.
    metric_note_events: u64,
    /// Count of cached events whose `relay_count` transitioned 1 → >1 (a relay
    /// delivered an event already present in the read-cache).
    metric_duplicate_events: u64,
    /// Tracks `events.len()` — incremented on insert, decremented on eviction.
    metric_stored_events: u64,
    timeline: VecDeque<String>,
    /// Author-view tracking (D0 app-domain state). See [`AuthorViewState`].
    author_view: AuthorViewState,
    /// Thread-view tracking (D0 app-domain state). See [`ThreadViewState`].
    thread_view: ThreadViewState,
    /// Diagnostic firehose tracking (D0 app-domain state). See
    /// [`DiagnosticFirehoseState`].
    diagnostic_firehose: DiagnosticFirehoseState,
    deferred_outbound: VecDeque<OutboundMessage>,
    seed_contacts: HashMap<String, Vec<String>>,
    /// Substrate NIP-65 (kind:10002) cache — step 3 of
    /// `docs/architecture/crate-boundaries.md` (V-50). Replaces the
    /// pre-step-3 `HashMap<String, AuthorRelayList>` so the kernel and
    /// the injected [`OutboxRouter`] read from one source of truth.
    /// Default: `EmptyMailboxCache` in production (substrate-honest debt
    /// B, 2026-05-24); `TestInMemoryMailboxCache` under
    /// `cfg(any(test, feature = "test-support"))`. Production composition
    /// (apps that depend on `nmp-router`) injects
    /// `nmp_router::InMemoryMailboxCache` via [`Kernel::set_routing`]
    /// (driven by [`crate::NmpApp::set_routing_substrate`]) before any
    /// kind:10002 is ingested.
    ///
    /// The kind:10002 ingest path (`ingest::relay_list::ingest_relay_list`)
    /// is the single writer of this cache. The `mailbox_cache` is read
    /// by the `outbox_router` slot (per-route lane 1 lookup) and by the
    /// `KernelMailboxes` planner-side adapter; the kernel's REQ-construction
    /// sites never read it directly — they call the router
    /// (`Kernel::route_subscription_relays` /
    /// `route_outbox_subscription_relays` /
    /// `partition_ids_via_router` in `kernel/mailboxes.rs`).
    mailbox_cache: Arc<dyn MailboxCache>,
    /// Substrate outbox router — step 3 of
    /// `docs/architecture/crate-boundaries.md` §3.2. The kernel holds
    /// this as `Arc<dyn OutboxRouter>` (per the spec) so a competing
    /// routing algorithm is a single-line swap at composition time.
    /// Default: [`crate::substrate::EmptyOutboxRouter`] (every call
    /// returns `Unroutable` — substrate-honest debt B, 2026-05-24).
    /// Production composition injects `nmp_router::GenericOutboxRouter`
    /// via [`Kernel::set_routing`] (driven by
    /// [`crate::NmpApp::set_routing_substrate`]) before any routing
    /// decision is requested.
    ///
    /// **Debt A**: the router is the live decision authority for every
    /// kernel-driven REQ. `kernel/requests/profile.rs` (`author_requests`,
    /// `profile_claim_request`, `pending_profile_claim_requests`,
    /// `firehose_requests`) and `kernel/requests/thread.rs`
    /// (`maybe_open_thread_hydration`) call through the router helpers
    /// in `kernel/mailboxes.rs`; the bootstrap discovery seed flows
    /// through the substrate seam at
    /// `RoutingContext::session_keys::app_relays` (lane 7 fallback).
    outbox_router: Arc<dyn OutboxRouter>,
    /// V-51 phase 1 — bounded ring-buffer projection of recent routing
    /// decisions. Constructed once in `Kernel::with_optional_publish_store_and_path`
    /// and threaded into production composition via the
    /// `RoutingSubstrateSlot` factory (`with_trace_observer`). The default
    /// [`crate::substrate::EmptyOutboxRouter`] never produces a decision
    /// so the ring stays empty until a real router is installed.
    /// Read by the FFI surface in phase 2 (`recent_routing_decisions`
    /// snapshot field).
    routing_trace: Arc<routing_trace::RoutingTraceProjection>,
    /// Substrate DM-inbox relay lookup — V-40 of
    /// `docs/architecture/crate-boundaries.md`. The kernel reads this when
    /// it needs a receiver's DM-inbox relay set; the concrete cache (NIP-17
    /// kind:10050) lives in the `nmp-nip17` crate behind this trait so the
    /// kernel never names the NIP-17 noun (D0). Default:
    /// `EmptyDmInboxRelayLookup` (cold-start; every lookup returns `None`,
    /// the fail-closed contract the gift-wrap publish path expects). Apps
    /// that need DM routing inject `nmp_nip17::DmRelayCache` via
    /// [`Kernel::set_dm_inbox_relay_lookup`] — the same `Arc` is
    /// simultaneously the writer side fed by `nmp_nip17::Kind10050Parser`
    /// (registered with `ingest_dispatcher`).
    dm_inbox_relays: Arc<dyn DmInboxRelayLookup>,
    /// Substrate blocked-relay lookup — wired through the
    /// [`crate::substrate::BlockedRelayLookup`] seam. The kernel reads this
    /// inside [`Kernel::build_routing_context`] on every routing decision
    /// so the router's subtractive blocked-set post-pass drops kind:10006
    /// blocked URLs from outbox routing. The concrete cache (kind:10006
    /// today) lives in `nmp-router` so the kernel never names the wire
    /// shape of a kind:10006 event (D0). Default:
    /// [`crate::substrate::EmptyBlockedRelayLookup`] (every lookup returns
    /// an empty set, preserving the pre-V-40 byte-for-byte zero-block
    /// behaviour the four `BlockedRelaySet::new()` call sites in
    /// `kernel/mailboxes.rs` assumed). Apps that need outbox blocking
    /// inject `nmp_router::InMemoryBlockedRelayCache` via
    /// [`Kernel::set_blocked_relay_lookup`] — the same `Arc` is
    /// simultaneously the writer side fed by
    /// `nmp_router::Kind10006Parser` (registered with `ingest_dispatcher`).
    blocked_relays: Arc<dyn BlockedRelayLookup>,
    /// Per-app override for the active-account bootstrap Tailing self-kinds
    /// list (`startup::SELF_KINDS_TAILING`). `None` (the default) uses the
    /// built-in `[0, 3, 10002, 10000, 10006]` list. Apps can override
    /// before `nmp_app_start` via the FFI slot to extend or narrow the
    /// reactive self-fetch — useful for apps that only care about a subset
    /// (e.g. a publish-only app needing kind:0 + kind:10002 alone) or that
    /// add app-specific replaceable kinds.
    bootstrap_self_kinds_override: Option<Vec<u32>>,
    /// Substrate `IngestParser` registry — V-40 of
    /// `docs/architecture/crate-boundaries.md`. Per-NIP crates register a
    /// parser for the kinds they own (NIP-17 kind:10050, future NIP-51
    /// list kinds, …) so the kernel never pattern-matches NIP kind numbers
    /// directly. The kernel's wildcard ingest arm fans every accepted
    /// `Inserted | Replaced` event through this dispatcher before the
    /// `KernelEventObserver`s fire. Empty by default — a kernel with no
    /// registrations is a zero-cost no-op (the dispatcher's own contract).
    ///
    /// Held behind an `Arc<RwLock<…>>` slot so `NmpApp::register_ingest_parser`
    /// can mutate the registry without crossing the actor boundary — the same
    /// slot pattern `host_op_handler`, `event_observers`, and the snapshot
    /// projection registry use.
    ingest_dispatcher: Arc<std::sync::RwLock<EventIngestDispatcher>>,
    /// Test-only handle to the [`crate::substrate::TestDmInboxRelayCache`]
    /// installed by [`Kernel::test_dm_relay_cache`]. Production composition
    /// never installs one of these — `nmp_nip17::DmRelayCache` is the
    /// production impl behind `dm_inbox_relays`. Tests inside `nmp-core` use
    /// this typed handle to seed entries without depending on `nmp-nip17`
    /// (a downstream crate cycle the doctrine forbids).
    #[cfg(any(test, feature = "test-support"))]
    test_dm_inbox_cache: Option<Arc<crate::substrate::TestDmInboxRelayCache>>,
    timeline_authors: BTreeSet<String>,
    /// T140 — M2 follow-feed interest tracking. Maps each currently-registered
    /// follow-feed `InterestId` so `sync_follow_feed_interests` can withdraw
    /// stale entries before re-registering on kind:3 change. Derived from the
    /// active account's kind:3 follow set; empty until first kind:3 arrives.
    follow_feed_interest_ids: BTreeSet<crate::planner::InterestId>,
    /// Host-declared event kinds the contact-list-authors subscription should
    /// REQ for the active account's follow set. Empty = the subscription is not
    /// active (no follow-feed interests are registered). The host (e.g. Chirp)
    /// declares its app-specific kinds via
    /// `ActorCommand::OpenContactListSubscription { kinds }`; `nmp-core` no
    /// longer hardcodes any kind set here (D0 — the substrate carries no
    /// app-specific social knowledge such as {1, 6}).
    ///
    /// `pub(crate)` so in-crate tests can seed it directly as fixture setup
    /// without triggering the `register_follow_feed_for_active_account`
    /// side-effect that `set_follow_feed_kinds` fires.
    pub(crate) follow_feed_kinds: BTreeSet<u32>,
    profile_claims: HashMap<String, BTreeSet<String>>,
    /// Generic event-claim refcount: `primary_id → BTreeSet<consumer_id>`,
    /// keyed by the same `primary_id` the snapshot's `claimed_events`
    /// projection uses (hex64 event id for nevent/note URIs;
    /// `kind:pubkey:d_tag` coordinate for naddr URIs).
    ///
    /// Driven by [`Kernel::claim_event`] / [`Kernel::release_event`]
    /// (F-CR-06 / ADR-0034). Capped per key by
    /// [`MAX_EVENT_CLAIMS_PER_KEY`]; overflow bumps
    /// [`Self::event_claim_drops_total`]. Symmetric with `profile_claims`
    /// and likewise NOT preserved across `Kernel::Reset` (claim refcounts
    /// are view-derived; views re-claim on re-open).
    event_claims: HashMap<String, BTreeSet<String>>,
    /// Set of `primary_id`s for which a `OneShot + Global` interest has
    /// already been registered with [`crate::subs::OneshotApi`] by
    /// [`Kernel::claim_event`]. Prevents the second claimer on the same
    /// id from registering a duplicate interest before the first EOSE
    /// (and the `complete_unknown_oneshot` release) has fired.
    ///
    /// An entry is removed by [`Kernel::release_event`] when the last
    /// consumer drops the claim — that lets a re-claim re-fetch (the
    /// `OneshotApi` row may have been released on EOSE long ago).
    event_claim_requested: BTreeSet<String>,
    /// Cold-start parking queue for `claim_event` calls that arrived
    /// before any relay socket reached the warm `can_send` state.
    ///
    /// Each entry is a `(uri, consumer_id)` pair — the exact arguments
    /// the host originally passed to `claim_event`. The parked claim has
    /// already been refcounted into [`Self::event_claims`] (so the
    /// renderer sees the claim row immediately) but has NOT yet
    /// registered a `OneShot + Global` interest with the OneshotApi —
    /// no relay is reachable so there is nowhere to send a REQ.
    ///
    /// Drained by [`Kernel::pending_event_claim_requests`] which the
    /// per-tick view-request dispatcher calls once at least one relay
    /// is connected. Each parked pair is replayed as a warm
    /// `claim_event(uri, consumer_id, can_send=true)` — `claim_event`
    /// is idempotent on the refcount side (the second `insert` on the
    /// same `(primary_id, consumer_id)` is a no-op) so the replay
    /// simply registers the OneshotApi interest that the cold-start
    /// path skipped.
    ///
    /// Symmetric with [`ProfileRequestState`]`.pending` and likewise
    /// NOT preserved across `Kernel::Reset` (claims are view-derived;
    /// views re-claim on re-open).
    pub(super) pending_event_claims: Vec<(String, String)>,
    /// Counter for `claim_event` attempts dropped because a single
    /// `primary_id`'s consumer set hit [`MAX_EVENT_CLAIMS_PER_KEY`].
    /// Read-only diagnostic; mirrors `claim_drops_total` for the
    /// profile-claim primitive. Not yet surfaced on the snapshot — the
    /// FFI projection seam will add it alongside the existing
    /// `claim_drops_total` in a follow-up (V-???).
    event_claim_drops_total: u64,
    /// Profile-fetch request tracking (D0 app-domain state). See
    /// [`ProfileRequestState`].
    profile_requests: ProfileRequestState,
    timeline_requested: bool,
    contacts_deadline: Option<Instant>,
    /// Wire (WebSocket) subscription bookkeeping (D0 app-domain state). See
    /// [`WireSubscriptionState`].
    ///
    /// `.subs` is keyed by `(relay_url, sub_id)`. #170: the M2 planner
    /// deliberately reuses the same `sub-*` id across relay URLs for one filter
    /// (NIP-01 §1 sub ids are per-connection; `subs/wire.rs`). A `sub_id`-only
    /// key let the second relay's REQ clobber the first's row and a CLOSE for
    /// one relay evict a still-live sibling. Same precedent as `plan_diff`
    /// (#161). The relay-URL half is a [`CanonicalRelayUrl`] — the only
    /// constructor canonicalizes, so a non-canonical key cannot be inserted and
    /// the EOSE/CLOSED lookup is guaranteed to agree.
    ///
    /// `.persistent` holds `(relay_url, sub_id)` pairs that must survive EOSE
    /// (the kernel's default policy is to auto-CLOSE any non-seed/non-firehose
    /// sub on EOSE). Protocol lanes like NWC (kind:23195 listener) register
    /// here so the wire-side subscription is kept open for the connection
    /// lifetime. #170: relay-scoped so a CLOSE for one relay never un-pins a
    /// sibling.
    wire: WireSubscriptionState,
    last_emitted_items: Vec<TimelineItem>,
    update_sequence: u64,
    /// Serialized length (bytes) of the snapshot emitted on the PREVIOUS
    /// `make_update` tick. The `Metrics::payload_bytes` diagnostic is sourced
    /// from this value so `make_update` serializes the `KernelUpdate` exactly
    /// once per tick instead of serializing-then-discarding to size the field.
    /// The reported `payload_bytes` therefore lags the actual snapshot by one
    /// tick — acceptable for a diagnostic field (no consumer treats it as
    /// authoritative; both the iOS bridge and the S3 harness measure the real
    /// frame length themselves). `0` on the first tick.
    last_payload_bytes: usize,
    last_make_update_us: u128,
    last_serialize_us: u128,
    update_frame_degradations_total: u64,
    events_since_last_update: u64,
    max_event_to_emit_ms: u128,
    max_events_per_update: u64,
    changed_since_emit: bool,
    logs: VecDeque<String>,
    /// M5+M2+M8 wiring: per-relay NIP-42 driver state. One entry per
    /// `RelayRole`. Default `NotRequired`; an inbound `AUTH` frame transitions
    /// to `ChallengeReceived` and triggers signer invocation.
    auth_drivers: HashMap<RelayRole, AuthDriverState>,
    /// M5+M2+M8 wiring: subscription lifecycle. Today the kernel uses ONLY
    /// `handle_auth_state_change` (diagnostic state fan-in to `AuthGate`); the
    /// compile / registry / wire-diff machinery stays dormant because the
    /// kernel's M1 hand-rolled `req()` path is still authoritative per
    /// `docs/plan/m8-subscription-lifecycle.md` §4 (both paths coexist until
    /// M11 migrates view modules onto `LogicalInterest`). The `AuthGate`'s
    /// pending-REQ buffer is the seam that activates on that migration;
    /// kernel-side AUTH-pause is currently routed through `defer_outbound`
    /// (the existing M1 generic queue) via `partition_auth_paused`.
    lifecycle: SubscriptionLifecycle,
    /// T82 — referenced-but-missing id collector (notedeck §3.10). Fed by the
    /// ingest seam (`collect_unknown_refs`); drained into `oneshot` fetches.
    unknown_ids: UnknownIds,
    /// T82 — transient one-shot read coordinator (notedeck §3.9). Issues
    /// `OneShot`-lifecycle interests on `lifecycle`'s registry to resolve
    /// drained `unknown_ids`; the wire lifecycle CLOSEs them on first EOSE.
    oneshot: OneshotApi,
    /// T82/T104 — discovery wire-sub-id → `(token, kind)` map so the EOSE
    /// handler can route a completed oneshot by typed [`discovery::OneshotKind`]
    /// rather than by string-prefix scan. Bounded by
    /// `Kernel::MAX_DISCOVERY_CONCURRENCY` (2): `drain_unknown_oneshots` guards
    /// the cap before inserting, so the map never grows beyond 2 entries in
    /// steady state. Entries are removed on completion.
    ///
    /// PD-033-C Stage 1: the key is the **planner-assigned `sub_id`** (`sub-<hash>`,
    /// see `subs/wire.rs::sub_id_for`), not the legacy `oneshot-disc-<token>`
    /// kernel-side label. The bridge lives in
    /// [`Kernel::register_planner_wire_frames`] — it consults
    /// `pending_discovery_oneshots` to translate `WireFrame::Req.interest_id`
    /// back into the `OneshotToken` and inserts the row under the planner sub_id.
    oneshot_subs: HashMap<String, (crate::subs::OneshotToken, discovery::OneshotKind)>,
    /// PD-033-C Stage 1 bridge: `InterestId` → `OneshotToken` map populated by
    /// [`Kernel::drain_unknown_oneshots`] for every registered discovery
    /// oneshot, consumed by [`Kernel::register_planner_wire_frames`] when the
    /// planner emits a `WireFrame::Req` for the corresponding interest. The
    /// consume step moves the entry into `oneshot_subs` keyed by the
    /// planner-assigned `sub_id` so the EOSE handler + store-gate routing
    /// (`is_discovery_oneshot`, `complete_unknown_oneshot`) work against the
    /// actual wire sub-id.
    ///
    /// Bounded by `MAX_DISCOVERY_CONCURRENCY` (2) like `oneshot_subs`: the cap
    /// on registered interests at any one time keeps this map at ≤2 entries.
    /// An entry that never sees its REQ frame compiled (no bootstrap relays,
    /// no NIP-65 mailbox, etc.) leaks until the next `register_planner_wire_frames`
    /// for the same interest_id (the planner's hash is deterministic across
    /// recompiles for the same shape, so a re-route consumes the stale entry).
    pending_discovery_oneshots: HashMap<crate::planner::InterestId, crate::subs::OneshotToken>,
    /// W5 — per-claim Phase 1/2/3 state machine entries, keyed by InterestId.
    /// §8.3: twin BTreeMaps provide O(log N) forward and reverse lookup.
    pending_claims:
        std::collections::BTreeMap<crate::planner::InterestId, claim_expansion::PendingClaim>,
    /// W5 — reverse index from wire sub_id → InterestId for O(log N) ingest lookup.
    /// Populated by `register_planner_wire_frames` when the planner assigns
    /// a sub_id to the claim's LogicalInterest.
    claim_sub_index: std::collections::BTreeMap<String, crate::planner::InterestId>,
    /// M6 signer injection, per relay role. The actor / iOS layer wires the
    /// user-identity signer for `Content`/`Indexer` from
    /// `nmp_signers::AccountManager::signer_active()`. Other lanes (e.g.
    /// `RelayRole::Wallet` for NWC) register their own per-protocol credentials
    /// — the NWC client secret signs kind:22242 against the wallet relay
    /// independently of the user's identity. Missing entry → challenges from
    /// that role are recorded but unanswered (driver stays in
    /// `ChallengeReceived` until a signer is bound for that role).
    auth_signers: HashMap<RelayRole, RelayAuthCredentials>,
    /// T66a identity/publish projections — flat wire-protocol summaries the
    /// actor pushes after each AccountManager-equivalent mutation. The actor
    /// (in `nmp-core`, so it CANNOT import `nmp-signers` per D0) owns the
    /// authoritative `nostr::Keys` map; these are the derived snapshot cache.
    accounts: Vec<AccountSummary>,
    active_account: Option<String>,
    publish_queue: Vec<PublishQueueEntry>,
    last_error_toast: Option<String>,
    /// Machine-readable category for `last_error_toast` (typed FFI error
    /// contract). Closed key set lives in `kernel::closed_reason`. Set by
    /// `set_error_toast_with_category`; cleared by the legacy
    /// `set_last_error_toast` so a newer uncategorized toast never leaves a
    /// stale category shadowing it.
    last_error_category: Option<String>,
    relay_edit_rows: Vec<RelayEditRow>,
    // D0: NIP-47 NWC is an app noun. Wallet state is no longer a kernel field
    // — the actor's wallet runtime owns it and the `projections["wallet"]`
    // snapshot projection surfaces it. The kernel holds no NWC state.
    //
    // D0: NIP-46 remote signing is likewise an app noun. Bunker handshake
    // state is no longer a kernel field — the actor's identity runtime owns it
    // and the `projections["bunker_handshake"]` snapshot projection surfaces
    // it. The kernel holds no NIP-46 handshake state.
    /// T117 — the publish engine drives the per-(event, relay) retry FSM
    /// (`publish/state.rs`). Mandatory on every Kernel; previously the
    /// kernel one-shotted a single EVENT frame and the engine was dead code
    /// (relay-lifecycle review §G5). Now every `publish_signed` builds a
    /// `PublishAction::Publish`, drives the engine, and drains the queue
    /// dispatcher into outbound frames. Per-relay OKs are folded back via
    /// `Kernel::handle_publish_ok` (called from `ingest::handle_text`).
    /// Actor-owned tracker for the snapshot-mirror `action_stages`
    /// projection. Records lifecycle transitions per dispatched `correlation_id`
    /// and retains them until the host acks via `nmp_app_ack_action_stage`.
    /// Caps and drop-oldest semantics live in [`action_stages`].
    action_stages: action_stages::ActionStageTracker,
    /// Actor-owned tracker for the `action_lifecycle` display projection
    /// (V5 thin-shell fix). Mirrors every transition the substrate-level
    /// `action_stages` tracker records, but collapses to the latest stage
    /// per correlation_id and drops terminals on a wall-clock TTL — no
    /// host ack required. Drives the host's spinner/toast UI without any
    /// reducer-side bookkeeping in the shell.
    action_lifecycle: action_lifecycle::ActionLifecycleTracker,
    publish_engine: crate::publish::PublishEngine,
    /// Buffered (`relay_url`, frame) pairs produced by the engine. The kernel
    /// drains this after each engine call and wraps the pairs as
    /// `OutboundMessage`s on the `RelayRole::Content` lane (the publish
    /// lane). Shared `Arc` so the engine's `Arc<dyn RelayDispatcher>` and the
    /// kernel both see the same buffer.
    publish_dispatcher: Arc<crate::publish::QueueDispatcher>,
    /// Durable publish-state store. Defaulted to in-memory for production
    /// today (M3 LMDB lands later). Held as `Arc` so tests can construct a
    /// second kernel sharing the same store to prove resume-from-store.
    #[allow(dead_code)]
    publish_store: Arc<dyn crate::publish::PublishStore>,
    /// T131 — per-URL first-source / duplicate / replaced / rejected
    /// counters, fed at `ingest/timeline.rs:68` from the store's
    /// `InsertOutcome` discriminator. The diagnostic projection
    /// (F4, future task) folds this into `KernelUpdate::relay_diagnostics`
    /// to expose `RelayUsefulness.novelty_ratio`
    /// (`docs/design/outbox-explorer-diagnostics.md` §2 line 152).
    pub(in crate::kernel) event_provenance: provenance::EventProvenance,
    /// T114b — count of `claim_profile` requests dropped because a single
    /// pubkey's `consumer_id` set hit `MAX_CLAIMS_PER_PUBKEY`. Surfaced on the
    /// snapshot via [`Metrics::claim_drops_total`] for D8 visibility into
    /// per-dispatch retention pressure.
    claim_drops_total: u64,
    /// T114b — diagnostic dispatch-drop counter (the same `Arc<AtomicU64>`
    /// owned by the FFI forwarder in `actor/mod.rs`). Under the current
    /// unbounded dual-channel design this is always zero (commands cannot be
    /// dropped); retained for API/diagnostic compatibility. `None` when the
    /// kernel is constructed outside the actor (tests, codegen); the snapshot
    /// then reports `dispatch_drops_total = 0`. Surfaced on the snapshot via
    /// [`Metrics::dispatch_drops_total`].
    dispatch_drops: Option<Arc<AtomicU64>>,
    /// G-S4 — actor command-channel depth straddle counter (the same
    /// `Arc<AtomicU64>` `NmpApp::send_cmd` increments and the actor loop
    /// decrements per dequeued command). The kernel only reads it, surfacing
    /// the value as [`Metrics::actor_queue_depth`] in `make_update`. `None`
    /// when the kernel is constructed outside the actor (tests, codegen); the
    /// snapshot then reports `actor_queue_depth = 0`. Bound once by
    /// `run_actor_with_observers` and rebound by the `Reset` path the same way
    /// `dispatch_drops` is.
    queue_depth: Option<Arc<AtomicU64>>,
    /// T118 / G3 — current iOS scenePhase reported through the lifecycle
    /// FFI. Starts as [`LifecyclePhase::Inactive`] (the sentinel meaning
    /// "shell hasn't reported a phase yet"). `set_lifecycle_phase`
    /// debounces repeated phases and returns the transition verdict the
    /// actor uses to drive the observer callback.
    lifecycle_phase: LifecyclePhase,
    /// T146 — kernel event observer slot. Integration lives in
    /// `kernel/event_observer.rs`; `None` until the actor binds the
    /// shared `Arc<Mutex<…>>` via `set_event_observers_handle`.
    event_observers: Option<crate::actor::KernelEventObserverSlot>,
    /// Raw signed-event tap slot. Integration lives in
    /// `kernel/raw_event_observer.rs`; `None` until the actor binds the
    /// shared `Arc<Mutex<…>>` via `set_raw_event_observers_handle`.
    /// Delivers the verbatim flat NIP-01 signed event (`sig` included)
    /// from the single all-kinds ingest point after the existing
    /// Schnorr + id-hash gate. Generic capability (D0) — no protocol nouns.
    raw_event_observers: Option<crate::actor::RawEventObserverSlot>,
    /// Host-extensible snapshot output slot. Integration lives in
    /// `kernel/snapshot_registry.rs`; `None` until the actor binds the
    /// shared `Arc<Mutex<…>>` via `set_snapshot_projection_handle`. Each
    /// registered closure runs in `make_update` and contributes a namespaced
    /// JSON value to `KernelSnapshot::projections`. The output-side
    /// counterpart to the action registry (D0 — the kernel emits, never
    /// names a host noun).
    snapshot_projections: Option<SnapshotProjectionSlot>,
    /// Shared handle to the relay-edit rows so the FFI layer can read the
    /// current user-configured write relays without
    /// importing kernel internals. Synced by `set_relay_edit_rows` in
    /// `identity_state.rs`.
    ///
    /// Slot type is [`RelayEditRowsSlot`] (`Arc<Mutex<RelayEditRowList>>`);
    /// D14 forbids bare `Arc<Mutex<Vec<…>>>` fields on `Kernel` and the
    /// typed wrapper makes the slot's purpose visible at the declaration site.
    relay_edit_rows_handle: Option<RelayEditRowsSlot>,
    /// Shared list of indexer relay URLs, kept in sync with `relay_edit_rows`
    /// by `set_relay_edit_rows`. The `Nip65OutboxResolver` holds a clone of
    /// this Arc and reads it on every discovery-kind publish.
    ///
    /// Typed slot ([`IndexerRelaysSlot`]) so the bare-`Vec` shape
    /// disappears from the field declaration (D14).
    indexer_relays_handle: IndexerRelaysSlot,
    /// Shared list of local write relays for the active account. This bridges
    /// onboarding relay rows into publish routing before the user's freshly
    /// published kind:10002 has round-tripped from a relay.
    ///
    /// Typed slot ([`LocalWriteRelaysSlot`]) — see `relay_projection.rs`.
    local_write_relays_handle: LocalWriteRelaysSlot,
    /// Shared active-account pubkey used by the publish resolver to scope the
    /// local relay-row fallback to the viewer's own events only.
    active_account_handle: ActiveAccountSlot,
    /// W2 — in-memory relay-author score map. D4: the kernel is the sole
    /// writer. W3 will record outcomes via `record_*`; W2 flushes to LMDB
    /// on actor idle via `flush_relay_scores_if_dirty`. Default: empty.
    relay_score_map: relay_score::RelayAuthorScoreMap,
    /// W2 — pluggable relay-author-score persistence store. `None` when the
    /// kernel is constructed in-memory-only (tests, CI without lmdb-backend).
    /// Set by `set_relay_score_store` after construction. D4: the kernel
    /// holds `Box` (not `Arc`) because it is the sole logical writer.
    relay_score_store: Option<Box<dyn crate::substrate::RelayAuthorScoreStore>>,
    /// Kernel must not cross thread boundaries — D4 single-writer enforced at type level.
    _not_send: PhantomData<*const ()>,
}

struct EventStoreBundle {
    store: Arc<dyn EventStore>,
    relay_score_store: Option<Box<dyn crate::substrate::RelayAuthorScoreStore>>,
}

/// Construct the kernel's `EventStore`.
///
/// Default: `MemEventStore` — used by all tests and the pre-M15 web target.
///
/// When compiled with `--features lmdb-backend`, an `LmdbEventStore` is
/// opened when a persistent path is available. The path is resolved in
/// priority order:
///
/// 1. `storage_path` — the FFI-supplied path threaded through from
///    `nmp_app_set_storage_path` (production iOS / Android). When the host
///    sets it before `nmp_app_start`, this is the path used.
/// 2. `NMP_LMDB_PATH` environment variable — the pre-existing opt-in
///    mechanism, kept for tests and tools that drive the kernel without
///    the FFI surface.
///
/// When neither is present (the common case for the in-process test
/// suites) the in-memory store is used. If the LMDB store cannot be
/// opened, the function falls back to the in-memory store silently — D6:
/// library code performs no I/O side effects / stderr writes.
fn build_event_store(storage_path: Option<&str>) -> EventStoreBundle {
    #[cfg(feature = "lmdb-backend")]
    {
        // Priority 1: FFI-supplied path. Priority 2: env-var fallback.
        let resolved: Option<String> = storage_path
            .map(str::to_owned)
            .or_else(|| std::env::var("NMP_LMDB_PATH").ok());
        if let Some(path) = resolved {
            // D6: silent fallback to the in-memory store if the open fails.
            if let Ok(s) = crate::store::LmdbEventStore::open(std::path::Path::new(&path)) {
                let relay_score_store =
                    crate::substrate::LmdbRelayAuthorScoreStore::from_event_store(s.clone());
                return EventStoreBundle {
                    store: Arc::new(s),
                    relay_score_store: Some(Box::new(relay_score_store)),
                };
            }
        }
    }
    // `storage_path` is unused when the `lmdb-backend` feature is off.
    #[cfg(not(feature = "lmdb-backend"))]
    let _ = storage_path;
    EventStoreBundle {
        store: Arc::new(MemEventStore::new()),
        relay_score_store: None,
    }
}

/// Choose the [`PublishStore`](crate::publish::PublishStore) backing the
/// publish engine.
///
/// Publish intents composed offline only survive an app kill if the store is
/// durable - `PublishEngine::resume_from_store` replays exactly what
/// `load_pending` returns at startup. There are three backends:
///
/// 1. [`FsPublishStore`](crate::publish::FsPublishStore) - JSON files under
///    `{storage_path}/publish_intents/`. Durable **without** any feature flag,
///    so it is the chosen backend whenever the host supplied a storage path.
/// 2. [`DomainPublishStore`](crate::publish::DomainPublishStore) - LMDB-backed
///    via the shared `EventStore`. Durable *only* with `--features
///    lmdb-backend`; without it the underlying store is `MemEventStore` and
///    intents are lost on restart. Kept as the fallback when no storage path
///    is set but the event store still opened cleanly.
/// 3. [`InMemoryPublishStore`](crate::publish::InMemoryPublishStore) - last
///    resort (and the steady state for CI / in-process tests, which pass no
///    storage path).
///
/// Resolution mirrors [`build_event_store`]: the FFI-supplied `storage_path`
/// wins, then the `NMP_LMDB_PATH` env-var fallback. When a path resolves, the
/// `FsPublishStore` is rooted at the *same* directory as the LMDB event store
/// so one `storage_path` covers all durable kernel state.
fn resolve_publish_store(
    storage_path: Option<&str>,
    event_store: &Arc<dyn EventStore>,
) -> Arc<dyn crate::publish::PublishStore> {
    let resolved = resolve_storage_path(storage_path);
    if let Some(path) = resolved {
        // Durable, feature-flag-independent: offline intents survive restart.
        return Arc::new(crate::publish::FsPublishStore::new(path));
    }
    // No storage path: fall back to the LMDB-domain store (durable only under
    // `lmdb-backend`), then the in-memory store. This keeps CI/test behaviour
    // (no storage path -> no on-disk artefacts) unchanged.
    crate::publish::DomainPublishStore::open(Arc::clone(event_store)).map_or_else(
        |_| {
            Arc::new(crate::publish::InMemoryPublishStore::new())
                as Arc<dyn crate::publish::PublishStore>
        },
        |store| Arc::new(store) as Arc<dyn crate::publish::PublishStore>,
    )
}

fn resolve_storage_path(storage_path: Option<&str>) -> Option<String> {
    storage_path
        .map(str::to_owned)
        .or_else(|| std::env::var("NMP_LMDB_PATH").ok())
}

fn load_profile_intents(
    publish_store: &Arc<dyn crate::publish::PublishStore>,
) -> HashMap<String, Profile> {
    let mut intents = HashMap::new();
    let Ok(records) = publish_store.load_pending() else {
        return intents;
    };
    for record in records {
        let Some(profile) = nostr::parse_profile_intent(&record.event) else {
            continue;
        };
        let pubkey = record.event.unsigned.pubkey;
        let should_replace = intents
            .get(&pubkey)
            .is_none_or(|existing: &Profile| existing.created_at <= profile.created_at);
        if should_replace {
            intents.insert(pubkey, profile);
        }
    }
    intents
}

impl Kernel {
    pub(crate) fn new(visible_limit: usize) -> Self {
        Self::with_storage_path(visible_limit, None)
    }

    /// Construct a Kernel, optionally backing the `EventStore` with a
    /// persistent LMDB path.
    ///
    /// `storage_path` is the FFI-supplied directory threaded through from
    /// `nmp_app_set_storage_path`. It is only honoured when the crate is
    /// built with `--features lmdb-backend`; without that feature (or when
    /// `storage_path` is `None`) the in-memory store is used. The actor
    /// thread is the sole caller that passes a non-`None` path — every test
    /// site goes through [`Kernel::new`], which passes `None` and so keeps
    /// the in-memory backend.
    pub fn with_storage_path(visible_limit: usize, storage_path: Option<&str>) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, None, storage_path)
    }

    /// Inject a production routing pair (substrate
    /// [`OutboxRouter`] + [`MailboxCache`] impls).
    ///
    /// Step 3 of the crate-boundary migration
    /// (`docs/architecture/crate-boundaries.md` §3) wires
    /// `Arc<dyn OutboxRouter>` + `Arc<dyn MailboxCache>` onto the
    /// kernel. Production composition (apps that depend on
    /// `nmp-router`) calls this after `Kernel::new` /
    /// `Kernel::with_storage_path` to swap the
    /// [`crate::substrate::EmptyOutboxRouter`] +
    /// [`crate::substrate::EmptyMailboxCache`] defaults (substrate-honest
    /// debt B, 2026-05-24) for `nmp_router::GenericOutboxRouter` +
    /// `nmp_router::InMemoryMailboxCache`. The kernel itself cannot
    /// depend on `nmp-router` (Layer 3 → Layer 2 would invert the
    /// dependency arrow), so injection is mandatory for the production
    /// swap.
    ///
    /// MUST be called BEFORE any kind:10002 event is ingested — the
    /// caches are independent stores, not a write-through pair, so a
    /// swap after ingest would lose the cached entries.
    ///
    /// Widened from `pub(crate)` to `pub` (V-51 phase 5): production
    /// composition (`nmp-app-chirp`) now drives this through the
    /// `NmpApp::set_routing_substrate` slot the actor's kernel
    /// constructor reads. Apps that want a competing router
    /// (`nmp_router::GenericOutboxRouter`, or a future Layer-2 impl)
    /// inject through that slot; the actor calls this method after
    /// `Kernel::with_storage_path` returns, threading the kernel's
    /// `RoutingTraceProjection` through the supplied router's
    /// `with_trace_observer` so the trace ring keeps populating across
    /// the swap.
    pub fn set_routing(&mut self, router: Arc<dyn OutboxRouter>, cache: Arc<dyn MailboxCache>) {
        self.outbox_router = router;
        self.mailbox_cache = cache;
    }

    /// Install a router-side publish-resolver implementation on the
    /// kernel's `PublishEngine`.
    ///
    /// Spec §271 (2026-05-25): `Nip65OutboxResolver` lives in `nmp-router`,
    /// not `nmp-core`. The kernel constructs `PublishEngine` with the
    /// in-crate `NoopOutboxResolver` default (every `PublishTarget::Auto`
    /// resolves to an empty set → `PublishEngineError::NoTargets`,
    /// fail-closed). Production composition
    /// (`nmp-app-template::register_defaults` → the
    /// `NmpApp::set_publish_resolver_factory` slot the actor reads at
    /// kernel construction) calls this method right after
    /// [`Self::set_routing`] to install
    /// `nmp_router::Nip65OutboxResolver::with_local_relays(...)` over the
    /// kernel-owned [`event_store_handle`](Self::event_store_handle) /
    /// [`indexer_relays_handle`](Self::indexer_relays_handle) /
    /// [`local_write_relays_handle`](Self::local_write_relays_handle) /
    /// [`active_account_handle`](Self::active_account_handle) slots.
    ///
    /// MUST be called BEFORE any publish lands. Swapping mid-publish leaves
    /// the in-flight engine state inconsistent with the resolver decisions
    /// that produced it.
    pub fn set_publish_resolver(&mut self, resolver: Arc<dyn crate::publish::OutboxResolver>) {
        self.publish_engine.set_outbox(resolver);
    }

    /// W2 — inject the relay-author-score persistence store and hydrate the
    /// in-memory map from it.
    ///
    /// Must be called before any score observations are recorded. On
    /// `load_all` error the map stays empty (D6 — silent fallback). Calling
    /// this more than once replaces the store and re-hydrates the map from
    /// scratch.
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

    /// Record a relay-author score outcome.
    ///
    /// W3 entry-point: called by the claim-lifecycle layer when a relay
    /// delivers (Hit), EOSEs without a match (EoseNoMatch), or fails
    /// (Failed). Marks the map dirty so the next idle flush persists it.
    ///
    /// D4: `&mut self` — the kernel is the sole writer of the score map.
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

    /// Look up the current `RelayAuthorScore` for `(author, relay_url)`.
    ///
    /// W4/W5 read path: warm-relay filter and claim expansion call this to
    /// decide whether a relay is eligible for Phase-1 bias.
    ///
    /// Unknown cells return a zero-cell (D6: total). The URL is
    /// canonicalized internally.
    #[must_use]
    pub fn get_relay_score(&self, author: &str, relay_url: &str) -> relay_score::RelayAuthorScore {
        self.relay_score_map.get(&author.to_string(), relay_url)
    }

    /// Test-only: whether the score map has unsaved mutations.
    ///
    /// Production code must not gate behaviour on this flag — the map is
    /// dirty or clean as a side-effect of `record_relay_score` /
    /// `flush_relay_scores_if_dirty`. Tests use it to assert flush semantics.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn test_relay_score_dirty(&self) -> bool {
        self.relay_score_map.is_dirty()
    }

    /// Borrow the kernel's `EventStore` handle.
    ///
    /// Returned as a cloned `Arc<dyn EventStore>` (the kernel uses `Arc` so
    /// the resolver can share the same store without a second copy). Used
    /// by the `set_publish_resolver_factory` composition site to construct
    /// `nmp_router::Nip65OutboxResolver::with_local_relays(store, ...)`
    /// over the same store the kernel reads kind:10002 from. Spec §271
    /// (2026-05-25).
    #[must_use]
    pub fn event_store_handle(&self) -> Arc<dyn EventStore> {
        Arc::clone(&self.store)
    }

    /// Borrow the kernel's indexer-relays slot.
    ///
    /// The actor pushes the configured indexer URL list into this slot on
    /// every relay-config mutation (D4 sole-writer); router-side resolvers
    /// (`nmp_router::Nip65OutboxResolver`) read through it without crossing
    /// the kernel boundary. Spec §271 (2026-05-25).
    #[must_use]
    pub fn indexer_relays_handle(&self) -> IndexerRelaysSlot {
        Arc::clone(&self.indexer_relays_handle)
    }

    /// Borrow the kernel's local-write-relays slot. See
    /// [`Self::indexer_relays_handle`] for the threading model.
    #[must_use]
    pub fn local_write_relays_handle(&self) -> LocalWriteRelaysSlot {
        Arc::clone(&self.local_write_relays_handle)
    }

    /// Borrow the kernel's active-account-pubkey slot. See
    /// [`Self::indexer_relays_handle`] for the threading model.
    #[must_use]
    pub fn active_account_handle(&self) -> ActiveAccountSlot {
        Arc::clone(&self.active_account_handle)
    }

    /// V-51 phase 1 — borrow the kernel's routing-trace projection.
    ///
    /// Returns an `Arc<RoutingTraceProjection>` so a host that swaps in a
    /// production router (`nmp_router::GenericOutboxRouter`) via
    /// [`Self::set_routing`] can pass the same projection through the
    /// router's `with_trace_observer` builder, and so phase 2's FFI snapshot
    /// surface can read the rings without holding a `&Kernel` borrow.
    ///
    /// V-51 phase 4 widens this from `pub(crate)` to `pub`: the validation
    /// harness (`nmp-testing`) and the chirp-repl `routing-trace`
    /// subcommand need to read the projection through a held `&Kernel`
    /// reference, and `NmpApp` publishes one clone into a shared slot at
    /// actor startup so callers can read it without holding the kernel
    /// directly.
    #[must_use]
    pub fn routing_trace(&self) -> Arc<routing_trace::RoutingTraceProjection> {
        Arc::clone(&self.routing_trace)
    }

    /// Construct a Kernel with an externally-supplied publish store. Used by
    /// integration tests that need two kernel instances to share one store
    /// (proving `PublishEngine::resume_from_store` survives a "restart"). The
    /// publish engine is built against this store + the kernel's NIP-65
    /// outbox resolver + a `QueueDispatcher` shared with the kernel for
    /// frame drainage.
    ///
    /// Gated on `cfg(any(test, feature = "test-support"))`: the production
    /// `Kernel::new` path routes through [`Kernel::with_storage_path`] (added
    /// for the FFI LMDB-path wiring), so the callers of this
    /// externally-supplied-store constructor are the `publish_engine_tests`
    /// cases and the NIP golden-tag `ConformanceHarness` (which keeps a clone
    /// of the store `Arc` to read back published events).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_publish_store(
        visible_limit: usize,
        publish_store: Arc<dyn crate::publish::PublishStore>,
    ) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, Some(publish_store), None)
    }

    /// Inner constructor: externally-supplied publish store + optional
    /// persistent LMDB `storage_path`. [`Kernel::with_publish_store`] (path
    /// `None`) and [`Kernel::with_storage_path`] (in-memory publish store)
    /// both funnel here so the body lives in exactly one place.
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
        let store_bundle = build_event_store(storage_path);
        let store = store_bundle.store;
        let publish_store =
            publish_store.unwrap_or_else(|| resolve_publish_store(storage_path, &store));
        let local_profile_intents = load_profile_intents(&publish_store);
        let publish_dispatcher = Arc::new(crate::publish::QueueDispatcher::new());
        // Typed-slot constructors so the slot's purpose is visible at
        // the call site and D14 does not fire on the field declaration.
        let indexer_relays_handle: IndexerRelaysSlot = new_indexer_relays_slot();
        let local_write_relays_handle: LocalWriteRelaysSlot = new_local_write_relays_slot();
        let active_account_handle: ActiveAccountSlot = new_active_account_slot();
        // Spec §271 (2026-05-25): `Nip65OutboxResolver` lives in
        // `nmp-router`, not `nmp-core`. The engine is built with the
        // in-crate `NoopOutboxResolver` default; production composition
        // (`nmp-app-template::register_defaults` → the
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

        // T129 — install the store-backed watermark resolver on the
        // subscription lifecycle. On reconnect, `recompile_and_diff` bumps
        // each non-ephemeral sub-shape's `since` to the newest stored
        // `created_at` matching that shape, so the relay does not re-emit
        // events already on disk. The closure captures a clone of the
        // `EventStore` handle and translates the `InterestShape` into a
        // `StoreQuery`: `AuthorKind` when the shape is scoped to exactly one
        // author with ≥1 kind, `KindTime` when there are no authors but ≥1
        // kind, and `None` (no rewrite) otherwise. `query_visit` with
        // `limit = 1` early-stops at the newest stored match on the relevant
        // secondary index (D8: no per-emit allocation).
        let watermark_store = Arc::clone(&store);
        let watermark_fn: crate::subs::WatermarkFn =
            Arc::new(move |shape: &crate::planner::InterestShape| {
                // `InterestShape::{authors,kinds}` are `BTreeSet`s; the
                // `StoreQuery` variants take `Vec<u32>`.
                let kinds: Vec<u32> = shape.kinds.iter().copied().collect();
                let query = if shape.authors.len() == 1 && !kinds.is_empty() {
                    // Exactly one author + ≥1 kind → `idx_author_kind` scan.
                    // Malformed hex → no watermark (never query the zero pubkey).
                    let author_hex = shape.authors.iter().next()?;
                    let author = hex_to_pubkey_bytes(author_hex)?;
                    crate::store::StoreQuery::AuthorKind {
                        author,
                        kinds,
                        since: None,
                        until: None,
                    }
                } else if !kinds.is_empty() {
                    // No (or multi-) author + ≥1 kind → `idx_kind_time` scan.
                    crate::store::StoreQuery::KindTime {
                        kinds,
                        since: None,
                        until: None,
                    }
                } else {
                    return None;
                };
                let mut ts: Option<u64> = None;
                let _ = watermark_store.query_visit(&query, 1, &mut |ev| {
                    ts = Some(ev.raw.created_at);
                    std::ops::ControlFlow::Break(())
                });
                ts
            });
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

        // Spec §271 (2026-05-25): under `cfg(test)` / `feature="test-support"`
        // the kernel auto-installs the in-crate `TestKind10002OutboxResolver`
        // (a minimal kind:10002 reader) so the dozens of in-tree publish
        // tests (`publish_engine_tests`, `outbox_tests`, `action_failure_tests`,
        // `publish_terminal_status_tests`, `eose_ok_notice_ingest_tests`,
        // `actor::commands::tests`, `kernel::test_support::seed_kind10002_for_test`
        // consumers) keep working without each test calling
        // `Kernel::set_publish_resolver` manually. Production builds use the
        // `NoopOutboxResolver` default the engine was built with above; the
        // production composition site (`nmp-app-template::register_defaults`)
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

        let mut kernel = Self {
            store,
            clock: Arc::new(SystemClock),
            rev: 0,
            visible_limit,
            timing: TimingMilestones::default(),
            relays: RelayRole::all()
                .into_iter()
                .map(|role| (role, RelayHealth::default()))
                .collect(),
            transport_relays: RelayTransportMap::default(),
            profiles: HashMap::new(),
            local_profile_intents,
            events: HashMap::new(),
            metric_note_events: 0,
            metric_duplicate_events: 0,
            metric_stored_events: 0,
            timeline: VecDeque::new(),
            author_view: AuthorViewState::default(),
            thread_view: ThreadViewState::default(),
            diagnostic_firehose: DiagnosticFirehoseState::default(),
            deferred_outbound: VecDeque::new(),
            seed_contacts: HashMap::new(),
            #[cfg(any(test, feature = "test-support"))]
            mailbox_cache: Arc::new(TestInMemoryMailboxCache::new()),
            #[cfg(not(any(test, feature = "test-support")))]
            mailbox_cache: Arc::new(EmptyMailboxCache::new()),
            outbox_router,
            routing_trace,
            dm_inbox_relays: empty_dm_inbox_relay_lookup(),
            blocked_relays: empty_blocked_relay_lookup(),
            bootstrap_self_kinds_override: None,
            ingest_dispatcher: Arc::new(std::sync::RwLock::new(EventIngestDispatcher::new())),
            #[cfg(any(test, feature = "test-support"))]
            test_dm_inbox_cache: None,
            timeline_authors: BTreeSet::new(),
            follow_feed_interest_ids: BTreeSet::new(),
            follow_feed_kinds: BTreeSet::new(),
            profile_claims: HashMap::new(),
            event_claims: HashMap::new(),
            event_claim_requested: BTreeSet::new(),
            pending_event_claims: Vec::new(),
            event_claim_drops_total: 0,
            profile_requests: ProfileRequestState::default(),
            timeline_requested: false,
            contacts_deadline: None,
            wire: WireSubscriptionState::default(),
            last_emitted_items: Vec::new(),
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
            accounts: Vec::new(),
            active_account: None,
            publish_queue: Vec::new(),
            last_error_toast: None,
            last_error_category: None,
            relay_edit_rows: Vec::new(),
            action_stages: action_stages::ActionStageTracker::new(),
            action_lifecycle: action_lifecycle::ActionLifecycleTracker::new(),
            publish_engine,
            publish_dispatcher,
            publish_store,
            event_provenance: provenance::EventProvenance::new(),
            claim_drops_total: 0,
            dispatch_drops: None,
            queue_depth: None,
            lifecycle_phase: LifecyclePhase::Inactive,
            event_observers: None,
            raw_event_observers: None,
            snapshot_projections: None,
            relay_edit_rows_handle: None,
            indexer_relays_handle,
            local_write_relays_handle,
            active_account_handle,
            relay_score_map: relay_score::RelayAuthorScoreMap::new(),
            relay_score_store: None,
            _not_send: PhantomData,
        };
        if let Some(store) = store_bundle.relay_score_store {
            kernel.set_relay_score_store(store);
        }
        kernel
    }

    /// Swap the kernel's wall-clock. Test / replay seam: production never
    /// calls this (the default `SystemClock` installed in
    /// [`Kernel::with_publish_store_and_path`] stays in place), but
    /// deterministic-replay tests inject a `FixedClock` so the reducer's
    /// `created_at` / `received_at_ms` output is reproducible. Exercised by
    /// `kernel/clock_injection_tests.rs`. The `test-support` exposure lets
    /// external crate integration tests call this seam without `cfg(test)`.
    // `allow(dead_code)`: called from `#[cfg(test)]` code only in nmp-core;
    // external crate integration tests reach it via the `test-support` feature.
    #[cfg_attr(not(test), allow(dead_code))]
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.clock = clock;
    }

    /// Current wall-clock time as whole seconds since the Unix epoch, read
    /// through the injected [`Clock`]. D9: time decisions inside the kernel
    /// boundary route through the kernel-owned clock, never a bare
    /// `SystemTime::now()`. Actor command handlers stamp event `created_at`
    /// via this accessor so `FixedClock` makes those timestamps testable.
    ///
    /// `pub` so NIP-crate runtimes (`nmp-nip47` post-V-38) running on the
    /// actor thread can stamp `created_at` via the kernel-owned clock.
    pub fn now_secs(&self) -> u64 {
        self.clock
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Current wall-clock time as milliseconds since the Unix epoch, read
    /// through the injected [`Clock`]. Used by the `action_stages` mirror
    /// so per-stage timestamps survive `FixedClock` injection and
    /// stay deterministic in tests/replay. A pre-epoch clock collapses to
    /// `0` (D6 — never panics).
    pub(crate) fn now_ms(&self) -> u64 {
        self.clock
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Resolve the configured bootstrap URLs for a given `RelayRole` from the
    /// app-provided `relay_edit_rows`.  When no relays are configured for the
    /// requested role, falls back to the well-known defaults so that cold-start
    /// sign-ins always have discovery relays available in production.
    pub(crate) fn bootstrap_urls_for_role(&self, role: RelayRole) -> Vec<String> {
        let matches = |row_role: &str| match role {
            RelayRole::Content => {
                crate::actor::has_role(row_role, "read")
                    || crate::actor::has_role(row_role, "write")
            }
            RelayRole::Indexer => crate::actor::has_role(row_role, "indexer"),
            RelayRole::Wallet => false,
        };
        let mut urls: Vec<String> = self
            .relay_edit_rows
            .iter()
            .filter(|r| matches(&r.role))
            .map(|r| r.url.clone())
            .collect();
        if urls.is_empty() {
            urls = match role {
                RelayRole::Content => {
                    vec![crate::relay::FALLBACK_CONTENT_RELAY.to_string()]
                }
                RelayRole::Indexer => {
                    vec![crate::relay::FALLBACK_INDEXER_RELAY.to_string()]
                }
                RelayRole::Wallet => Vec::new(),
            };
        }
        urls
    }

    /// The cold-start discovery seed as an owned `Vec`.  Reads from the
    /// app-provided `relay_edit_rows`; returns an empty vec when nothing is
    /// configured yet.
    pub(crate) fn bootstrap_discovery_relays(&self) -> Vec<String> {
        let mut urls: Vec<String> = self
            .bootstrap_urls_for_role(RelayRole::Indexer)
            .into_iter()
            .chain(self.bootstrap_urls_for_role(RelayRole::Content))
            .collect();
        sort_dedup(&mut urls);
        urls
    }

    /// T114b — install the actor's FFI-channel drop counter so the diagnostic
    /// snapshot surfaces it. Idempotent: re-binding replaces the prior handle.
    /// `None`-on-construction is fine — the snapshot reports zero when unbound.
    /// Called once by `run_actor` immediately after the kernel is built.
    pub(crate) fn set_dispatch_drops_handle(&mut self, handle: Arc<AtomicU64>) {
        self.dispatch_drops = Some(handle);
    }

    /// T114b — extract the FFI-channel drop-counter handle before a `Reset`
    /// replaces the kernel. The dispatch drops counter is process-lifetime
    /// (shared with the FFI forwarder thread) so the Reset path moves it
    /// onto the fresh kernel via `set_dispatch_drops_handle`.
    pub(crate) fn take_dispatch_drops_handle_for_reset(&mut self) -> Option<Arc<AtomicU64>> {
        self.dispatch_drops.take()
    }

    /// T114b — diagnostic counter; always 0 under the current unbounded
    /// dual-channel design. Retained for API compatibility. Also returns 0
    /// when the kernel was constructed outside the actor (tests, codegen)
    /// and no handle is bound.
    pub(crate) fn dispatch_drops_total(&self) -> u64 {
        self.dispatch_drops
            .as_ref()
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// G-S4 — install the actor's command-channel depth counter so the
    /// diagnostic snapshot surfaces it as `actor_queue_depth`. Idempotent:
    /// re-binding replaces the prior handle. `None`-on-construction is fine —
    /// the snapshot reports zero when unbound (tests, codegen). Called once by
    /// `run_actor_with_observers` immediately after the kernel is built.
    pub(crate) fn set_queue_depth_handle(&mut self, handle: Arc<AtomicU64>) {
        self.queue_depth = Some(handle);
    }

    /// G-S4 — extract the queue-depth counter handle before a `Reset` replaces
    /// the kernel. The counter is process-lifetime (shared with `NmpApp`'s
    /// `send_cmd`) so the Reset path moves it onto the fresh kernel via
    /// `set_queue_depth_handle`.
    pub(crate) fn take_queue_depth_handle_for_reset(&mut self) -> Option<Arc<AtomicU64>> {
        self.queue_depth.take()
    }

    /// G-S4 — current actor command-channel depth (`send_cmd` increments,
    /// the actor loop decrements per dequeued command). Returns 0 when the
    /// kernel was constructed outside the actor and no handle is bound.
    /// Saturates at `u32::MAX` because `Metrics::actor_queue_depth` is `u32`.
    pub(crate) fn actor_queue_depth(&self) -> u32 {
        let depth = self
            .queue_depth
            .as_ref()
            .map_or(0, |c| c.load(Ordering::Relaxed));
        depth.min(u64::from(u32::MAX)) as u32
    }

    /// T114b — number of `claim_profile` requests dropped because a pubkey's
    /// `consumer_id` set hit `MAX_CLAIMS_PER_PUBKEY`. Read-only accessor; the
    /// counter is owned by the kernel and mutated only by `claim_profile`.
    pub(crate) fn claim_drops_total(&self) -> u64 {
        self.claim_drops_total
    }

    #[cfg(test)]
    pub(crate) fn claim_drops_total_test(&self) -> u64 {
        self.claim_drops_total
    }

    /// Return the lightning address / LNURL from the author's cached kind:0
    /// profile, or `None` if the profile hasn't arrived yet or has no
    /// lightning address. Used by `ProtocolCommandContext::lnurl_for_pubkey`
    /// so `FetchLnurlInvoiceCommand` can resolve the destination without
    /// the shell having to carry or know about LNURL.
    pub(crate) fn lnurl_for_pubkey(&self, pubkey: &str) -> Option<String> {
        self.profiles.get(pubkey)?.lnurl.clone()
    }

    #[cfg(test)]
    pub(crate) fn profile_claims_len_for_test(&self, pubkey: &str) -> usize {
        self.profile_claims
            .get(pubkey)
            .map(|consumers| consumers.len())
            .unwrap_or(0)
    }

    /// Test-only: number of consumers currently holding a `claim_event`
    /// on `primary_id`. Mirrors `profile_claims_len_for_test`.
    #[cfg(test)]
    pub(crate) fn event_claims_len_for_test(&self, primary_id: &str) -> usize {
        self.event_claims
            .get(primary_id)
            .map(|consumers| consumers.len())
            .unwrap_or(0)
    }

    /// Test-only: `claim_event` requests dropped because a single
    /// `primary_id`'s consumer set hit `MAX_EVENT_CLAIMS_PER_KEY`.
    #[cfg(test)]
    pub(crate) fn event_claim_drops_total_for_test(&self) -> u64 {
        self.event_claim_drops_total
    }

    /// Test-only: `true` when `primary_id` is on the
    /// `event_claim_requested` set (an interest has been registered with
    /// the OneshotApi but not yet released by `complete_unknown_oneshot`).
    #[cfg(test)]
    pub(crate) fn event_claim_is_requested_for_test(&self, primary_id: &str) -> bool {
        self.event_claim_requested.contains(primary_id)
    }

    /// T133 retention-test accessor — total `wire_subs` row count, evicted or
    /// not. The whole point of T133 is that this stabilises rather than
    /// growing with close-cycle count.
    #[cfg(test)]
    pub(crate) fn wire_subs_len_for_test(&self) -> usize {
        self.wire.subs.len()
    }

    /// Bind a per-role signer callback used by the NIP-42 handshake on `role`,
    /// with the active pubkey hex. The actor (or iOS layer) adapts the user's
    /// `nmp_signers::AccountManager::signer_active()` for `Content`/`Indexer`;
    /// other lanes (e.g. NWC `Wallet`) bind their own per-protocol keypair.
    /// Replaces any previously-bound signer for that role.
    ///
    /// Generic per-role NIP-42 primitive (D0). `pub` so NIP-crate runtimes
    /// (`nmp-nip47` post-V-38) can register their per-lane signer.
    pub fn set_relay_auth_signer(
        &mut self,
        role: RelayRole,
        pubkey_hex: String,
        signer: AuthSignerFn,
    ) {
        self.auth_signers
            .insert(role, RelayAuthCredentials { signer, pubkey_hex });
    }

    /// Drop the signer for `role`. Challenges from that role are then recorded
    /// but never answered until a signer is rebound.
    ///
    /// Generic per-role NIP-42 primitive (D0). `pub` so NIP-crate runtimes
    /// (`nmp-nip47` post-V-38) running on the actor thread can clear the
    /// wallet-lane signer on disconnect.
    pub fn clear_relay_auth_signer(&mut self, role: RelayRole) {
        self.auth_signers.remove(&role);
    }

    /// Compat wrapper: bind the same identity signer to every user-identity
    /// relay role (Content + Indexer). Replaces any previously-bound identity
    /// signer on those roles; other roles (e.g. NWC `Wallet`) are unaffected.
    /// FFI bridge that surfaces this from Swift is T59
    /// (filed in `docs/perf/pending-user-decisions.md`).
    pub(crate) fn bind_auth_signer(&mut self, pubkey_hex: String, signer: AuthSignerFn) {
        self.auth_signers.insert(
            RelayRole::Content,
            RelayAuthCredentials {
                signer: Arc::clone(&signer),
                pubkey_hex: pubkey_hex.clone(),
            },
        );
        self.auth_signers.insert(
            RelayRole::Indexer,
            RelayAuthCredentials { signer, pubkey_hex },
        );
    }

    /// Compat wrapper: drop the identity signer for the user-identity roles
    /// (Content + Indexer). Other roles (e.g. NWC `Wallet`) are unaffected —
    /// use `clear_relay_auth_signer(role)` for per-role clearing.
    pub(crate) fn clear_auth_signer(&mut self) {
        self.auth_signers.remove(&RelayRole::Content);
        self.auth_signers.remove(&RelayRole::Indexer);
    }

    /// Returns `true` if at least one relay role has an auth signer bound.
    pub(crate) fn has_auth_signer(&self) -> bool {
        !self.auth_signers.is_empty()
    }

    /// Bind the shared relay-edit rows slot so the FFI layer can read
    /// relay-edit rows without reaching into kernel internals.
    ///
    /// The slot is a typed [`RelayEditRowsSlot`] (`Arc<Mutex<RelayEditRowList>>`).
    pub(crate) fn set_relay_edit_rows_handle(&mut self, handle: RelayEditRowsSlot) {
        self.relay_edit_rows_handle = Some(handle);
    }

    /// Extract the relay-edit rows handle before a `Reset` replaces the
    /// kernel. The underlying `Arc` is process-lifetime and must survive
    /// across kernel reinstantiation.
    pub(crate) fn take_relay_edit_rows_handle_for_reset(&mut self) -> Option<RelayEditRowsSlot> {
        self.relay_edit_rows_handle.take()
    }

    /// Test-only seam — clear the kernel's `relay_edit_rows` so the empty
    /// bootstrap state can be exercised end-to-end.
    ///
    /// `bootstrap_urls_for_role` has a `#[cfg(test)]` fallback that seeds a
    /// default Content/Indexer relay when `relay_edit_rows` is empty (see
    /// `kernel/mod.rs::bootstrap_urls_for_role`'s `#[cfg(test)] if urls.is_empty()`
    /// block). That fallback exists so the vast majority of unit tests don't
    /// need to hand-roll a relay seed for every fresh kernel. The D10
    /// defensive-guard test wants the OPPOSITE — a kernel whose
    /// `relay_edit_rows` is empty AND whose `bootstrap_urls_for_role`
    /// returns empty, so the dispatch path that lands a kind:1059 envelope
    /// in `publish_signed_event` with `relays: vec![]` cannot accidentally
    /// pass the guard via the cfg(test) backstop.
    ///
    /// `pub(crate)` is sufficient — no FFI / cross-crate caller; the
    /// `commands` tests reach it through the kernel's internal API.
    #[cfg(test)]
    pub(crate) fn clear_relay_edit_rows_for_test(&mut self) {
        self.relay_edit_rows.clear();
        if let Some(handle) = self.relay_edit_rows_handle.as_ref() {
            if let Ok(mut guard) = handle.lock() {
                guard.replace(Vec::new());
            }
        }
    }

    /// Register a subscription id as persistent — EOSE will not auto-CLOSE it.
    /// Used by long-lived protocol lanes (NWC kind:23195 listener) where the
    /// subscription must remain open for the connection lifetime. Inverse of
    /// [`unregister_persistent_sub`]. Idempotent.
    ///
    /// T-relay-url-normalize: the `relay_url` is canonicalized before it is
    /// used as the set key. The persistent-sub registry must agree with the
    /// EOSE handler's lookup, which keys on the canonical delivering URL. NWC
    /// wallet callers register with the raw `NwcUri` relay (which does NOT
    /// canonicalize); without this, a non-canonical NWC relay URL would never
    /// satisfy `is_persistent_sub` and the kind:23195 listener would be
    /// wrongly auto-CLOSE'd on its first EOSE. Canonicalizing inside the
    /// primitive makes every caller correct without each having to remember.
    pub fn register_persistent_sub(
        &mut self,
        relay_url: impl Into<String>,
        sub_id: impl Into<String>,
    ) {
        let relay_url = relay_url.into();
        let key = CanonicalRelayUrl::parse_or_raw(&relay_url);
        self.wire.persistent.insert((key, sub_id.into()));
    }

    /// Remove `(relay_url, sub_id)` from the persistent set. Called when the
    /// protocol lane (e.g. wallet disconnect) or the planner withdraws its
    /// subscription on that relay. Idempotent. #170: relay-scoped so closing
    /// the sub on one relay never un-pins a sibling relay still carrying it.
    ///
    /// T-relay-url-normalize: canonicalizes `relay_url` so the removal matches
    /// the canonical key written by [`register_persistent_sub`] regardless of
    /// the URL spelling the caller supplies.
    pub fn unregister_persistent_sub(&mut self, relay_url: &str, sub_id: &str) {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire.persistent.remove(&(key, sub_id.to_string()));
    }

    /// True when `(relay_url, sub_id)` is registered as persistent — EOSE
    /// handlers consult this to skip the default auto-CLOSE policy.
    ///
    /// T-relay-url-normalize: canonicalizes `relay_url` so the lookup matches
    /// the canonical key written by [`register_persistent_sub`].
    pub(crate) fn is_persistent_sub(&self, relay_url: &str, sub_id: &str) -> bool {
        let key = CanonicalRelayUrl::parse_or_raw(relay_url);
        self.wire.persistent.contains(&(key, sub_id.to_string()))
    }

    /// Single-writer insert into `self.wire.subs` (PD-033-C Stage 0).
    ///
    /// Every row written to the wire-sub bookkeeping map MUST flow through
    /// this helper. There are two callers today (`Kernel::req_for_relay` and
    /// `Kernel::register_planner_wire_frames` — the M1/M2 dual writers named
    /// in `docs/architecture-audit/pd033c-plan.md` §1.2); stages 1–6 of the
    /// migration retire M1, leaving `register_planner_wire_frames` as the
    /// sole caller. Funneling both callers through one body up-front turns
    /// "two writers" into "two callers of one writer" so the rest of the
    /// migration is a mechanical grep — see PD-033-C §5 Stage 0.
    ///
    /// `initial_state` is supplied by the caller so the helper preserves the
    /// pre-existing per-caller invariants without growing branches: M1 stamps
    /// `"auth_paused"` when `relay_auth_paused(role)` is true at REQ-emission
    /// time (see PD-033-C §4.1 — a latent gap M2 does not yet honor); M2
    /// stamps `"opening"`. Resolving that asymmetry is Stage 6 territory,
    /// **not** Stage 0 — this helper is a pure behavior-preserving extraction.
    ///
    /// T-relay-url-normalize: `relay_url` is the already-canonical key half
    /// (matches the `(CanonicalRelayUrl, String)` `wire.subs` key type) — the
    /// helper does NOT canonicalize again; that is the caller's contract so
    /// the same canonical value reaches both the map key and the stored
    /// `WireSub.relay_url` field without a redundant parse.
    pub(crate) fn insert_wire_sub(
        &mut self,
        role: RelayRole,
        relay_url: CanonicalRelayUrl,
        sub_id: String,
        filter_summary: String,
        initial_state: &str,
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
            },
        );
        self.changed_since_emit = true;
    }

    pub(crate) fn start(&mut self) {
        if self.timing.started_at.is_none() {
            self.timing.started_at = Some(Instant::now());
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

    /// Force the next due tick to emit a snapshot, even though no kernel field
    /// changed.
    ///
    /// The actor's regular tick only emits when `changed_since_emit()` is true
    /// (see `tick::flush_due`). State that lives OUTSIDE the kernel — notably
    /// the NIP-47 wallet status, an app noun surfaced through the `"wallet"`
    /// snapshot projection (D0) — has no kernel field to flip the flag. The
    /// wallet runtime calls this after writing its shared status slot so a
    /// kind:23195 balance response (which the kernel itself drops as an
    /// unknown kind) still drives a timely projection refresh.
    ///
    /// D0: callers are off-kernel app-noun projections that write their state
    /// to a shared slot instead of a typed `KernelSnapshot` field — the
    /// wallet runtime (`projections["wallet"]`, `feature = "wallet"`) and the
    /// identity runtime's NIP-46 bunker handshake
    /// (`projections["bunker_handshake"]`). A slot write does not flip
    /// `changed_since_emit` on its own, so each calls this to drive a timely
    /// projection refresh on the next due tick.
    pub fn mark_changed_since_emit(&mut self) {
        self.changed_since_emit = true;
    }

    /// Mutable access to the subscription lifecycle (registry + trigger inbox).
    ///
    /// The actor-side `KernelAction` reducer (T95) uses this to register the
    /// `LogicalInterest` resolved from an `OpenUri` action through the
    /// single-writer [`crate::subs::InterestRegistry`] (D4). Kept crate-private
    /// so the FFI surface never sees a subscription-internal type (D0/D6).
    pub(crate) fn lifecycle_mut(&mut self) -> &mut SubscriptionLifecycle {
        &mut self.lifecycle
    }

    /// Pre-populate `seed_contacts` for a given pubkey with the specified follows.
    /// Used during account creation so the follow-feed can be set up immediately
    /// without waiting for the kind:3 event to round-trip from relays.
    pub(crate) fn prepopulate_seed_contacts(&mut self, pubkey: String, follows: Vec<String>) {
        self.seed_contacts.insert(pubkey, follows);
    }

    /// Pre-populate the local NIP-65 mailbox cache from an event this kernel just
    /// signed, so account-scoped interests can route before the relay echo
    /// arrives.
    pub(crate) fn prepopulate_author_relay_list(
        &mut self,
        pubkey: String,
        event_id: String,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) {
        let parsed = parse_relay_list_to_substrate(&event_id, created_at, &tags);
        let empty = parsed.read.is_empty() && parsed.write.is_empty() && parsed.both.is_empty();
        if empty {
            self.mailbox_cache.remove(&pubkey);
        } else {
            self.mailbox_cache.upsert(pubkey.clone(), parsed);
        }
        self.lifecycle
            .enqueue_trigger(CompileTrigger::Nip65Arrived { pubkey, created_at });
    }

    /// Read-only access to the substrate NIP-65 [`MailboxCache`] the
    /// kernel routes through. The kind:10002 ingest path is the single
    /// writer; this getter is for kernel-internal helpers (status,
    /// outbox, planner adapter) and for tests that need to assert
    /// cache state without using the private field.
    pub(crate) fn mailbox_cache(&self) -> &dyn MailboxCache {
        &*self.mailbox_cache
    }

    /// Test-only seed helper — push a NIP-65 cache entry without going
    /// through the kind:10002 ingest path. Replaces the pre-step-3
    /// `kernel.author_relay_lists.insert(...)` pattern dozens of tests
    /// used. Production code MUST NOT call this — the
    /// `ingest::relay_list::ingest_relay_list` path is the single writer
    /// in production (it also fans the `Nip65Arrived` recompile trigger
    /// the M2 planner consumes; this helper does not, by design — tests
    /// that need the trigger should ingest a real kind:10002 event).
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

    /// Shared handle to the substrate [`MailboxCache`]. Used by the
    /// planner-side adapter (`KernelMailboxes`) so the planner reads
    /// the same NIP-65 entries the router does. Test-only because the
    /// in-tree consumer (`drain_lifecycle_tick`) clones the field
    /// directly to satisfy the borrow checker; external tests want a
    /// stable accessor.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mailbox_cache_arc(&self) -> Arc<dyn MailboxCache> {
        Arc::clone(&self.mailbox_cache)
    }

    /// Read-only access to the injected [`OutboxRouter`].
    #[allow(dead_code)] // Reserved for follow-on wiring of actual routing call sites.
    pub(crate) fn outbox_router(&self) -> &dyn OutboxRouter {
        &*self.outbox_router
    }

    /// Inject the DM-inbox relay lookup (V-40 composition seam). Production
    /// composition (apps that depend on `nmp-nip17`) calls this after
    /// `Kernel::new` to install the shared `Arc<DmRelayCache>` so the
    /// kernel's `recipient_dm_relays` reader + the planner-side
    /// `KernelMailboxes` adapter both see the same kind:10050 entries the
    /// kind:10050 ingest parser writes. Default is
    /// [`crate::substrate::EmptyDmInboxRelayLookup`] (every lookup returns
    /// `None`, the fail-closed cold-start contract).
    ///
    /// MUST be called BEFORE the first kind:10050 event is ingested — the
    /// caches are independent stores, not a write-through pair, so a swap
    /// after ingest would lose cached entries.
    pub(crate) fn set_dm_inbox_relay_lookup(&mut self, lookup: Arc<dyn DmInboxRelayLookup>) {
        self.dm_inbox_relays = lookup;
    }

    /// Inject the blocked-relay lookup (composition seam). Production
    /// composition (apps that depend on `nmp-router`) calls this after
    /// `Kernel::new` to install a shared `Arc<InMemoryBlockedRelayCache>`
    /// so the kernel's `build_routing_context` reader and the
    /// kind:10006 ingest parser writer see the same cache. Default is
    /// [`crate::substrate::EmptyBlockedRelayLookup`] (every lookup returns
    /// the empty set — the pre-V-40 zero-block default).
    ///
    /// MUST be called BEFORE the first kind:10006 event is ingested — the
    /// caches are independent stores, not a write-through pair, so a swap
    /// after ingest would lose cached entries.
    pub(crate) fn set_blocked_relay_lookup(&mut self, lookup: Arc<dyn BlockedRelayLookup>) {
        self.blocked_relays = lookup;
    }

    /// Shared handle to the injected `Arc<dyn BlockedRelayLookup>` — used by
    /// `kernel/mailboxes.rs::build_routing_context` to snapshot a
    /// [`crate::substrate::BlockedRelaySet`] per call.
    pub(crate) fn blocked_relays_arc(&self) -> Arc<dyn BlockedRelayLookup> {
        Arc::clone(&self.blocked_relays)
    }

    /// Override the active-account bootstrap Tailing self-kinds list
    /// (`startup::SELF_KINDS_TAILING`). `None` (the default) uses the
    /// built-in list.
    ///
    /// MUST be called BEFORE the first `active_account_bootstrap_requests`
    /// call so the override takes effect on cold-start / sign-in. The
    /// FFI's `bootstrap_self_kinds` pre-start slot wires through this
    /// setter at actor start.
    pub(crate) fn set_bootstrap_self_kinds_override(&mut self, kinds: Option<Vec<u32>>) {
        self.bootstrap_self_kinds_override = kinds;
    }

    /// Read-only accessor for the bootstrap self-kinds override slot. The
    /// `startup.rs` module reads through this rather than the bare field
    /// so the override resolution policy (None → use builtin) stays
    /// localised to a single call site.
    pub(crate) fn bootstrap_self_kinds_override(&self) -> Option<&[u32]> {
        self.bootstrap_self_kinds_override.as_deref()
    }

    /// Replace the kernel's [`EventIngestDispatcher`] slot with `slot`.
    /// Composition-time wiring path — the actor calls this with the
    /// `Arc<RwLock<EventIngestDispatcher>>` slot owned by `NmpApp` so
    /// `NmpApp::register_ingest_parser` and the kernel share one
    /// dispatcher.
    ///
    /// MUST be called BEFORE the first event is ingested.
    pub(crate) fn set_ingest_dispatcher_slot(
        &mut self,
        slot: Arc<std::sync::RwLock<EventIngestDispatcher>>,
    ) {
        self.ingest_dispatcher = slot;
    }

    /// Shared handle to the injected `Arc<dyn DmInboxRelayLookup>`. Used by
    /// the planner-side `KernelMailboxes` adapter so the planner reads the
    /// same DM-inbox relay entries the gift-wrap publish path reads.
    pub(crate) fn dm_inbox_relays_arc(&self) -> Arc<dyn DmInboxRelayLookup> {
        Arc::clone(&self.dm_inbox_relays)
    }

    /// Register a [`crate::substrate::IngestParser`] for `kind` against the
    /// kernel's shared [`EventIngestDispatcher`] slot. Composition-time
    /// wiring path — `NmpApp::register_ingest_parser` calls this through
    /// a kernel handle shared with the actor; the slot pattern matches
    /// the rest of the substrate's host-extension seams.
    ///
    /// D6 — a poisoned dispatcher lock degrades to a no-op (the
    /// registration is dropped; the kernel keeps its current set).
    /// MUST be called before the first event is ingested.
    #[allow(dead_code)] // Wired through `NmpApp` at composition time.
    pub(crate) fn register_ingest_parser(
        &self,
        kind: u32,
        parser: Arc<dyn crate::substrate::IngestParser>,
    ) {
        if let Ok(mut d) = self.ingest_dispatcher.write() {
            d.register_kind(kind, parser);
        }
    }

    /// Shared handle to the kernel's [`EventIngestDispatcher`] slot. Used
    /// by the actor / kernel ingest path to dispatch a verified event to
    /// every registered parser; used by the FFI composition seam to
    /// install fresh parsers.
    pub(crate) fn ingest_dispatcher_slot(&self) -> Arc<std::sync::RwLock<EventIngestDispatcher>> {
        Arc::clone(&self.ingest_dispatcher)
    }
}

/// Adapter — translate the kernel's existing `parse_relay_list`
/// (which returns the legacy `AuthorRelayList` with `event_id` +
/// `created_at` supersession metadata) into the substrate
/// [`ParsedRelayList`] the [`MailboxCache`] trait operates on.
///
/// The supersession metadata is dropped here — the store enforces
/// kind:10002 supersession before `ingest_relay_list` is called
/// (see the doc comment on `ingest::relay_list::ingest_relay_list`).
/// The pre-step-3 kernel kept a "belt-and-suspenders" mirror of
/// the store's logic on the kernel-side cache; step 3 collapses to a
/// single source of truth (the store) per the planning-discipline rule
/// (`AGENTS.md`: "single source of truth per fact").
fn parse_relay_list_to_substrate(
    event_id: &str,
    created_at: u64,
    tags: &[Vec<String>],
) -> ParsedRelayList {
    // Reuse the existing parser, then translate fields.
    let legacy = parse_relay_list(event_id, created_at, tags);
    ParsedRelayList {
        read: legacy.read_relays,
        write: legacy.write_relays,
        both: legacy.both_relays,
    }
}
