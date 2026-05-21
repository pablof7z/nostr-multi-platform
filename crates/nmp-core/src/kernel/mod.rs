//! Kernel — the actor-owned event-processing core.
//!
//! Sub-modules:
//! - `types`        — pure data types shared across the kernel
//! - `ingest`       — relay frame parsing, event dispatch, and kind-specific ingest
//! - `requests`     — relay state transitions, startup/view REQs, req/defer primitives
//! - `status`       — diagnostics, metrics, and update-payload assembly
//! - `update`       — diff/emit logic for the FFI update loop
//! - `nostr`        — NostrEvent deserialization + helper functions
//! - `test_support` — signature-free injection helpers (test / test-support feature)
//! - `tests`        — unit tests (cfg(test) only)

// M6 (first half) — the runtime that drives the `substrate::ActionModule`
// trait. `pub(crate)` so the crate-private `ffi` module can reach
// `ActionRegistry` / `default_registry` for the `nmp_app_dispatch_action`
// entry point.
pub(crate) mod action_registry;
#[cfg(test)]
mod action_failure_tests;
mod auth;
mod clock;
#[cfg(test)]
mod clock_injection_tests;
#[cfg(test)]
mod closed_classifier_tests;
// `pub(crate)` so the typed FFI error-category constants (`ERR_*`) are
// reachable from the `actor` module's command handlers, not just kernel-
// internal callsites.
pub(crate) mod closed_reason;
mod discovery;
#[cfg(test)]
mod discovery_tests;
#[cfg(test)]
mod eose_ok_notice_ingest_tests;
mod event_observer;
#[cfg(test)]
mod event_observer_tests;
mod identity_state;
mod ingest;
#[cfg(test)]
mod ingest_tests;
mod lifecycle;
mod local_publish_intent;
#[cfg(test)]
mod local_publish_intent_tests;
mod nostr;
mod outbox;
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
mod publish_terminal_status_tests;
// Diagnostics-screen projection — pre-rolled relay/wire-sub roll-ups +
// pre-formatted display strings. Replaces the §4.5 / §6 anti-pattern #1
// derivations the three iOS diagnostics views used to do client-side. See
// the module doc for the bible references.
mod relay_diagnostics;
mod raw_event_observer;
#[cfg(test)]
mod raw_event_observer_tests;
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
mod timeline_perf_tests;
#[cfg(test)]
mod timeline_order_tests;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
#[cfg(test)]
mod tests;
mod types;
mod update;

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
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::Message;

use nostr::*;
pub(crate) use nostr::{is_hex_id, is_hex_pubkey};

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
use auth::{AuthSignerFn, Nip42DriverState};
use clock::{Clock, SystemClock};
// M6 — action-dispatch runtime, reachable from the `ffi` module for the
// `nmp_app_dispatch_action` entry point.
pub(crate) use action_registry::{default_registry, ActionRegistry};
pub(crate) use identity_state::{
    AccountSummary, PublishQueueEntry, RelayAckOutcome, RelayEditRow,
};
// Host-extensible snapshot output — reachable from the `ffi` module for the
// `nmp_app_register_snapshot_projection` C-ABI entry point.
pub(crate) use snapshot_registry::{new_snapshot_projection_slot, SnapshotProjectionSlot};
pub(crate) use lifecycle::{LifecyclePhase, LifecycleTransition};
// D0: NIP-47 NWC is an app noun. `WalletStatus` no longer lives in the kernel
// — it moved to the wallet command runtime (`actor::commands::wallet`) and is
// surfaced via the `projections["wallet"]` snapshot projection, NOT a typed
// `KernelSnapshot` field. The kernel never names the NWC noun.
use std::sync::atomic::{AtomicU64, Ordering};
use types::*;

/// Per-pubkey claim consumer-id retention cap (T114b — per-dispatch retention audit).
///
/// `profile_claims[pk]: BTreeSet<consumer_id>` grows once per `claim_profile` call;
/// without a cap a long-lived process accumulates consumer_ids in proportion to
/// dispatch count rather than working-set size (a D8 violation — see PD-021
/// line-11 and `docs/perf/m10.5/s2-drain-analysis.md`). The S2 flood mix issues
/// unique consumer_ids per dispatch with no matching release, isolating this leak.
///
/// 256 is generous for legitimate UI: every concurrent view that
/// calls `ProfileInterestAvatar` carries its own consumer_id; real apps hold
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
pub(crate) struct Kernel {
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
    profiles: HashMap<String, Profile>,
    /// Locally-authored kind:0 publish intents that have not necessarily
    /// round-tripped from a relay yet. Kept separate from `profiles` so
    /// `metrics.profile_events` still counts only real relay/store ingest.
    local_profile_intents: HashMap<String, Profile>,
    events: HashMap<String, StoredEvent>,
    /// Incrementally-maintained diagnostic counters for the `Metrics` snapshot
    /// fields `note_events` / `duplicate_events` / `stored_events`. Maintained
    /// at the `events` ingest/mutation sites so `make_update` (up to 60 Hz)
    /// never has to walk the whole `events` HashMap to recompute them — see
    /// `docs/perf` and the O(events) snapshot-emit violation this replaced.
    ///
    /// `events` is insert-only today (no eviction path mutates the HashMap;
    /// `sort_timeline` truncates only the `timeline` VecDeque). The
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
    author_relay_lists: HashMap<String, AuthorRelayList>,
    /// NIP-17 kind:10050 DM-relay lists, keyed by author pubkey (hex). Each
    /// value is the deduped, canonicalized set of DM-inbox relay URLs the
    /// author declared. Populated by `ingest_dm_relay_list`; read by
    /// `recipient_dm_relays` to pin kind:1059 gift-wrap envelopes to their
    /// receiver's DM-inbox relays (NIP-17 § 2). Deliberately distinct from
    /// `author_relay_lists` (kind:10002) — DM routing must not leak onto the
    /// public NIP-65 mailbox.
    dm_relay_lists: HashMap<String, Vec<String>>,
    timeline_authors: BTreeSet<String>,
    /// T140 — M2 follow-feed interest tracking. Maps each currently-registered
    /// follow-feed `InterestId` so `sync_follow_feed_interests` can withdraw
    /// stale entries before re-registering on kind:3 change. Derived from the
    /// active account's kind:3 follow set; empty until first kind:3 arrives.
    follow_feed_interest_ids: BTreeSet<crate::planner::InterestId>,
    profile_claims: HashMap<String, BTreeSet<String>>,
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
    events_since_last_update: u64,
    max_event_to_emit_ms: u128,
    max_events_per_update: u64,
    changed_since_emit: bool,
    logs: VecDeque<String>,
    /// M5+M2+M8 wiring: per-relay NIP-42 driver state. One entry per
    /// `RelayRole`. Default `NotRequired`; an inbound `AUTH` frame transitions
    /// to `ChallengeReceived` and triggers signer invocation.
    nip42_drivers: HashMap<RelayRole, Nip42DriverState>,
    /// M5+M2+M8 wiring: subscription lifecycle. Today the kernel uses ONLY
    /// `handle_auth_state_change` (diagnostic state fan-in to AuthGate); the
    /// compile / registry / wire-diff machinery stays dormant because the
    /// kernel's M1 hand-rolled `req()` path is still authoritative per
    /// `docs/plan/m8-subscription-lifecycle.md` §4 (both paths coexist until
    /// M11 migrates view modules onto `LogicalInterest`). The AuthGate's
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
    oneshot_subs: HashMap<String, (crate::subs::OneshotToken, discovery::OneshotKind)>,
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
    publish_engine: crate::publish::PublishEngine,
    /// Buffered (relay_url, frame) pairs produced by the engine. The kernel
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
    /// pubkey's consumer_id set hit `MAX_CLAIMS_PER_PUBKEY`. Surfaced on the
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
    relay_edit_rows_handle: Option<Arc<Mutex<Vec<RelayEditRow>>>>,
    /// Shared list of indexer relay URLs, kept in sync with `relay_edit_rows`
    /// by `set_relay_edit_rows`. The `Nip65OutboxResolver` holds a clone of
    /// this Arc and reads it on every discovery-kind publish.
    indexer_relays_handle: Arc<Mutex<Vec<String>>>,
    /// Shared list of local write relays for the active account. This bridges
    /// onboarding relay rows into publish routing before the user's freshly
    /// published kind:10002 has round-tripped from a relay.
    local_write_relays_handle: Arc<Mutex<Vec<String>>>,
    /// Shared active-account pubkey used by the publish resolver to scope the
    /// local relay-row fallback to the viewer's own events only.
    active_account_handle: Arc<Mutex<Option<String>>>,
    /// Kernel must not cross thread boundaries — D4 single-writer enforced at type level.
    _not_send: PhantomData<*const ()>,
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
fn build_event_store(storage_path: Option<&str>) -> Arc<dyn EventStore> {
    #[cfg(feature = "lmdb-backend")]
    {
        // Priority 1: FFI-supplied path. Priority 2: env-var fallback.
        let resolved: Option<String> = storage_path
            .map(str::to_owned)
            .or_else(|| std::env::var("NMP_LMDB_PATH").ok());
        if let Some(path) = resolved {
            // D6: silent fallback to the in-memory store if the open fails.
            if let Ok(s) = crate::store::LmdbEventStore::open(std::path::Path::new(&path)) {
                return Arc::new(s);
            }
        }
    }
    // `storage_path` is unused when the `lmdb-backend` feature is off.
    #[cfg(not(feature = "lmdb-backend"))]
    let _ = storage_path;
    Arc::new(MemEventStore::new())
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
    let resolved: Option<String> = storage_path
        .map(str::to_owned)
        .or_else(|| std::env::var("NMP_LMDB_PATH").ok());
    if let Some(path) = resolved {
        // Durable, feature-flag-independent: offline intents survive restart.
        return Arc::new(crate::publish::FsPublishStore::new(path));
    }
    // No storage path: fall back to the LMDB-domain store (durable only under
    // `lmdb-backend`), then the in-memory store. This keeps CI/test behaviour
    // (no storage path -> no on-disk artefacts) unchanged.
    crate::publish::DomainPublishStore::open(Arc::clone(event_store))
        .map(|store| Arc::new(store) as Arc<dyn crate::publish::PublishStore>)
        .unwrap_or_else(|_| Arc::new(crate::publish::InMemoryPublishStore::new()))
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
            .map(|existing: &Profile| existing.created_at <= profile.created_at)
            .unwrap_or(true);
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
    pub(crate) fn with_storage_path(visible_limit: usize, storage_path: Option<&str>) -> Self {
        Self::with_optional_publish_store_and_path(visible_limit, None, storage_path)
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
        let store: Arc<dyn EventStore> = build_event_store(storage_path);
        let publish_store = publish_store
            .unwrap_or_else(|| resolve_publish_store(storage_path, &store));
        let local_profile_intents = load_profile_intents(&publish_store);
        let publish_dispatcher = Arc::new(crate::publish::QueueDispatcher::new());
        let indexer_relays_handle: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let local_write_relays_handle: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let active_account_handle: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let publish_engine = publish_engine::build_engine(
            Arc::clone(&store),
            Arc::clone(&publish_dispatcher),
            Arc::clone(&publish_store),
            Arc::clone(&indexer_relays_handle),
            Arc::clone(&local_write_relays_handle),
            Arc::clone(&active_account_handle),
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

        Self {
            store,
            clock: Arc::new(SystemClock),
            rev: 0,
            visible_limit,
            timing: TimingMilestones::default(),
            relays: RelayRole::all()
                .into_iter()
                .map(|role| (role, RelayHealth::default()))
                .collect(),
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
            author_relay_lists: HashMap::new(),
            dm_relay_lists: HashMap::new(),
            timeline_authors: BTreeSet::new(),
            follow_feed_interest_ids: BTreeSet::new(),
            profile_claims: HashMap::new(),
            profile_requests: ProfileRequestState::default(),
            timeline_requested: false,
            contacts_deadline: None,
            wire: WireSubscriptionState::default(),
            last_emitted_items: Vec::new(),
            update_sequence: 0,
            last_payload_bytes: 0,
            events_since_last_update: 0,
            max_event_to_emit_ms: 0,
            max_events_per_update: 0,
            changed_since_emit: true,
            logs: VecDeque::new(),
            nip42_drivers: RelayRole::all()
                .into_iter()
                .map(|role| (role, Nip42DriverState::new()))
                .collect(),
            lifecycle,
            unknown_ids: UnknownIds::new(),
            oneshot: OneshotApi::new(),
            oneshot_subs: HashMap::new(),
            auth_signers: HashMap::new(),
            accounts: Vec::new(),
            active_account: None,
            publish_queue: Vec::new(),
            last_error_toast: None,
            last_error_category: None,
            relay_edit_rows: Vec::new(),
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
            _not_send: PhantomData,
        }
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
    pub(crate) fn now_secs(&self) -> u64 {
        self.clock
            .now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Resolve the configured bootstrap URLs for a given `RelayRole` from the
    /// app-provided `relay_edit_rows`.  Empty when the operator has not yet
    /// configured any relays for that role.
    pub(crate) fn bootstrap_urls_for_role(&self, role: RelayRole) -> Vec<String> {
        let matches = |row_role: &str| match role {
            RelayRole::Content => {
                crate::actor::has_role(row_role, "read")
                    || crate::actor::has_role(row_role, "write")
            }
            RelayRole::Indexer => crate::actor::has_role(row_role, "indexer"),
            RelayRole::Wallet => false,
        };
        // `mut` is required only under `#[cfg(test)]` where the fallback
        // block may reassign `urls`; non-test builds never mutate it.
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut urls: Vec<String> = self
            .relay_edit_rows
            .iter()
            .filter(|r| matches(&r.role))
            .map(|r| r.url.clone())
            .collect();
        #[cfg(test)]
        if urls.is_empty() {
            urls = match role {
                RelayRole::Content => vec!["wss://relay.damus.io".to_string()],
                RelayRole::Indexer => vec!["wss://purplepag.es".to_string()],
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
        urls.sort();
        urls.dedup();
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
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
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
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0);
        depth.min(u32::MAX as u64) as u32
    }

    /// T114b — number of `claim_profile` requests dropped because a pubkey's
    /// consumer_id set hit `MAX_CLAIMS_PER_PUBKEY`. Read-only accessor; the
    /// counter is owned by the kernel and mutated only by `claim_profile`.
    pub(crate) fn claim_drops_total(&self) -> u64 {
        self.claim_drops_total
    }

    #[cfg(test)]
    pub(crate) fn claim_drops_total_test(&self) -> u64 {
        self.claim_drops_total
    }

    #[cfg(test)]
    pub(crate) fn profile_claims_len_for_test(&self, pubkey: &str) -> usize {
        self.profile_claims
            .get(pubkey)
            .map(|consumers| consumers.len())
            .unwrap_or(0)
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
    /// Generic per-role NIP-42 primitive (D0). The only non-test caller today
    /// is the `wallet` feature's NWC lane, so without that feature this is
    /// dead code — `allow(dead_code)` keeps the D0-proof (`--no-default-features`)
    /// build warning-clean without gating a kernel primitive on an app noun.
    #[cfg_attr(not(feature = "wallet"), allow(dead_code))]
    pub(crate) fn set_relay_auth_signer(
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
    /// Generic per-role NIP-42 primitive (D0); see `set_relay_auth_signer`
    /// for the `allow(dead_code)` rationale.
    #[cfg_attr(not(feature = "wallet"), allow(dead_code))]
    pub(crate) fn clear_relay_auth_signer(&mut self, role: RelayRole) {
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
                signer: signer.clone(),
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

    /// Bind the shared `Arc<Mutex<Vec<RelayEditRow>>>` handle so the FFI
    /// layer can read relay-edit rows without reaching into kernel internals.
    pub(crate) fn set_relay_edit_rows_handle(&mut self, handle: Arc<Mutex<Vec<RelayEditRow>>>) {
        self.relay_edit_rows_handle = Some(handle);
    }

    /// Extract the relay-edit rows handle before a `Reset` replaces the
    /// kernel. The underlying `Arc` is process-lifetime and must survive
    /// across kernel reinstantiation.
    pub(crate) fn take_relay_edit_rows_handle_for_reset(
        &mut self,
    ) -> Option<Arc<Mutex<Vec<RelayEditRow>>>> {
        self.relay_edit_rows_handle.take()
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
    pub(crate) fn register_persistent_sub(
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
    pub(crate) fn unregister_persistent_sub(&mut self, relay_url: &str, sub_id: &str) {
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
    pub(crate) fn mark_changed_since_emit(&mut self) {
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
        let relay_list = parse_relay_list(&event_id, created_at, &tags);
        let empty = relay_list.read_relays.is_empty()
            && relay_list.write_relays.is_empty()
            && relay_list.both_relays.is_empty();
        if empty {
            self.author_relay_lists.remove(&pubkey);
        } else {
            self.author_relay_lists.insert(pubkey.clone(), relay_list);
        }
        self.lifecycle
            .enqueue_trigger(CompileTrigger::Nip65Arrived { pubkey, created_at });
    }
}
