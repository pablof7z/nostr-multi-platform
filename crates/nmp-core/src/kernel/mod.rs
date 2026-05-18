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
mod publish_cmd;
mod requests;
mod status;
#[cfg(any(test, feature = "test-support"))]
mod test_support;
#[cfg(test)]
mod tests;
mod types;
mod update;

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
pub(crate) use identity_state::{AccountSummary, PublishQueueEntry, RelayEditRow};
use types::*;

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
}

impl Kernel {
    pub(crate) fn new(visible_limit: usize) -> Self {
        Self {
            store: Arc::new(MemEventStore::new()),
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
        }
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
