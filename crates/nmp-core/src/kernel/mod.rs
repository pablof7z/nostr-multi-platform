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
// ADR-0049 Part 2 — the composition ledger (explain-the-composition surface):
// an append-only record of host-init registration decisions, read back as JSON
// through `nmp_app_composition_report`. Written only at registration time, not
// on any hot path (D8).
pub mod composition_ledger;
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
#[cfg(test)]
mod signed_events_return_tests;
// V-59 rung 1 — public typed accessor over the active account's
// `timeline_authors` projection (raw pubkeys). The OP-centric feed's
// `FollowSetLookup` capability (later rung) reads through this seam.
mod active_timeline_authors;
#[cfg(test)]
mod active_timeline_authors_tests;
mod auth;
// V-06 / #960 — NIP-42 AUTH signer-binding state (local-vs-remote disjoint
// bindings) + the `PendingAuthSign` async-AUTH queue, extracted from this file to
// keep it within its size budget. Adds methods to `impl Kernel`.
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
// ADR-0045 E1 — store-cache serve seam. The first half of the one event-
// acquisition mechanism: at interest-open time, query the store for the
// newest-N events matching the interest's shape and feed them through the
// post-store projection-dispatch path (not store.insert — see ADR §1.2).
mod cache_serve;
#[cfg(test)]
mod cache_serve_all_kinds_dispatcher_tests;
#[cfg(test)]
mod cache_serve_budget_tests;
#[cfg(test)]
mod cache_serve_tests;
#[cfg(test)]
mod cache_serve_truncation_tests;
#[cfg(test)]
mod cache_serve_universal_tests;
#[cfg(test)]
mod cache_serve_watermark_tests;
pub(crate) mod closed_reason;
// K3 Stage D1 (ADR-0056 §3) — coverage-ledger write path.
mod coverage_ledger;
mod diagnostic_counters;
mod discovery;
/// ADR-0052 §D5 — `&mut Kernel` → narrow wallet/zap capability adapter.
pub mod wallet_access;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod coverage_ledger_d1_tests;
#[cfg(test)]
mod eose_ok_notice_ingest_tests;
#[cfg(test)]
mod event_claim_tests;
#[cfg(test)]
mod interest_install_cache_serve_support;
#[cfg(test)]
mod interest_install_cache_serve_tests;
#[cfg(test)]
mod resolved_profiles_tests;
// V-59 rung 1 (#4) — `event_claim_released` ring projection + the
// in-process `EventClaimReleasedObserver` registration. `pub(crate)` so the
// trait is reachable for the struct field type in this module.
pub(crate) mod event_claim_released;
#[cfg(test)]
mod event_claim_released_tests;
mod event_observer;
#[cfg(test)]
mod event_observer_tests;
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
mod local_publish_intent;
#[cfg(test)]
mod local_publish_intent_tests;
mod mailboxes;
#[cfg(any(test, feature = "test-support"))]
mod negentropy_test_support;
mod negentropy_types;
mod nostr;
#[cfg(test)]
mod outbox_tests;
#[cfg(test)]
mod pre_kind3_buffer_tests;
#[cfg(test)]
mod proactive_profile_fetch_tests;
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
// `Arc<Mutex<Vec<String>>>` / `Arc<Mutex<Vec<AppRelay>>>` slots from the
// publish resolver and `NmpApp` move behind named types here so D14 can flag
// future regressions on the field shape.
mod relay_frame;
mod relay_projection;
// Substrate-pure RelayAuthorScore type + per-author/relay scoring map.
// Consumed by score-update seams and the planner warm-relay preference.
// LMDB hydration/flush keeps the map restart-stable.
pub mod relay_score;
#[cfg(test)]
mod relay_score_tests;
// F-TTL — replaceable event freshness policy. Configures per-kind TTLs for
// how long replaceable identities (kind, pubkey, d_tag?) remain "fresh" before
// a re-verification REQ is needed. The LMDB sub-db `replaceable_freshness`
// stores `check_again_after` timestamps; this config determines the delta.
pub mod replaceable_ttl;
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
#[cfg(test)]
mod replaceable_ttl_gate_tests;
mod replay;
#[cfg(test)]
mod replay_tests;
mod requests;
#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod watermark_author_tests;
// Host-extensible snapshot output — the `nmp_app_register_snapshot_projection`
// seam. `pub(crate)` so the crate-private `ffi` module can reach the registry
// + slot helpers for the C-ABI registration entry point.
#[cfg(test)]
mod d1_offline_bootstrap_tests;
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
/// Tier-2 (kernel-owned built-in) typed-projection codecs + `make_update`
/// wiring. The Wave C counterpart to the host-registered Tier-1 typed
/// projections (ADR-0037). See the module doc for the mechanism rationale.
mod typed_projections;
/// ADR-0055 Rung 1 — kernel-owned per-projection revision manifest.
/// Source-version counters + `ProjectionRevTracker` owned by `Kernel`.
/// Zero wire change in Rung 1 — pure infrastructure for Rung 2/3.
pub(crate) mod projection_rev;
#[cfg(test)]
mod typed_projections_tests;
#[cfg(test)]
mod typed_projections_wave_c_diagnostics_tests;
#[cfg(test)]
mod typed_projections_wave_c_tests;
mod types;
mod update;
// `WireSub` row (moved out of `types.rs` for the LOC cap).
mod wire_sub;
pub use update::KERNEL_BUILTIN_PROJECTION_KEYS;
#[cfg(any(test, feature = "test-support"))]
pub use update::{PROCESS_PROJECTIONS_CHANGED, PROCESS_PROJECTIONS_SERIALIZED};
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
mod contacts_fanout_tests;

use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole, DEFAULT_EMIT_HZ};
// `chrono::Local` reads the OS-local wall clock; the `clock` feature it lives
// behind is gated to `native` in Cargo.toml. The wall-clock display helper
// `now_hms` in `kernel/nostr.rs` is native-only — see the `#[cfg(feature =
// "native")]` gate and the single use site in `kernel/status.rs`.
#[cfg(feature = "native")]
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::Arc;
// W1 — use the wasm-safe time shim (`crate::time`) rather than `std::time`
// directly. On wasm32 `Instant` → `performance.now()`, `SystemTime` →
// `Date.now()`; on native the shim re-exports `std::time` verbatim so native
// behaviour is unchanged. Submodules in `kernel/` access these via `super::`.
use crate::time::{Duration, Instant, UNIX_EPOCH};
// `SystemTime` is consumed by native-gated `now_hms` in `kernel/nostr.rs`
// and by `Clock::now()` (always-compiled). Use the shim on both targets.
use crate::time::SystemTime;
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

/// Public decode surface for the typed-projection sidecar (re-exported at the
/// crate root as `nmp_core::typed_projections`). The per-key decoders + their
/// typed DTOs let out-of-tree Rust consumers read typed projections instead of
/// string-keying the generic JSON `payload`. See the `typed_projections` module
/// doc for the return-type / scope rationale.
pub mod public_typed_projections;

use nostr::{parse_profile, parse_relay_list, ratio, short_hex, truncate, NostrEvent};
// V-01 Phase 1c follow-up: `now_hms` is `#[cfg(feature = "native")]` in
// `kernel/nostr.rs` (reads the OS wall clock via `chrono::Local`). Importing
// it unconditionally breaks `--no-default-features` (wasm32) builds. The
// single call site in `status.rs` is already `#[cfg(feature = "native")]`,
// so the re-export is gated too.
// `format_timestamp` was deleted by ADR-0032 / V-115: publish_outbox now
// emits raw `created_at` (Unix seconds); shells format timestamps locally.
#[cfg(feature = "native")]
use nostr::now_hms;
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
// V-06 / #960 — surfaced at the kernel-module root so the `Kernel` field type and
// the `ingest::auth_handlers` enqueue site (`super::super::PendingAuthSign`)
// resolve exactly as they did when the struct lived in this file.
pub use auth_sign_state::PendingAuthSign;
use clock::SystemClock;
// Re-export `Clock` at `crate::kernel::Clock` so the always-compiled
// `crate::slots::KernelClockSlot` (`Arc<Mutex<Option<Arc<dyn Clock>>>>`) can
// name the trait across crates, and so this module's own `use` of it stays
// valid. Only the trait NAME is public; the swap-in setter (`set_clock`) stays
// `pub(crate)`, so downstream crates cannot replace the kernel clock except
// through the test-support `NmpApp::set_kernel_clock_for_test` seam.
pub use clock::Clock;
// Test-support: the advanceable clock external e2e tests install through the
// FFI `set_kernel_clock_for_test` seam.
#[cfg(any(test, feature = "test-support"))]
pub use clock::MonotonicSecondClock;
// M6 — action-dispatch runtime, reachable from the `ffi` module for the
// `nmp_app_dispatch_action` entry point. V-01 Phase 1c: native FFI only.
// `default_registry` / `ActionRegistry` are reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::*` (the FFI surface owns the
// `nmp_app_dispatch_action` entry point).
#[cfg(feature = "native")]
pub use action_registry::{default_registry, ActionRegistry};
pub use composition_ledger::{
    CompositionLedger, CompositionRecord, Disposition, COMPOSITION_REPORT_SCHEMA_VERSION,
};
pub(crate) use identity_state::{AccountSummary, PublishQueueEntry, RelayAckOutcome};
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
pub use identity_state::{read_eligible_relay_urls, AppRelay};
#[cfg(feature = "codegen-schema")]
pub(crate) use types::TimelineItem as TimelineItemForCodegen;
#[cfg(feature = "codegen-schema")]
pub(crate) use types::WireSubscriptionStatus as WireSubscriptionStatusForCodegen;
// Host-extensible snapshot output — reachable from the `ffi` module for the
// `nmp_app_register_snapshot_projection` C-ABI entry point.
// `SnapshotProjectionSlot` is a Kernel struct field type (always-compiled);
// `new_snapshot_projection_slot` is called from `KernelReducer::new` on all
// targets (PR-4) and from `nmp_app_new` on native.
// `SnapshotProjectionSlot` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::SnapshotProjectionSlot` (the NmpApp struct
// field type).
pub use snapshot_registry::new_snapshot_projection_slot;
pub use snapshot_registry::SnapshotProjectionSlot;
// `ChangeGate`: the opt-in per-projection change-gate trait. A host
// (per-app crate) names this to pass its rev `Arc<AtomicU64>` as the gate to
// the gated registration seam (`register_snapshot_projection_gated`), so a
// projection whose inputs did not change is served from cache instead of being
// re-run (and re-serialized) on every emit.
pub use snapshot_registry::ChangeGate;
// Typed slot wrappers + constructors. `AppRelaySlot` /
// `AppRelayList` are re-exported below at `pub use` because per-app
// crates (e.g. `nmp-app-chirp`) consume the slot via
// `NmpApp::configured_relays_handle()` and iterate via `guard.as_slice()`;
// without the public re-export Chirp could not name the returned slot type.
// `RelayUrls` and the URL-slot aliases stay kernel-internal: no external
// caller names them directly (the resolver constructs slots via the
// `new_*_slot()` helpers and reads through `as_slice()`).
pub use relay_projection::{AppRelayList, AppRelaySlot};
// Re-exported `pub` (widened from `pub(crate)`) so `crate::slots` can
// surface them — `nmp-router::Nip65OutboxResolver` (spec §271) constructs
// resolver slots with handles shared by the kernel actor's reducer. Direct
// in-crate consumers continue to import through `crate::kernel::{...}`.
pub use relay_projection::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
// `new_app_relay_slot` is reached by `nmp-ffi` through
// `nmp_core::__ffi_internal::new_app_relay_slot` (called once from
// `nmp_app_new` to construct the slot the actor and the per-app crate
// share).
#[cfg(feature = "native")]
pub use relay_projection::new_app_relay_slot;
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
    empty_blocked_relay_lookup, empty_dm_inbox_relay_lookup, BlockedRelayLookup, BoundedMessageMap,
    DmInboxRelayLookup, EmptyOutboxRouter, EventIngestDispatcher, MailboxCache, OutboxRouter,
    ParsedRelayList, MAX_PROJECTION_MESSAGES,
};
use crate::util::sort_dedup;
use relay_transport::RelayTransportMap;
use std::sync::atomic::{AtomicU64, Ordering};
// ADR-0044 — re-exported crate-wide (not just `use`d into `kernel`) so the
// transport layer (`crate::update_envelope::encode_snapshot_with_envelope`) can
// name `&KernelSnapshot` to populate the typed Tier-3 `SnapshotFrame` fields.
pub(crate) use types::KernelSnapshot;
use types::{
    ClaimedEventDto, Counters, DiagnosticFirehoseState, LogicalInterestStatus,
    MentionProfilePayload, Metrics, OutboxSummarySnapshot, Profile, ProfileCard,
    ProfileRequestState, PublishOutboxItem, PublishOutboxRelay, RelayHealth, RelayStatus,
    StoredEvent, TimelineItem, TimingMilestones, WireSub, WireSubscriptionState,
    WireSubscriptionStatus,
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

/// F-TTL — inflight REQ guard duration (unix milliseconds). When a replaceable
/// event's re-verification REQ is dispatched, the kernel sets its
/// `check_again_after` to `now + INFLIGHT_GUARD_MS` so rapid repeated claims
/// for the same identity don't hammer relays with concurrent REQs. On EOSE
/// or event insertion, the kernel updates it to `now + ttl_for_kind(kind)`.
pub(crate) const INFLIGHT_GUARD_MS: u64 = 3_600_000; // 1 hour

/// Per-relay-role NIP-42 credentials. The closure signs the kind:22242 with
/// whatever keypair is appropriate for that role (user identity for Content /
/// Indexer; NWC client secret for Wallet). `pubkey_hex` is stamped on the
/// unsigned template's `pubkey` field — NIP-42 requires the AUTH event to be
/// signed by the connecting client's key.
pub(crate) struct RelayAuthCredentials {
    pub(crate) signer: AuthSignerFn,
    pub(crate) pubkey_hex: String,
}

/// V-58 — kernel-side backoff hint for a relay URL. The kernel populates
/// `Kernel::pending_backoff_hints` when it classifies a NIP-01 CLOSED reason
/// that warrants a long reconnect delay; the actor drains the queue and
/// forwards each hint to the pool worker via `Pool::set_backoff_hint`.
///
/// The enum lives in `nmp-core` (not `nmp-network`) because the kernel
/// owns the CLOSED-reason classification. The actor maps it to
/// [`nmp_network::pool::BackoffClass`] before calling the pool.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackoffHint {
    /// Relay issued `CLOSED ["rate-limited: …"]` — use
    /// `RELAY_RECONNECT_DELAY_RATE_LIMITED` on the next reconnect.
    RateLimited,
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
    /// ADR-0055 Rung 1 — per-projection revision tracker (typed `SourceVersions`
    /// counters + dependency-derived per-key monotonic revs). Internal-only in
    /// Rung 1: `make_update` does NOT consult it (wire bytes unchanged). Reset to
    /// 0 on `Kernel` rebuild (fresh `Default`).
    pub(crate) projection_rev_tracker: projection_rev::ProjectionRevTracker,
    /// ADR-0055 Rung 1 (F3) — biconditional completeness oracle state, carried
    /// across ticks. `cfg(any(test, test-support))` ONLY: a production build
    /// neither holds this field nor runs the oracle (ZERO emit-path cost).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) projection_oracle: projection_rev::oracle::OracleState,
    /// FFI diagnostic timing milestones (D0 app-domain state). See
    /// [`TimingMilestones`].
    timing: TimingMilestones,
    relays: HashMap<RelayRole, RelayHealth>,
    transport_relays: RelayTransportMap,
    profiles: HashMap<String, Profile>,
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
    /// Cached `estimated_store_bytes` value. Memoized on first call after
    /// cache invalidation (set to `None`) at every store-mutation site. The
    /// cache is correct (bit-identical to a fresh full-scan) because
    /// invalidation happens after EVERY insert (events, profiles, seed_contacts).
    /// Cell<Option<usize>> allows `estimated_store_bytes()` to take `&self` and
    /// memoize without &mut. See `status.rs` for the getter logic.
    cached_estimated_store_bytes: std::cell::Cell<Option<usize>>,
    timeline: VecDeque<String>,
    // V-68 / V-112 (ADR-0042): author_view (AuthorViewState) / thread_view
    // (ThreadViewState) fields deleted. View state lives in per-app FlatFeed.
    /// Diagnostic firehose tracking (D0 app-domain state). See
    /// [`DiagnosticFirehoseState`].
    diagnostic_firehose: DiagnosticFirehoseState,
    deferred_outbound: VecDeque<OutboundMessage>,
    /// V-58 — pending one-shot backoff hints the actor drains after each
    /// `handle_message` call and forwards to the pool worker via
    /// `Pool::set_backoff_hint`. Each entry is `(relay_url, class)`.
    ///
    /// `relay_url` is the raw delivering URL the CLOSED frame arrived on
    /// (same key the pool uses to look up the worker slot). The kernel
    /// populates this only for `CloseReason::RateLimited`; the actor is
    /// responsible for mapping it to the correct pool handle via the
    /// `relay_controls` map (the same way every other per-URL dispatch
    /// works). Using the URL avoids a new handle-lookup API on Kernel and
    /// keeps the field substrate-generic (no `RelayHandle` dependency).
    pending_backoff_hints: Vec<(String, BackoffHint)>,
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
    /// `pub(crate)` so in-crate tests can assert close-contact-feed clears
    /// the follow author set without triggering the full follow-feed
    /// registration side-effect that `set_follow_feed_kinds` fires.
    pub(crate) timeline_authors: BTreeSet<String>,
    /// V-59 rung 1 (Q7) — pre-kind:3 ingest buffer. Holds host-declared
    /// follow-feed events that arrived BEFORE the active account's follow set
    /// named their author — i.e. `should_store_event` returned `false` solely
    /// because `!timeline_authors.contains(author)`. Instead of dropping such
    /// an event (which is the historical behavior), `ingest_timeline_event`
    /// parks it here keyed by event id.
    ///
    /// `sync_follow_feed_interests` walks the buffer after rebuilding
    /// `timeline_authors`: any entry whose author is now followed is re-fed
    /// through `ingest_timeline_event` (and thus stored); the rest are dropped.
    /// Cleared on identity change so a switched-out account's parked events
    /// never leak into the new account's stream.
    ///
    /// Bounded by [`MAX_PROJECTION_MESSAGES`] (D5): a burst of events for
    /// authors that never become followed evicts oldest-first rather than
    /// growing without bound. No consumer reads this buffer outside the kernel
    /// ingest path — it is purely an internal staging area.
    ///
    /// The value pairs the parked `NostrEvent` with the delivering relay URL
    /// (its provenance) so the replay through `ingest_timeline_event`
    /// re-records the SAME first-source provenance the event would have had if
    /// the follow set had named its author on first arrival. (The V-59 §5
    /// sketch typed this `BoundedMessageMap<EventId, NostrEvent>`; the tuple
    /// preserves provenance for the replay — the buffer has no external
    /// consumer, so the value shape is an internal detail.)
    pre_kind3_buffer: BoundedMessageMap<String, (NostrEvent, String)>,
    /// T140 — M2 follow-feed interest tracking. Maps each currently-registered
    /// follow-feed `InterestId` so `sync_follow_feed_interests` can withdraw
    /// stale entries before re-registering on kind:3 change. Derived from the
    /// active account's kind:3 follow set; empty until first kind:3 arrives.
    /// `pub(crate)` so in-crate tests can assert the interest registry is
    /// empty after `close_contact_feed` without triggering side-effects.
    pub(crate) follow_feed_interest_ids: BTreeSet<crate::planner::InterestId>,
    /// Host-declared event kinds the contact-feed subscription should REQ for
    /// the active account's follow set. Empty = the subscription is not active
    /// (no follow-feed interests are registered). The host (e.g. Chirp) declares
    /// its app-specific kinds via `ActorCommand::OpenContactFeed { kinds }`;
    /// `nmp-core` no longer hardcodes any kind set here (D0 — the substrate
    /// carries no app-specific social knowledge such as {1, 6}).
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
    /// V-59 rung 1 (#4) — bounded ring of `primary_id`s whose claim resolved
    /// to EOSE-without-match. When `complete_unknown_oneshot` observes the
    /// EOSE for a claim sub whose event never arrived, it clears the
    /// `event_claims` / `event_claim_requested` state for that id and pushes
    /// the id here. Later rungs (the OP-centric feed engine) register an
    /// `EventClaimReleasedObserver` to learn "this claimed root could not be
    /// hydrated" and drop the pending attribution. No consumer in this PR.
    ///
    /// Bounded by [`MAX_PROJECTION_MESSAGES`] (D5). Append-only signal log,
    /// not a keyed projection — see [`crate::substrate::BoundedRing`].
    event_claim_released: crate::substrate::BoundedRing<String>,
    /// V-59 rung 1 (#4) — in-process observers notified on each
    /// `event_claim_released` push. Rust-only for now (no FFI consumer yet);
    /// the C-ABI channel can be added later mirroring
    /// `actor/commands/raw_event_observer.rs` when an FFI consumer appears.
    event_claim_released_observers: Vec<Arc<dyn event_claim_released::EventClaimReleasedObserver>>,
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
    /// K3 Stage D1 (ADR-0056 §3) — off-by-default flag gating the coverage-
    /// ledger WRITE path. When `false` (the default) the kernel records NO
    /// coverage at EOSE / NEG-DONE, so D1 is a pure no-op additive change and
    /// nothing reads the ledger anyway (the since-floor stays presence-derived
    /// until Stage D2). When `true` the ledger fills via
    /// `EventStore::record_coverage`, but READ behaviour is still unchanged in
    /// D1 — the floor is swapped to read the ledger only in Stage D2. The
    /// eventual default-on rides a single release cut (ADR-0056 §3 Stage D) so
    /// git-rev-pinning external consumers can pin across the change.
    coverage_ledger_enabled: bool,
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
    /// V-06 / #960 — per-role AUTH pubkey for a *remote* (NIP-46 / NIP-55)
    /// account. The kernel knows WHOM to AUTH as but holds no synchronous signer
    /// (the broker is the only thing that can sign), so a challenge on such a
    /// lane enqueues a [`PendingAuthSign`] instead of signing inline. A role is
    /// in `auth_signers` XOR `auth_remote_pubkeys` (local-key XOR remote signer),
    /// never both — `bind_auth_signer` / `bind_auth_remote` keep them disjoint.
    auth_remote_pubkeys: HashMap<RelayRole, String>,
    /// V-06 / #960 — AUTH kind:22242 events awaiting a remote signature. The
    /// actor drains this after each inbound frame (`take_pending_auth_signs`),
    /// routes each through the async signer port, and re-enters
    /// `dispatch_signed_auth` on resolution.
    pending_auth_signs: Vec<PendingAuthSign>,
    /// T66a identity/publish projections — flat wire-protocol summaries the
    /// actor pushes after each AccountManager-equivalent mutation. The actor
    /// (in `nmp-core`, so it CANNOT import `nmp-signers` per D0) owns the
    /// authoritative `nostr::Keys` map; these are the derived snapshot cache.
    accounts: Vec<AccountSummary>,
    active_account: Option<String>,
    /// Sign-and-return results parked by the `SignEventForReturn` actor command
    /// (`ParkedOp` `SignedEventsProjection` sink resolution), keyed by
    /// `correlation_id`. Each entry
    /// is `Ok(signed_event_json)` — the standard flat Nostr event JSON, ready
    /// for the host to attach to an out-of-band transport — or `Err(message)`.
    ///
    /// Drain-on-emit, mirroring `action_results`: `make_update` surfaces this
    /// map into `projections["signed_events"]` then `clear()`s it. The host
    /// reads each id exactly once (its `signEventForReturn` continuation
    /// resumes on first appearance), so the kernel never retains them.
    signed_events: HashMap<String, Result<String, String>>,
    publish_queue: Vec<PublishQueueEntry>,
    last_error_toast: Option<String>,
    /// Machine-readable category for `last_error_toast` (typed FFI error
    /// contract). Closed key set lives in `kernel::closed_reason`. Set by
    /// `set_error_toast_with_category`; cleared by the legacy
    /// `set_last_error_toast` so a newer uncategorized toast never leaves a
    /// stale category shadowing it.
    last_error_category: Option<String>,
    configured_relays: Vec<AppRelay>,
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
    /// Per-tick capture of the FIVE drain-on-emit / wall-clock-sensitive
    /// projection values, written ONCE at their JSON-insertion site in
    /// `snapshot_projections_with_publish_cluster` and read by the Tier-2
    /// typed sidecar path (`builtin_typed_projections`) in the SAME tick (Wave
    /// C, ADR-0037). These exist because the producing accessors must not be
    /// invoked twice per tick:
    /// - `action_results` / `signed_events` DRAIN their source (calling twice
    ///   loses data);
    /// - `action_lifecycle` runs a wall-clock TTL sweep (`&mut self`);
    /// - `action_stages` is mutated earlier in the same tick by the
    ///   `action_results` drain (captured for uniformity);
    /// - `relay_diagnostics` pre-formats wall-clock-relative "Xs ago" labels
    ///   against an internal `now` (a second call could straddle a one-second
    ///   bucket and diverge from the JSON form).
    ///
    /// Each is reset every tick: `Some(value)` exactly when the matching JSON
    /// key is inserted, `None` otherwise — so the typed entry is present iff the
    /// JSON entry is, and never carries stale data into the next tick. The four
    /// drain-on-emit ones hold the captured `serde_json::Value` (parsed back
    /// into a typed Model by the codec); `relay_diagnostics` holds the captured
    /// struct (mapped struct->Model, the #1031 convention).
    captured_action_results: Option<serde_json::Value>,
    captured_signed_events: Option<serde_json::Value>,
    captured_action_stages: Option<serde_json::Value>,
    captured_action_lifecycle: Option<serde_json::Value>,
    captured_relay_diagnostics: Option<relay_diagnostics::RelayDiagnosticsSnapshot>,
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
    /// importing kernel internals. Synced by `set_configured_relays` in
    /// `identity_state.rs`.
    ///
    /// Slot type is [`AppRelaySlot`] (`Arc<Mutex<AppRelayList>>`);
    /// D14 forbids bare `Arc<Mutex<Vec<…>>>` fields on `Kernel` and the
    /// typed wrapper makes the slot's purpose visible at the declaration site.
    configured_relays_handle: Option<AppRelaySlot>,
    /// Shared list of indexer relay URLs, kept in sync with `configured_relays`
    /// by `set_configured_relays`. The `Nip65OutboxResolver` holds a clone of
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
    /// F-TTL — replaceable event freshness policy. Configures per-kind TTLs
    /// for how long replaceable identities (kind, pubkey, d_tag?) remain "fresh"
    /// before a re-verification REQ is needed. The LMDB sub-db `replaceable_freshness`
    /// stores `check_again_after` timestamps; this config determines the delta
    /// added to `now` to produce the next check_again_after value. Defaults to
    /// kind:0 = 1h, kind:10002 = 6h, others = 6h. Can be overridden via
    /// `set_replaceable_ttl()`.
    replaceable_ttl: replaceable_ttl::ReplaceableTtlConfig,
    /// F-TTL — pending re-verification queue for replaceable events. When a
    /// replaceable identity's `check_again_after` is due, it is enqueued here
    /// and drained in `pending_view_requests` as REQ filters. Maps to subscription
    /// IDs via `reverify_subs` so the EOSE handler can update `check_again_after`
    /// with fresh TTL when the REQ completes.
    pending_reverify: VecDeque<crate::store::ReplaceableKey>,
    /// F-TTL — in-flight reverification subscriptions. Maps wire sub_id →
    /// Vec<ReplaceableKey> so the EOSE handler knows which keys to update
    /// with fresh TTL on completion. Entries are removed when their sub_id
    /// receives EOSE.
    reverify_subs: HashMap<String, Vec<crate::store::ReplaceableKey>>,
    /// V-67: set when a persistent storage path was supplied but the LMDB
    /// store failed to open. `None` in the healthy case AND when no path was
    /// given (in-memory is the legitimate default for tests/CI — not a
    /// degradation). Surfaced on every snapshot tick via `store_open_failure`
    /// so the host observes the degraded-store state immediately instead of
    /// silently losing all persisted events.
    ///
    /// D6: no stderr writes; diagnostic flows through the normal snapshot
    /// channel. D0: generic name ("store", not an LMDB/NIP noun).
    store_open_failure: Option<String>,
    /// GAP-5: NIP-agnostic negentropy session statistics.  Accumulated by the
    /// NIP-77 runtime and pushed via `set_negentropy_sync_stats` on session
    /// completion.  Zero-default; `last_reconcile_at_ms` stamped from the
    /// kernel's injected clock (D9) so replay/tests stay deterministic.
    negentropy_sync_stats: types::NegentropySyncStats,
    /// #1069 — last bounded GC pass result + the kernel-clock wall-time it ran.
    /// Populated by [`Kernel::run_gc_step`] (the actor's 60-second idle-tick gc
    /// pass). `None` until the first pass runs. Observable so the GC schedule is
    /// not a silent ending — surfaced for diagnostics (`gc.md` §7 `StoreHealth`).
    ///
    /// D4: the actor is the single writer (gc runs only on the actor thread).
    /// D9: `last_gc_at_ms` is read from the injected [`Clock`], not a bare
    /// `SystemTime::now()`, so replay/tests stay deterministic.
    last_gc: Option<crate::store::GcReport>,
    last_gc_at_ms: Option<u64>,
    /// ADR-0045 E1 — completion set for store-cache serve.
    ///
    /// Each entry is a `completion_key` (stable hash of interest scope-key +
    /// shape content). An interest whose key is in this set has already had its
    /// stored events served into projections (one-shot per key) and will not be
    /// re-served on subsequent recompiles (relay reconnect, follow-list change).
    ///
    /// Cleared on account-switch / kernel reset so the next identity's interests
    /// get a fresh serve. Populated when a queued serve **finishes** (see
    /// [`Kernel::run_cache_serve_step`]).
    ///
    /// `HashSet<u64>` — only completion keys; bounded by the number of distinct
    /// interests ever opened in a session (in practice O(visible-views × 10)).
    pub(in crate::kernel) served_interest_shapes: HashSet<u64>,
    /// ADR-0045 §5 — continuation queue for store-cache serves.
    ///
    /// [`Kernel::enqueue_cache_serve`] pushes; [`Kernel::run_cache_serve_step`]
    /// drains under ONE shared per-tick budget, resuming partially-completed
    /// serves (per-query `until` cursor) on subsequent actor ticks. This is
    /// the chunked continuation ADR §5 mandates so a cold start with hundreds
    /// of per-follow interests never bursts unbounded synchronous work on the
    /// actor thread (the #1085 lesson at the aggregate level).
    pub(in crate::kernel) pending_cache_serves: VecDeque<cache_serve::PendingCacheServe>,
    /// K3 Stage B3 / #1380 — WRITE surface of cursor-less (`Etag`/`Ptag`) serves
    /// budget-truncated mid-chunk. Keyed by `PendingCacheServe::completion_key`
    /// (SubKey-aware), NOT `cursor_less_query_key`, so one interest's exhaustion
    /// cannot clear a sibling's mark (#1380 Bug 1). Written by `serve_chunk`, which
    /// then refreshes the read view via [`Kernel::recompute_truncated_query_keys`].
    pub(in crate::kernel) etag_ptag_truncated_serves: Arc<std::sync::Mutex<HashSet<u64>>>,
    /// K3 #1380 — READ view of [`Self::etag_ptag_truncated_serves`] keyed by
    /// `cursor_less_query_key`: holds a query key iff ≥1 active interest mapping to
    /// it is truncated; read by the shape-only `watermark_fn` / `shape_floor`. See
    /// [`Kernel::recompute_truncated_query_keys`].
    pub(in crate::kernel) etag_ptag_truncated_query_keys: Arc<std::sync::Mutex<HashSet<u64>>>,
    snapshot_builder: flatbuffers::FlatBufferBuilder<'static>, // Rung 3 D3-6: reset+to_vec pattern
    /// Kernel must not cross thread boundaries — D4 single-writer enforced at type level.
    _not_send: PhantomData<*const ()>,
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

    /// V-82 — like [`Self::with_storage_path`], but threads in an
    /// externally-owned [`ActiveAccountSlot`] instead of minting one.
    ///
    /// The actor thread (`run_actor_with_observers`) is the sole caller: it
    /// hands the kernel the SAME `Arc` the FFI shell (`nmp-ffi::NmpApp`) holds,
    /// so `NmpApp::active_account_handle()` reads the very slot the kernel
    /// actor writes on every identity mutation (`set_accounts`). The `Reset`
    /// dispatch arm rebuilds the kernel through this constructor with the
    /// actor-held slot so the shared handle survives a state wipe — mirroring
    /// how the routing-trace projection is re-published across `Reset`.
    ///
    /// Substrate-clean: the slot holds a raw pubkey `String` (D0).
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
    /// (`nmp-defaults::register_defaults` → the
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

    /// Set the TTL (time-to-live) policy for replaceable events (F-TTL).
    ///
    /// Configures how long replaceable identities (kind, pubkey, optional d_tag)
    /// remain "fresh" before a re-verification REQ is dispatched. Defaults to
    /// kind:0 = 1 hour, kind:10002 = 6 hours, all others = 6 hours.
    ///
    /// Can be called at any time; affects all subsequent TTL lookups via
    /// `ttl_for_kind()`.
    ///
    /// D4: `&mut self` — the kernel is the sole writer of this configuration.
    pub fn set_replaceable_ttl(&mut self, config: replaceable_ttl::ReplaceableTtlConfig) {
        self.replaceable_ttl = config;
    }

    /// Look up the TTL for a given replaceable event kind.
    ///
    /// Returns the kind-specific TTL if configured, otherwise the default TTL.
    /// Used by the F-TTL ingest path and pending_reverify queue to determine
    /// `check_again_after` values.
    ///
    /// D8: no clock — the kernel never calls this; callers pass millisecond
    /// timestamps (now + ttl_for_kind = check_again_after).
    #[must_use]
    pub(crate) fn ttl_for_kind(&self, kind: u32) -> std::time::Duration {
        self.replaceable_ttl.ttl_for_kind(kind)
    }

    /// F-TTL — enqueue a replaceable event for re-verification if its freshness
    /// has expired.
    ///
    /// Called when a view component claims a replaceable identity (kind, pubkey, d_tag?).
    ///
    /// TTL-gated (F-TTL): a claim only enqueues a re-verification REQ when the
    /// cached identity's `check_again_after` has elapsed. A fresh identity (one
    /// whose TTL has not yet expired) is a no-op — this is what stops the
    /// kernel from spamming a REQ on every claim. Identities the store has
    /// never stamped (`None` → treated as `0`) are always due, so a cold cache
    /// re-verifies eagerly.
    ///
    /// On enqueue we stamp `check_again_after = now + INFLIGHT_GUARD_MS` so that
    /// repeated claims for the same identity while a REQ is in flight (before
    /// its EOSE lands and re-stamps with the real per-kind TTL) do not re-enqueue.
    ///
    /// `force` bypasses the TTL gate: a forced claim treats the stored
    /// `check_again_after` as `0` (always due), so it enqueues a re-fetch even
    /// when the cached identity is still fresh. This is the "user explicitly
    /// navigated to / pulled-to-refresh this entity" path; the lazy, gated path
    /// (`force == false`) is what stops the kernel from spamming a REQ on every
    /// `.onAppear`. (Replaces the deleted `nmp_app_refresh_replaceable` FFI:
    /// force-refresh is now a `force` argument on the claim functions.)
    ///
    /// D9 clock seam: `now_ms()` reads the injected `Clock`.
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

    /// Number of replaceable identities currently queued for re-verification.
    /// Test-only window into the F-TTL gate (`claim_replaceable`).
    #[cfg(test)]
    pub(crate) fn pending_reverify_len(&self) -> usize {
        self.pending_reverify.len()
    }

    /// Sub-ids currently tracked for reverify EOSE handling.
    /// Test-only window into `reverify_subs` (introspection, not logic).
    #[cfg(test)]
    pub(crate) fn reverify_sub_ids_for_test(&self) -> Vec<String> {
        self.reverify_subs.keys().cloned().collect()
    }

    /// Seed a reverify sub_id → key mapping directly.
    ///
    /// Test-only: the production registration happens in `drain_pending_reverify`,
    /// but that path requires configured outbox relays to emit a REQ. This
    /// seam lets the EOSE re-stamp arm be tested in isolation from relay
    /// routing — it writes the same `reverify_subs` entry the drain would.
    #[cfg(test)]
    pub(crate) fn seed_reverify_sub_for_test(
        &mut self,
        sub_id: &str,
        keys: Vec<crate::store::ReplaceableKey>,
    ) {
        self.reverify_subs.insert(sub_id.to_string(), keys);
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

    /// Read the kernel's current active-account pubkey (lowercase canonical
    /// hex), or `None` if no active account is set.
    #[must_use]
    pub(crate) fn active_account_pubkey(&self) -> Option<&str> {
        self.active_account.as_deref()
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
        Self::with_optional_publish_store_path_and_account_slot(
            visible_limit,
            publish_store,
            storage_path,
            None,
        )
    }

    /// V-82 — innermost constructor. Adds an optional externally-supplied
    /// [`ActiveAccountSlot`] on top of [`Self::with_optional_publish_store_and_path`].
    ///
    /// `active_account_handle = None` (every existing caller — `Kernel::new`,
    /// `with_storage_path`, `with_publish_store`, `with_publish_store_and_path`)
    /// keeps the historical behaviour: the kernel mints its own slot. The actor
    /// thread is the ONLY caller passing `Some(slot)` (via
    /// [`Self::with_storage_path_and_account_slot`]); it threads in the same
    /// `Arc` the FFI shell (`nmp-ffi::NmpApp`) holds, so
    /// `NmpApp::active_account_handle()` reads the very slot the kernel actor
    /// writes on sign-in / account-switch / logout — a single source of truth,
    /// no divergent mirror. The local `active_account_handle` below (and its
    /// `Arc::clone` into the test-support outbox resolver) reference the
    /// supplied slot, so no internal consumer diverges either.
    ///
    /// Substrate-clean: the slot holds a raw pubkey `String` — no NIP noun, no
    /// protocol coupling (D0 stays clean; this is generic identity plumbing).
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

        // T129 — install the store-backed watermark resolver on the
        // subscription lifecycle. On reconnect, `recompile_and_diff` bumps
        // each non-ephemeral sub-shape's `since` to the newest stored
        // `created_at` matching that shape, so the relay does not re-emit
        // events already on disk. The closure captures a clone of the
        // `EventStore` handle and translates the `InterestShape` into a
        // per-author minimum watermark:
        //
        // - Exactly-one-author + ≥1 kind  → single `AuthorKind` scan.
        // - Multi-author + ≥1 kind         → per-author `AuthorKind` scan for
        //   every author; returns min(per-author newest) so the floor is safe
        //   for the entire shape. Any author with zero stored events forces
        //   `None` (no rewrite) — their history must be fetched in full.
        //   (V-118 fix: the old KindTime path returned newest-from-anyone,
        //   which could floor a newly-followed author above all their past
        //   events.)
        // - Zero authors + ≥1 kind         → `None` (no rewrite) — we cannot
        //   safely floor a global-kind scan without risking missing events.
        // - No kinds                        → `None`.
        //
        // `query_visit` with `limit = 1` early-stops at the newest stored
        // match on the `idx_author_kind` index per author (D8: no per-emit
        // allocation beyond one u64 per author in the shape).
        let watermark_store = Arc::clone(&store);
        // K3 Stage B3 / #1380 — completion-key WRITE set + query-key READ view (field docs).
        let etag_ptag_truncated_serves: Arc<std::sync::Mutex<HashSet<u64>>> =
            Arc::new(std::sync::Mutex::new(HashSet::new()));
        let etag_ptag_truncated_query_keys: Arc<std::sync::Mutex<HashSet<u64>>> =
            Arc::new(std::sync::Mutex::new(HashSet::new()));
        let watermark_truncated = Arc::clone(&etag_ptag_truncated_query_keys);
        let watermark_fn: crate::subs::WatermarkFn =
            Arc::new(move |shape: &crate::planner::InterestShape| {
                // ADR-0045 §6 / #1119: the watermark floor is now derived from
                // the SAME `shape_to_store_queries` mapping cache-serve uses —
                // "one table read two ways". `watermark_from_queries` folds the
                // per-query newest timestamps with the established policy (min
                // across AuthorKind with abort-on-empty author, min across
                // KindDtag coords with abort-on-empty coord (K3 Stage B1 — the
                // same min/abort rule as authors), single value for Etag/Ptag,
                // never-floor for the zero-author KindTime global feed). This makes the
                // floored⇒served invariant structural: an uncovered shape maps
                // to no queries, so it cannot be floored.
                //
                // The scan normalizes each query to its watermark form
                // (since/until = None) and reads the newest stored match via a
                // `limit = 1` early-stopping `query_visit` (D8: one u64 per
                // query, no per-emit allocation).
                cache_serve::watermark_from_queries(
                    shape,
                    |query| {
                        let mut q = query.clone();
                        if let Some(since) = cache_serve::query_since_mut(&mut q) {
                            *since = None;
                        }
                        if let Some(until) = cache_serve::query_until_mut(&mut q) {
                            *until = None;
                        }
                        let mut ts: Option<u64> = None;
                        let _ = watermark_store.query_visit(&q, 1, &mut |ev| {
                            ts = Some(ev.raw.created_at);
                            std::ops::ControlFlow::Break(())
                        });
                        ts
                    },
                    // K3 Stage B3 / #1380: refuse the floor for a cursor-less
                    // shape whose serve was budget-truncated this session. `key`
                    // is the query-content key; the captured read view holds it
                    // iff AT LEAST ONE active interest mapping to that query is
                    // truncated, so any contributing interest's truncation refuses
                    // the shared merged-REQ floor (the conservative, correct merge).
                    |key| {
                        watermark_truncated
                            .lock()
                            .map(|set| set.contains(&key))
                            .unwrap_or(false)
                    },
                )
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

        let mut kernel = Self {
            store,
            clock: Arc::new(SystemClock),
            rev: 0,
            visible_limit,
            // ADR-0055 Rung 1: initialized to default (all counters 0, epoch 0).
            // Resets are free on the Kernel rebuild (Reset) path.
            projection_rev_tracker: projection_rev::ProjectionRevTracker::default(),
            #[cfg(any(test, feature = "test-support"))]
            projection_oracle: projection_rev::oracle::OracleState::default(),
            timing: TimingMilestones::default(),
            relays: RelayRole::all()
                .into_iter()
                .map(|role| (role, RelayHealth::default()))
                .collect(),
            transport_relays: RelayTransportMap::default(),
            profiles: HashMap::new(),
            events: HashMap::new(),
            metric_note_events: 0,
            metric_duplicate_events: 0,
            metric_stored_events: 0,
            cached_estimated_store_bytes: std::cell::Cell::new(None),
            timeline: VecDeque::new(),
            diagnostic_firehose: DiagnosticFirehoseState::default(),
            deferred_outbound: VecDeque::new(),
            pending_backoff_hints: Vec::new(),
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
            pre_kind3_buffer: BoundedMessageMap::new(MAX_PROJECTION_MESSAGES),
            follow_feed_interest_ids: BTreeSet::new(),
            follow_feed_kinds: BTreeSet::new(),
            profile_claims: HashMap::new(),
            event_claims: HashMap::new(),
            event_claim_requested: BTreeSet::new(),
            event_claim_released: crate::substrate::BoundedRing::new(MAX_PROJECTION_MESSAGES),
            event_claim_released_observers: Vec::new(),
            pending_event_claims: Vec::new(),
            event_claim_drops_total: 0,
            profile_requests: ProfileRequestState::default(),
            timeline_requested: false,
            contacts_deadline: None,
            wire: WireSubscriptionState::default(),
            // K3 Stage D1: OFF by default — D1 ships the write path dormant.
            coverage_ledger_enabled: false,
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
            action_stages: action_stages::ActionStageTracker::new(),
            action_lifecycle: action_lifecycle::ActionLifecycleTracker::new(),
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
            dispatch_drops: None,
            queue_depth: None,
            lifecycle_phase: LifecyclePhase::Inactive,
            event_observers: None,
            raw_event_observers: None,
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
            store_open_failure,
            negentropy_sync_stats: types::NegentropySyncStats::default(),
            last_gc: None,
            last_gc_at_ms: None,
            served_interest_shapes: HashSet::new(),
            pending_cache_serves: VecDeque::new(),
            etag_ptag_truncated_serves,
            etag_ptag_truncated_query_keys,
            snapshot_builder: flatbuffers::FlatBufferBuilder::new(), // ADR-0055 Rung 3 (D3-6)
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
    // Always compiled: the test-support kernel-clock injection seam
    // (`NmpApp::set_kernel_clock_for_test` → actor → here) is the production
    // code path that calls this, even though production never installs a
    // non-default clock. `allow(dead_code)` covers builds with neither the
    // `test` cfg nor any clock-injecting consumer linked.
    // `pub` under `test-support` (not just `pub(crate)`) so external crate
    // integration tests — e.g. `nmp-nip77`'s NEG-OPEN liveness-deadline oracle
    // (K3 Stage B2) — can install a `MonotonicSecondClock` and advance it to
    // drive a wall-clock-gated `on_idle_tick` without a real sleep (D8).
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
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Current wall-clock time as milliseconds since the Unix epoch, read
    /// through the injected [`Clock`] so `FixedClock` keeps it deterministic
    /// (used by the `action_stages` mirror and the `start()` wall anchor). A
    /// pre-epoch clock collapses to `0` (D6 — never panics).
    pub(crate) fn now_ms(&self) -> u64 {
        self.clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// #1069 — run one bounded GC pass against the store on the production
    /// budget, recording the result so the schedule is observable.
    ///
    /// Called from the actor's 60-second idle-tick gate (`actor/mod.rs`). This
    /// is the sole production caller of [`EventStore::gc_step`] — before this,
    /// the store's GC machinery (NIP-40 expiry reaping, LRU eviction, tombstone
    /// purge) was dead on every device (audit Finding 1).
    ///
    /// - **Budget**: [`GcBudget::production`] — `2000` events / `50 ms` scan
    ///   bounds, LRU ceiling at [`crate::store::HOT_EVENT_CEILING`] (10 000
    ///   events, enabled by #1090 Stage 3 / #1327). When the floor-coherent
    ///   pin scan is truncated by its D8 budget, [`Self::derive_store_gc_inputs`]
    ///   returns a no-eviction budget so LRU is skipped this tick (#1348).
    /// - **`now_secs`**: read through the injected [`Clock`] via
    ///   [`Self::now_secs`] (D7/D9 — the store never reads the clock; the kernel
    ///   threads it in, so replay/tests stay deterministic).
    /// - **Cooperative**: runs on the actor thread between mailbox messages; the
    ///   budget bounds the worst-case latency, never an FFI call path (`gc.md` §3).
    ///
    /// A store error is surfaced as `tracing::warn!` and the pass is skipped —
    /// gc is best-effort maintenance (D1), never a correctness gate; the next
    /// tick retries. The result (or the prior one on error) stays in `last_gc`.
    pub fn run_gc_step(&mut self) -> Option<crate::store::GcReport> {
        let now_secs = self.now_secs();
        // #1088 — RAM-tier eviction runs on every GC pass regardless of
        // whether the store pass succeeds.  This is a separate call site from
        // the LMDB-tier gc_step (#1085) so the two paths stay independent and
        // merge-clean.
        let ram_report = self.evict_ram_caches();
        if ram_report.events_evicted
            + ram_report.profiles_evicted
            + ram_report.seed_contacts_evicted
            > 0
        {
            tracing::debug!(
                events_evicted = ram_report.events_evicted,
                profiles_evicted = ram_report.profiles_evicted,
                seed_contacts_evicted = ram_report.seed_contacts_evicted,
                "ram cache eviction pass",
            );
        }
        // #1090 Stage 1 — derive the ephemeral store-tier pin set and the
        // matching budget (#1348 truncation→no-eviction decision lives in
        // `derive_store_gc_inputs`), then thread both into Phase-2 LRU eviction.
        let (pins, gc_budget) = self.derive_store_gc_inputs();
        match self.store.gc_step_with_pins(gc_budget, now_secs, &pins) {
            Ok(report) => {
                self.last_gc_at_ms = Some(self.now_ms());
                self.last_gc = Some(report.clone());
                Some(report)
            }
            Err(e) => {
                tracing::warn!(error = %e, "gc_step failed; skipping this pass");
                None
            }
        }
    }

    /// #1069 — the last [`GcReport`](crate::store::GcReport) produced by
    /// [`Self::run_gc_step`], or `None` if no gc pass has run yet. Read by
    /// diagnostics so the GC schedule is observable (`gc.md` §7).
    pub fn last_gc(&self) -> Option<&crate::store::GcReport> {
        self.last_gc.as_ref()
    }

    /// #1069 — wall-clock time (Unix ms, from the injected [`Clock`]) of the
    /// last [`Self::run_gc_step`], or `None` if no gc pass has run yet.
    pub fn last_gc_at_ms(&self) -> Option<u64> {
        self.last_gc_at_ms
    }

    /// Resolve the configured relay URLs for a given `RelayRole` from the
    /// app-provided `configured_relays`.
    ///
    /// Returns an **empty** vec when no relay is configured for the requested
    /// role. Production no longer falls back to a hardcoded default: the app is
    /// responsible for declaring its initial relay set (via
    /// `NmpAppBuilder::with_relay(s)` or pre-start `nmp_app_add_relay`), which
    /// is carried into the kernel through `ActorCommand::Start { initial_relays }`.
    /// An empty result is surfaced to the host through the
    /// `no_configured_relays` diagnostic (V-66) — the kernel never silently
    /// dials an unconsented relay.
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

    /// The cold-start discovery seed as an owned `Vec`.  Reads from the
    /// app-provided `configured_relays`; returns an empty vec when nothing is
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

    /// Advance the kernel's `rev` counter so that its first `make_update` call
    /// yields a rev **strictly greater** than the supplied `floor`.
    ///
    /// This is used exclusively by the actor startup path when a pre-flight
    /// snapshot has already been emitted from a temporary kernel: by setting
    /// the real kernel's `rev` to `floor` before its first `make_update`, the
    /// first emitted frame carries `rev = floor + 1`, which passes the iOS
    /// host's `guard update.rev > rev` monotonicity check regardless of what
    /// the pre-flight frame's rev was.
    ///
    /// `floor` must equal the pre-flight kernel's `rev` after its single
    /// `make_update(false)` call (= 1 today; generalises if the pre-flight
    /// path ever emits more frames).  Setting `rev` here is safe because the
    /// real kernel has not yet called `make_update` — `rev` is still 0.
    /// Returns the current revision counter (the `rev` that was stamped on
    /// the most recently emitted `make_update` frame).
    ///
    /// Used by the actor startup path to capture the pre-flight kernel's rev
    /// so that the real kernel can be advanced past it via
    /// [`Self::resume_rev_after_preflight`], guaranteeing strict monotonicity
    /// across the pre-flight → Start frame sequence.
    pub(crate) fn current_rev(&self) -> u64 {
        self.rev
    }

    pub(crate) fn resume_rev_after_preflight(&mut self, floor: u64) {
        self.rev = floor;
    }

    /// T114b — extract the FFI-channel drop-counter handle before a `Reset`
    /// replaces the kernel. The dispatch drops counter is process-lifetime
    /// (shared with the FFI forwarder thread) so the Reset path moves it
    /// onto the fresh kernel via `set_dispatch_drops_handle`.
    pub(crate) fn take_dispatch_drops_handle_for_reset(&mut self) -> Option<Arc<AtomicU64>> {
        self.dispatch_drops.take()
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

    /// Bind the shared relay-edit rows slot so the FFI layer can read
    /// relay-edit rows without reaching into kernel internals.
    ///
    /// The slot is a typed [`AppRelaySlot`] (`Arc<Mutex<AppRelayList>>`).
    pub(crate) fn set_app_relay_slot(&mut self, handle: AppRelaySlot) {
        self.configured_relays_handle = Some(handle);
    }

    /// Extract the relay-edit rows handle before a `Reset` replaces the
    /// kernel. The underlying `Arc` is process-lifetime and must survive
    /// across kernel reinstantiation.
    pub(crate) fn take_app_relay_slot_for_reset(&mut self) -> Option<AppRelaySlot> {
        self.configured_relays_handle.take()
    }

    /// Test-only seam — clear the kernel's `configured_relays` so the empty
    /// bootstrap state can be exercised end-to-end.
    ///
    /// `bootstrap_urls_for_role` has a `#[cfg(test)]` fallback that seeds a
    /// default Content/Indexer relay when `configured_relays` is empty (see
    /// `kernel/mod.rs::bootstrap_urls_for_role`'s `#[cfg(test)] if urls.is_empty()`
    /// block). That fallback exists so the vast majority of unit tests don't
    /// need to hand-roll a relay seed for every fresh kernel. The D10
    /// defensive-guard test wants the OPPOSITE — a kernel whose
    /// `configured_relays` is empty AND whose `bootstrap_urls_for_role`
    /// returns empty, so the dispatch path that lands a kind:1059 envelope
    /// in `publish_signed_event` with `relays: vec![]` cannot accidentally
    /// pass the guard via the cfg(test) backstop.
    ///
    /// `pub(crate)` is sufficient — no FFI / cross-crate caller; the
    /// `commands` tests reach it through the kernel's internal API.
    #[cfg(test)]
    pub(crate) fn clear_configured_relays_for_test(&mut self) {
        self.configured_relays.clear();
        if let Some(handle) = self.configured_relays_handle.as_ref() {
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

    // ADR-0055 Rung 1 — `projection_manifest()` / `projection_state()` live in
    // `projection_rev/kernel_impl.rs` (sibling) to keep this file at baseline.

    /// Mutable access to the subscription lifecycle (registry + trigger inbox).
    ///
    /// The actor-side `KernelAction` reducer (T95) uses this to register the
    /// `LogicalInterest` resolved from an `OpenUri` action through the
    /// single-writer [`crate::subs::InterestRegistry`] (D4). Kept crate-private
    /// so the FFI surface never sees a subscription-internal type (D0/D6).
    pub(crate) fn lifecycle_mut(&mut self) -> &mut SubscriptionLifecycle {
        &mut self.lifecycle
    }

    /// M2 (ADR-0042) — attach one owner to a generic feed interest and, when the
    /// `(scope, key)` slot was newly installed, enqueue a recompile trigger so
    /// the next compile pass emits the REQ. Shared by the `OpenInterest` and
    /// `CloseInterest` dispatch arms (open calls this; close calls
    /// [`Kernel::close_interest_sub`]).
    ///
    /// ADR-0045 E1: when the interest is newly installed, also serve stored
    /// events matching the interest's shape directly into projections — the
    /// first half of the one event-acquisition mechanism. The serve is
    /// one-shot per (key, shape-content) hash so reconnect recompiles do not
    /// re-replay what the projection already has.
    ///
    /// Returns `true` iff the interest was newly installed (the caller may use
    /// this for diagnostics; the trigger enqueue is handled here so the two
    /// dispatch arms cannot drift on the "trigger only on change" invariant).
    pub(crate) fn open_interest_sub(
        &mut self,
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    ) -> bool {
        // ADR-0045 E1 — delegate to the single ensure-install front door so the
        // "register-if-absent → trigger + store-serve on newly-installed" recipe
        // lives in exactly one place (shared with the EnsureInterest dispatch
        // arm and the open_uri resolver).
        self.ensure_interest_and_serve(identity, interest, "open-interest")
    }

    /// M2 (ADR-0042) — detach one owner from a generic feed interest and, when
    /// the last owner left (slot removed), enqueue a recompile trigger so the
    /// next compile pass closes the REQ. Counterpart to
    /// [`Kernel::open_interest_sub`].
    ///
    /// Returns `true` iff the slot was removed (last owner left).
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

    /// Pre-populate `seed_contacts` for a given pubkey with the specified follows.
    /// Used during account creation so the follow-feed can be set up immediately
    /// without waiting for the kind:3 event to round-trip from relays.
    pub(crate) fn prepopulate_seed_contacts(&mut self, pubkey: String, follows: Vec<String>) {
        self.seed_contacts.insert(pubkey, follows);
        self.cached_estimated_store_bytes.set(None);
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

    /// V-67 test seam — inject a `store_open_failure` string as if `build_event_store`
    /// had failed to open the LMDB path. Lets unit tests verify that the failure is
    /// projected through `make_update` without requiring the `lmdb-backend` feature or
    /// a real filesystem failure. Mirrors the `set_planner_error_for_test` seam (T171).
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_store_open_failure_for_test(&mut self, reason: impl Into<String>) {
        self.store_open_failure = Some(reason.into());
    }

    /// V-66 test seam — set `active_account` directly so unit tests can exercise
    /// the "signed in but no configured relays" diagnostic path without wiring up
    /// the full account-creation flow. Production code sets `active_account` via
    /// `identity_state::update_accounts`; this seam is test-only.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.active_account = Some(pubkey.into());
    }

    /// Seed a sentinel in the pre-kind:3 buffer (test-only).
    #[cfg(test)]
    pub(crate) fn seed_pre_kind3_buffer_for_test(&mut self, event_id: impl Into<String>) {
        let id = event_id.into();
        let e = NostrEvent {
            id: id.clone(),
            pubkey: "d".repeat(64),
            created_at: 0,
            kind: 1,
            tags: vec![],
            content: String::new(),
            sig: "s".repeat(128),
        };
        self.pre_kind3_buffer.insert(id, (e, String::new()));
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

    /// Inject the blocked-relay lookup (composition seam). Apps that depend on
    /// `nmp-router` call this after `Kernel::new` to install a shared
    /// `Arc<InMemoryBlockedRelayCache>` so the `build_routing_context` reader,
    /// the kind:10006 ingest parser writer, AND the publish engine all see the
    /// same cache. Default is [`crate::substrate::EmptyBlockedRelayLookup`]
    /// (zero-block). MUST be called BEFORE the first kind:10006 ingest — the
    /// caches are independent stores, so a swap after ingest loses entries.
    pub(crate) fn set_blocked_relay_lookup(&mut self, lookup: Arc<dyn BlockedRelayLookup>) {
        // Forward to the publish engine (privacy fix — the outbox resolver
        // must also exclude blocked relays); then keep the routing-side handle.
        self.publish_engine
            .set_blocked_relay_lookup(Arc::clone(&lookup));
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

    /// V-58 — drain any pending backoff hints enqueued during the last
    /// `handle_message` call. The actor calls this immediately after each
    /// inbound frame dispatch to forward hints to the pool worker.
    ///
    /// Returns an empty `Vec` (no allocation) when there are no hints.
    /// The returned `Vec` is owned; the kernel's queue is cleared on return.
    pub(crate) fn take_backoff_hints(&mut self) -> Vec<(String, BackoffHint)> {
        std::mem::take(&mut self.pending_backoff_hints)
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
