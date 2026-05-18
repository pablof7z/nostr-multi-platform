//! Pure data types shared across kernel sub-modules.
//!
//! Holds all struct/enum definitions with no behaviour of their own: StoredEvent,
//! Profile, TimelineItem, ProfileCard, view payloads, relay health/status, wire
//! subscription state, counters, and the AuthorRelayList cache entry.

use super::*;

// ── Seed accounts ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub(super) struct SeedAccount {
    pub(super) name: &'static str,
    pub(super) pubkey: &'static str,
}

pub(super) fn seed_accounts() -> Vec<SeedAccount> {
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

// ── Event read-cache ──────────────────────────────────────────────────────────

/// Lightweight read-cache entry for timeline ordering and display.
///
/// The `EventStore` is the single authoritative writer (D4).  This struct is
/// populated **only** after `EventStore::insert` returns `Inserted | Replaced`.
#[derive(Clone, Debug)]
pub(super) struct StoredEvent {
    pub(super) id: String,
    pub(super) author: String,
    pub(super) kind: u32,
    pub(super) created_at: u64,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    pub(super) relay_count: u32,
}

// ── Profile cache ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub(super) struct Profile {
    pub(super) event_id: String,
    pub(super) created_at: u64,
    pub(super) display: String,
    pub(super) picture_url: Option<String>,
    pub(super) nip05: String,
    pub(super) about: String,
    pub(super) avatar_initials: String,
    pub(super) avatar_color: String,
}

// ── Timeline and view payloads ────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub(super) struct TimelineItem {
    pub(super) id: String,
    pub(super) author_pubkey: String,
    pub(super) author_display: String,
    pub(super) author_picture_url: Option<String>,
    pub(super) author_avatar_initials: String,
    pub(super) author_avatar_color: String,
    pub(super) author_avatar_source: String,
    pub(super) content: String,
    pub(super) content_preview: String,
    pub(super) created_at_display: String,
    pub(super) relay_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ProfileCard {
    pub(super) pubkey: String,
    pub(super) npub: String,
    pub(super) display: String,
    pub(super) picture_url: Option<String>,
    pub(super) nip05: String,
    pub(super) about: String,
    pub(super) avatar_initials: String,
    pub(super) avatar_color: String,
    pub(super) source: String,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct AuthorViewPayload {
    pub(super) pubkey: String,
    pub(super) state: String,
    pub(super) profile: ProfileCard,
    pub(super) items: Vec<TimelineItem>,
    pub(super) note_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct ThreadViewPayload {
    pub(super) focused_event_id: String,
    pub(super) root_event_id: String,
    pub(super) state: String,
    pub(super) items: Vec<TimelineItem>,
    pub(super) previous_count: usize,
    pub(super) next_count: usize,
}

// ── Relay health and wire subscription state ──────────────────────────────────
#[derive(Clone, Debug, Serialize)]
pub(super) struct RelayStatus {
    pub(super) role: String,
    pub(super) relay_url: String,
    pub(super) connection: String,
    pub(super) auth: String,
    pub(super) nip77_negentropy: String,
    pub(super) active_wire_subscriptions: usize,
    pub(super) reconnect_count: u32,
    pub(super) last_connected_at_ms: Option<u128>,
    pub(super) last_event_at_ms: Option<u128>,
    pub(super) last_notice: Option<String>,
    pub(super) last_error: Option<String>,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct WireSubscriptionStatus {
    pub(super) wire_id: String,
    pub(super) relay_url: String,
    pub(super) filter_summary: String,
    pub(super) state: String,
    pub(super) logical_consumer_count: u32,
    pub(super) opened_at_ms: u128,
    pub(super) last_event_at_ms: Option<u128>,
    pub(super) eose_at_ms: Option<u128>,
    pub(super) close_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct LogicalInterestStatus {
    pub(super) key: String,
    pub(super) state: String,
    pub(super) refcount: u32,
    pub(super) relay_urls: Vec<String>,
    pub(super) cache_coverage: String,
    pub(super) warming_until_ms: Option<u128>,
}

/// Per-relay rolling counters for diagnostics.
#[derive(Clone, Debug, Default)]
pub(super) struct Counters {
    pub(super) frames_rx: u64,
    pub(super) events_rx: u64,
    pub(super) eose_rx: u64,
    pub(super) notices_rx: u64,
    pub(super) closed_rx: u64,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
}

/// Active wire (WebSocket) subscription state.
pub(super) struct WireSub {
    pub(super) id: String,
    pub(super) role: RelayRole,
    pub(super) filter_summary: String,
    pub(super) state: String,
    pub(super) opened_at: Instant,
    pub(super) last_event_at: Option<Instant>,
    pub(super) eose_at: Option<Instant>,
    pub(super) close_reason: Option<String>,
}

/// Per-relay health state: connection status, timestamps, and counters.
#[derive(Clone, Debug)]
pub(super) struct RelayHealth {
    pub(super) connection: String,
    pub(super) connected_at: Option<Instant>,
    pub(super) last_event_at: Option<Instant>,
    pub(super) last_notice: Option<String>,
    pub(super) last_error: Option<String>,
    pub(super) reconnect_count: u32,
    pub(super) counters: Counters,
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

// ── NIP-65 relay list cache ───────────────────────────────────────────────────

/// Cached kind:10002 relay list for an author.
///
/// `event_id` is used as a tiebreak when two events share the same `created_at`:
/// lexicographically smaller event id wins, mirroring the store's supersession
/// logic.
#[derive(Clone, Debug, Default)]
pub(super) struct AuthorRelayList {
    /// Event id of the kind:10002 that produced this relay list.
    pub(super) event_id: String,
    pub(super) created_at: u64,
    pub(super) read_relays: Vec<String>,
    pub(super) write_relays: Vec<String>,
    pub(super) both_relays: Vec<String>,
}

// ── View interest (refcounted) ────────────────────────────────────────────────
/// Tracks an open view (author, thread, firehose) with a refcount.
///
/// Refcounting allows multiple SwiftUI view instances to share the same relay
/// subscription.  The subscription is closed only when the last claimant calls
/// `close_*`.
#[derive(Clone, Debug)]
pub(super) struct ViewInterest {
    pub(super) key: String,
    pub(super) refcount: u32,
}

// ── Metrics snapshot ──────────────────────────────────────────────────────────
#[derive(Clone, Debug, Serialize)]
pub(super) struct Metrics {
    pub(super) generated_events: u64,
    pub(super) note_events: u64,
    pub(super) profile_events: u64,
    pub(super) duplicate_events: u64,
    pub(super) delete_events: u64,
    pub(super) stored_events: usize,
    pub(super) tombstones: usize,
    pub(super) visible_items: usize,
    pub(super) visible_profiled_items: usize,
    pub(super) visible_placeholder_avatar_items: usize,
    pub(super) open_views: u32,
    pub(super) events_since_last_update: u64,
    pub(super) diagnostic_firehose_events: u64,
    pub(super) inserted_count: usize,
    pub(super) updated_count: usize,
    pub(super) removed_count: usize,
    pub(super) events_per_second_configured: u32,
    pub(super) emit_hz_configured: u32,
    pub(super) update_sequence: u64,
    pub(super) estimated_store_bytes: usize,
    pub(super) payload_bytes: usize,
    pub(super) store_to_payload_ratio: f64,
    pub(super) actor_queue_depth: u32,
    pub(super) frames_rx: u64,
    pub(super) events_rx: u64,
    pub(super) eose_rx: u64,
    pub(super) notices_rx: u64,
    pub(super) closed_rx: u64,
    pub(super) bytes_rx: u64,
    pub(super) bytes_tx: u64,
    pub(super) contacts_authors: usize,
    pub(super) timeline_authors: usize,
    pub(super) first_event_ms: Option<u128>,
    pub(super) target_profile_loaded_ms: Option<u128>,
    pub(super) timeline_opened_ms: Option<u128>,
    pub(super) timeline_first_item_ms: Option<u128>,
    pub(super) update_emitted_ms: Option<u128>,
    pub(super) last_event_to_emit_ms: Option<u128>,
    pub(super) max_event_to_emit_ms: u128,
    pub(super) max_events_per_update: u64,
}

// ── Update envelope ───────────────────────────────────────────────────────────
#[derive(Clone, Debug, Serialize)]
pub(super) struct KernelUpdate {
    pub(super) rev: u64,
    pub(super) update_kind: &'static str,
    pub(super) running: bool,
    pub(super) relay_url: &'static str,
    pub(super) test_npub: &'static str,
    pub(super) profile: ProfileCard,
    pub(super) items: Vec<TimelineItem>,
    pub(super) author_view: Option<AuthorViewPayload>,
    pub(super) thread_view: Option<ThreadViewPayload>,
    pub(super) inserted: Vec<TimelineItem>,
    pub(super) updated: Vec<TimelineItem>,
    pub(super) removed: Vec<String>,
    pub(super) metrics: Metrics,
    pub(super) relay_status: RelayStatus,
    pub(super) relay_statuses: Vec<RelayStatus>,
    pub(super) logical_interests: Vec<LogicalInterestStatus>,
    pub(super) wire_subscriptions: Vec<WireSubscriptionStatus>,
    pub(super) logs: Vec<String>,
}
