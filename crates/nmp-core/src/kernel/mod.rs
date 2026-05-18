mod ingest;
mod nostr;
mod requests;
mod status;
#[cfg(test)]
mod tests;
mod update;

use crate::relay::{
    OutboundMessage, RelayRole, CONTENT_RELAY_URL, DEFAULT_EMIT_HZ, FIATJAF_PUBKEY,
    INDEXER_RELAY_URL, JB55_PUBKEY, TEST_NPUB, TEST_PUBKEY, TIMELINE_AUTHOR_LIMIT,
};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::Message;

use nostr::*;
pub(crate) use nostr::{is_hex_id, is_hex_pubkey};

use crate::store::{EventStore, MemEventStore};

#[derive(Clone)]
struct SeedAccount {
    name: &'static str,
    pubkey: &'static str,
}

fn seed_accounts() -> Vec<SeedAccount> {
    vec![
        SeedAccount {
            name: "pablof7z",
            pubkey: TEST_PUBKEY,
        },
        SeedAccount {
            name: "fiatjaf",
            pubkey: FIATJAF_PUBKEY,
        },
        SeedAccount {
            name: "jb55",
            pubkey: JB55_PUBKEY,
        },
    ]
}

#[derive(Clone, Debug)]
struct StoredEvent {
    id: String,
    author: String,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
    content: String,
    relay_count: u32,
}

#[derive(Clone, Debug, Default)]
struct Profile {
    event_id: String,
    created_at: u64,
    display: String,
    picture_url: Option<String>,
    nip05: String,
    about: String,
    avatar_initials: String,
    avatar_color: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
struct TimelineItem {
    id: String,
    author_pubkey: String,
    author_display: String,
    author_picture_url: Option<String>,
    author_avatar_initials: String,
    author_avatar_color: String,
    author_avatar_source: String,
    content: String,
    content_preview: String,
    created_at_display: String,
    relay_count: u32,
}

#[derive(Clone, Debug, Serialize)]
struct ProfileCard {
    pubkey: String,
    npub: String,
    display: String,
    picture_url: Option<String>,
    nip05: String,
    about: String,
    avatar_initials: String,
    avatar_color: String,
    source: String,
}

#[derive(Clone, Debug, Serialize)]
struct AuthorViewPayload {
    pubkey: String,
    state: String,
    profile: ProfileCard,
    items: Vec<TimelineItem>,
    note_count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct ThreadViewPayload {
    focused_event_id: String,
    root_event_id: String,
    state: String,
    items: Vec<TimelineItem>,
    previous_count: usize,
    next_count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct RelayStatus {
    role: String,
    relay_url: String,
    connection: String,
    auth: String,
    nip77_negentropy: String,
    active_wire_subscriptions: usize,
    reconnect_count: u32,
    last_connected_at_ms: Option<u128>,
    last_event_at_ms: Option<u128>,
    last_notice: Option<String>,
    last_error: Option<String>,
    bytes_rx: u64,
    bytes_tx: u64,
}

#[derive(Clone, Debug, Serialize)]
struct WireSubscriptionStatus {
    wire_id: String,
    relay_url: String,
    filter_summary: String,
    state: String,
    logical_consumer_count: u32,
    opened_at_ms: u128,
    last_event_at_ms: Option<u128>,
    eose_at_ms: Option<u128>,
    close_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct LogicalInterestStatus {
    key: String,
    state: String,
    refcount: u32,
    relay_urls: Vec<String>,
    cache_coverage: String,
    warming_until_ms: Option<u128>,
}

#[derive(Clone, Debug, Serialize)]
struct Metrics {
    generated_events: u64,
    note_events: u64,
    profile_events: u64,
    duplicate_events: u64,
    delete_events: u64,
    stored_events: usize,
    tombstones: usize,
    visible_items: usize,
    visible_profiled_items: usize,
    visible_placeholder_avatar_items: usize,
    open_views: u32,
    events_since_last_update: u64,
    diagnostic_firehose_events: u64,
    inserted_count: usize,
    updated_count: usize,
    removed_count: usize,
    events_per_second_configured: u32,
    emit_hz_configured: u32,
    update_sequence: u64,
    estimated_store_bytes: usize,
    payload_bytes: usize,
    store_to_payload_ratio: f64,
    actor_queue_depth: u32,
    frames_rx: u64,
    events_rx: u64,
    eose_rx: u64,
    notices_rx: u64,
    closed_rx: u64,
    bytes_rx: u64,
    bytes_tx: u64,
    contacts_authors: usize,
    timeline_authors: usize,
    first_event_ms: Option<u128>,
    target_profile_loaded_ms: Option<u128>,
    timeline_opened_ms: Option<u128>,
    timeline_first_item_ms: Option<u128>,
    update_emitted_ms: Option<u128>,
    last_event_to_emit_ms: Option<u128>,
    max_event_to_emit_ms: u128,
    max_events_per_update: u64,
}

#[derive(Clone, Debug, Serialize)]
struct KernelUpdate {
    rev: u64,
    update_kind: &'static str,
    running: bool,
    relay_url: &'static str,
    test_npub: &'static str,
    profile: ProfileCard,
    items: Vec<TimelineItem>,
    author_view: Option<AuthorViewPayload>,
    thread_view: Option<ThreadViewPayload>,
    inserted: Vec<TimelineItem>,
    updated: Vec<TimelineItem>,
    removed: Vec<String>,
    metrics: Metrics,
    relay_status: RelayStatus,
    relay_statuses: Vec<RelayStatus>,
    logical_interests: Vec<LogicalInterestStatus>,
    wire_subscriptions: Vec<WireSubscriptionStatus>,
    logs: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct Counters {
    frames_rx: u64,
    events_rx: u64,
    eose_rx: u64,
    notices_rx: u64,
    closed_rx: u64,
    bytes_rx: u64,
    bytes_tx: u64,
}

struct WireSub {
    id: String,
    role: RelayRole,
    filter_summary: String,
    state: String,
    opened_at: Instant,
    last_event_at: Option<Instant>,
    eose_at: Option<Instant>,
    close_reason: Option<String>,
}

#[derive(Clone, Debug)]
struct RelayHealth {
    connection: String,
    connected_at: Option<Instant>,
    last_event_at: Option<Instant>,
    last_notice: Option<String>,
    last_error: Option<String>,
    reconnect_count: u32,
    counters: Counters,
}

impl Default for RelayHealth {
    fn default() -> Self {
        Self {
            connection: "offline".to_string(),
            connected_at: None,
            last_event_at: None,
            last_notice: None,
            last_error: None,
            reconnect_count: 0,
            counters: Counters::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct AuthorRelayList {
    created_at: u64,
    read_relays: Vec<String>,
    write_relays: Vec<String>,
    both_relays: Vec<String>,
}

#[derive(Clone, Debug)]
struct ViewInterest {
    key: String,
    refcount: u32,
}

pub(crate) struct Kernel {
    /// Pluggable event store. Defaults to `MemEventStore`; will be replaced by
    /// `LmdbEventStore` once the full M3 LMDB integration is complete.
    ///
    /// The existing `events: HashMap<String, StoredEvent>` field is preserved
    /// for backward compatibility during the M3 migration. The store field is
    /// the target home for all event persistence after M3 completes.
    #[allow(dead_code)]
    store: Box<dyn EventStore>,
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
}

impl Kernel {
    pub(crate) fn new(visible_limit: usize) -> Self {
        Self {
            store: Box::new(MemEventStore::new()),
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
        }
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
}
