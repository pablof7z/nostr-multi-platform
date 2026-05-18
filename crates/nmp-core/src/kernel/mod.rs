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

mod auth;
mod discovery;
#[cfg(test)]
mod discovery_tests;
mod identity_state;
mod ingest;
mod nostr;
mod outbox;
#[cfg(test)]
mod outbox_tests;
mod provenance;
#[cfg(test)]
mod provenance_wire_tests;
mod publish_cmd;
mod publish_engine;
#[cfg(test)]
mod publish_engine_tests;
#[cfg(test)]
mod publish_terminal_status_tests;
mod publish_engine_wire;
mod requests;
#[cfg(test)]
mod retention_tests;
mod status;
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

use crate::relay::{
    OutboundMessage, RelayRole, CONTENT_RELAY_URL, DEFAULT_EMIT_HZ, FIATJAF_PUBKEY,
    INDEXER_RELAY_URL, JB55_PUBKEY, TEST_NPUB, TEST_PUBKEY, TIMELINE_AUTHOR_LIMIT,
};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
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
use crate::subs::{OneshotApi, SubscriptionLifecycle, UnknownIds};
use auth::{AuthSignerFn, Nip42DriverState};
pub(crate) use identity_state::{
    AccountSummary, PublishQueueEntry, RelayAckOutcome, RelayEditRow, WalletStatus,
};
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
/// 256 is generous for legitimate UI: every concurrent SwiftUI view that
/// calls `ProfileInterestAvatar` carries its own consumer_id; real apps hold
/// at most a few dozen simultaneously (one per visible row in a list view).
/// Caps worst-case retention per pubkey at ~12 KiB (256 × ~50 B node + key);
/// across 50 pubkeys (S2's working set) that's ~600 KiB, well under the 1 MiB
/// D8 budget. The S2 30 s flood (60 k claims across 50 pubkeys → ~1.2 k per
/// pubkey) hits the cap by design — that is the audit's load-bearing test.
///
/// Drop-newest semantics: a claim attempt past the cap silently no-ops and
/// increments `claim_drops_total`. This mirrors the bounded actor channel's
/// drop-newest policy (`BOUNDED_ACTOR_CMD_CAPACITY` in `actor/mod.rs`) — see
/// the audit table in `retention_tests.rs` for the per-structure rationale.
pub(crate) const MAX_CLAIMS_PER_PUBKEY: usize = 256;

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
    rev: u64,
    visible_limit: usize,
    started_at: Option<Instant>,
    last_event_at: Option<Instant>,
    first_event_at: Option<Instant>,
    target_profile_loaded_at: Option<Instant>,
    timeline_opened_at: Option<Instant>,
    timeline_first_item_at: Option<Instant>,
    relays: HashMap<RelayRole, RelayHealth>,
    profiles: HashMap<String, Profile>,
    events: HashMap<String, StoredEvent>,
    timeline: VecDeque<String>,
    selected_author: Option<ViewInterest>,
    author_request_pending: bool,
    author_view_seq: u64,
    selected_thread: Option<ViewInterest>,
    thread_request_pending: bool,
    thread_view_seq: u64,
    diagnostic_firehose: Option<ViewInterest>,
    diagnostic_firehose_seq: u64,
    diagnostic_firehose_events: u64,
    pending_thread_ids: BTreeSet<String>,
    requested_thread_ids: HashSet<String>,
    thread_ids_inflight: bool,
    pending_thread_reply_targets: BTreeSet<String>,
    requested_thread_reply_targets: HashSet<String>,
    thread_replies_inflight: bool,
    deferred_outbound: VecDeque<OutboundMessage>,
    seed_contacts: HashMap<String, Vec<String>>,
    author_relay_lists: HashMap<String, AuthorRelayList>,
    timeline_authors: BTreeSet<String>,
    profile_claims: HashMap<String, BTreeSet<String>>,
    requested_profiles: HashSet<String>,
    pending_profiles: BTreeSet<String>,
    profile_req_seq: u64,
    timeline_requested: bool,
    contacts_deadline: Option<Instant>,
    wire_subs: HashMap<String, WireSub>,
    last_emitted_items: Vec<TimelineItem>,
    update_sequence: u64,
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
    /// T82 — discovery wire-sub-id → [`crate::subs::OneshotToken`] map so the
    /// EOSE handler can route a completed oneshot back to its token for
    /// release. Entries are removed on completion (bounded by in-flight set).
    oneshot_subs: HashMap<String, crate::subs::OneshotToken>,
    /// M6 signer injection: actor / iOS layer wires this from
    /// `nmp_signers::AccountManager::signer_active()` at startup. `None`
    /// means no active account — AUTH challenges are recorded but no
    /// kind:22242 dispatch is attempted (the driver stays in
    /// `ChallengeReceived` until the signer is bound).
    auth_signer: Option<AuthSignerFn>,
    /// Hex pubkey of the active signer. Bound alongside `auth_signer`; used
    /// as the `pubkey` field of the unsigned kind:22242 template (NIP-42
    /// requires the AUTH event to be signed by the connecting client's key).
    auth_signer_pubkey: Option<String>,
    /// T66a identity/publish projections — flat wire-protocol summaries the
    /// actor pushes after each AccountManager-equivalent mutation. The actor
    /// (in `nmp-core`, so it CANNOT import `nmp-signers` per D0) owns the
    /// authoritative `nostr::Keys` map; these are the derived snapshot cache.
    accounts: Vec<AccountSummary>,
    active_account: Option<String>,
    publish_queue: Vec<PublishQueueEntry>,
    last_error_toast: Option<String>,
    relay_edit_rows: Vec<RelayEditRow>,
    wallet_status: Option<WalletStatus>,
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
    /// T114b — bounded-actor-channel drop counter (the same `Arc<AtomicU64>`
    /// owned by the FFI forwarder in `actor/mod.rs`). `None` when the kernel
    /// is constructed outside the actor (tests, codegen); the snapshot then
    /// reports `dispatch_drops_total = 0`. Surfaced on the snapshot via
    /// [`Metrics::dispatch_drops_total`].
    dispatch_drops: Option<Arc<AtomicU64>>,
}

impl Kernel {
    pub(crate) fn new(visible_limit: usize) -> Self {
        Self::with_publish_store(
            visible_limit,
            Arc::new(crate::publish::InMemoryPublishStore::new()),
        )
    }

    /// Construct a Kernel with an externally-supplied publish store. Used by
    /// integration tests that need two kernel instances to share one store
    /// (proving `PublishEngine::resume_from_store` survives a "restart"). The
    /// publish engine is built against this store + the kernel's NIP-65
    /// outbox resolver + a `QueueDispatcher` shared with the kernel for
    /// frame drainage.
    pub(crate) fn with_publish_store(
        visible_limit: usize,
        publish_store: Arc<dyn crate::publish::PublishStore>,
    ) -> Self {
        let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
        let publish_dispatcher = Arc::new(crate::publish::QueueDispatcher::new());
        let publish_engine = publish_engine::build_engine(
            Arc::clone(&store),
            Arc::clone(&publish_dispatcher),
            Arc::clone(&publish_store),
        );
        Self {
            store,
            rev: 0,
            visible_limit,
            started_at: None,
            last_event_at: None,
            first_event_at: None,
            target_profile_loaded_at: None,
            timeline_opened_at: None,
            timeline_first_item_at: None,
            relays: RelayRole::all()
                .into_iter()
                .map(|role| (role, RelayHealth::default()))
                .collect(),
            profiles: HashMap::new(),
            events: HashMap::new(),
            timeline: VecDeque::new(),
            selected_author: None,
            author_request_pending: false,
            author_view_seq: 0,
            selected_thread: None,
            thread_request_pending: false,
            thread_view_seq: 0,
            diagnostic_firehose: None,
            diagnostic_firehose_seq: 0,
            diagnostic_firehose_events: 0,
            pending_thread_ids: BTreeSet::new(),
            requested_thread_ids: HashSet::new(),
            thread_ids_inflight: false,
            pending_thread_reply_targets: BTreeSet::new(),
            requested_thread_reply_targets: HashSet::new(),
            thread_replies_inflight: false,
            deferred_outbound: VecDeque::new(),
            seed_contacts: HashMap::new(),
            author_relay_lists: HashMap::new(),
            timeline_authors: BTreeSet::new(),
            profile_claims: HashMap::new(),
            requested_profiles: HashSet::new(),
            pending_profiles: BTreeSet::new(),
            profile_req_seq: 0,
            timeline_requested: false,
            contacts_deadline: None,
            wire_subs: HashMap::new(),
            last_emitted_items: Vec::new(),
            update_sequence: 0,
            events_since_last_update: 0,
            max_event_to_emit_ms: 0,
            max_events_per_update: 0,
            changed_since_emit: true,
            logs: VecDeque::new(),
            nip42_drivers: RelayRole::all()
                .into_iter()
                .map(|role| (role, Nip42DriverState::new()))
                .collect(),
            lifecycle: SubscriptionLifecycle::new(),
            unknown_ids: UnknownIds::new(),
            oneshot: OneshotApi::new(),
            oneshot_subs: HashMap::new(),
            auth_signer: None,
            auth_signer_pubkey: None,
            accounts: Vec::new(),
            active_account: None,
            publish_queue: Vec::new(),
            last_error_toast: None,
            relay_edit_rows: Vec::new(),
            wallet_status: None,
            publish_engine,
            publish_dispatcher,
            publish_store,
            event_provenance: provenance::EventProvenance::new(),
            claim_drops_total: 0,
            dispatch_drops: None,
        }
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

    /// T114b — number of FFI dispatches dropped by the bounded actor channel
    /// (`BOUNDED_ACTOR_CMD_CAPACITY` overflow). Returns 0 when the kernel was
    /// constructed outside the actor (tests, codegen) and no handle is bound.
    pub(crate) fn dispatch_drops_total(&self) -> u64 {
        self.dispatch_drops
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
            .unwrap_or(0)
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
        self.wire_subs.len()
    }

    /// Bind a signer callback used by the NIP-42 handshake, with the active
    /// pubkey hex. The actor (or iOS layer) adapts
    /// `nmp_signers::AccountManager::signer_active()` to this signature at
    /// startup — keeping the kernel free of any `nmp-signers` dependency
    /// (no cycle). Replaces any previously-bound signer. The FFI bridge that
    /// surfaces this from Swift is T59 (filed in `docs/perf/pending-user-decisions.md`).
    pub(crate) fn bind_auth_signer(&mut self, pubkey_hex: String, signer: AuthSignerFn) {
        self.auth_signer = Some(signer);
        self.auth_signer_pubkey = Some(pubkey_hex);
    }

    /// Drop the active signer + pubkey (no active account). AUTH challenges
    /// are then recorded but never answered until a signer is rebound.
    pub(crate) fn clear_auth_signer(&mut self) {
        self.auth_signer = None;
        self.auth_signer_pubkey = None;
    }

    pub(crate) fn start(&mut self) {
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
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

    /// Mutable access to the subscription lifecycle (registry + trigger inbox).
    ///
    /// The actor-side `KernelAction` reducer (T95) uses this to register the
    /// `LogicalInterest` resolved from an `OpenUri` action through the
    /// single-writer [`crate::subs::InterestRegistry`] (D4). Kept crate-private
    /// so the FFI surface never sees a subscription-internal type (D0/D6).
    pub(crate) fn lifecycle_mut(&mut self) -> &mut SubscriptionLifecycle {
        &mut self.lifecycle
    }
}
