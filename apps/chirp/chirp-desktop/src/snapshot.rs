//! Read-only mirror of the kernel's JSON `KernelUpdate` envelope.
//!
//! Doctrine: the UI owns *no* state beyond the latest snapshot. These
//! structs are a deserialization-only projection of the actor's emitted
//! JSON. Every field is `#[serde(default)]` so a forward-compatible kernel
//! that adds/removes fields never breaks the shell — best-effort rendering.
//!
//! Per aim.md §2, the kernel snapshot ships raw protocol data — pubkeys
//! as hex, timestamps as Unix `u64`, display names as `Option<String>`.
//! This shell is the presentation layer: it formats raw fields itself at
//! render time.

use std::collections::HashMap;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Top-level snapshot
// ---------------------------------------------------------------------------

/// The latest decoded snapshot. Held behind a mutex and swapped wholesale on
/// every actor emit — the shell never mutates it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub rev: u64,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub last_error_toast: Option<String>,
    #[serde(default)]
    pub relay_statuses: Vec<RelayStatus>,
    #[serde(default)]
    pub metrics: Metrics,
    #[serde(default)]
    pub profile: ProfileCard,
    #[serde(default)]
    pub items: Vec<TimelineItem>,
    #[serde(default)]
    pub active_account: Option<String>,
    #[serde(default)]
    pub accounts: Vec<AccountSummary>,

    /// Host-registered and built-in projections (thread_view, author_view,
    /// nmp.feed.home, relay_edit_rows, action_lifecycle, mention_profiles, …).
    #[serde(default)]
    pub projections: HashMap<String, serde_json::Value>,
}

impl Snapshot {
    /// Pull a typed projection out of the host-extensible map.
    pub fn projection<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.projections.get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

// ---------------------------------------------------------------------------
// Built-in kernel fields (mirrors from nmp-core::kernel::types)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct TimelineItem {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author_pubkey: String,
    #[serde(default)]
    pub author_picture_url: Option<String>,
    #[serde(default)]
    pub kind: u32,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub content_preview: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub relay_count: u32,
    #[serde(default)]
    pub is_repost: bool,
    #[serde(default)]
    pub nav_target_id: String,
    #[serde(default)]
    pub repost_inner_content: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileCard {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub npub: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    #[serde(default)]
    pub nip05: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub has_profile: bool,
    #[serde(default)]
    pub lnurl: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Metrics {
    #[serde(default)]
    pub note_events: u64,
    #[serde(default)]
    pub events_rx: u64,
    #[serde(default)]
    pub visible_items: usize,
    #[serde(default)]
    pub events_since_last_update: u64,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelayStatus {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub relay_url: String,
    #[serde(default)]
    pub connection: String,
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub events_rx: u64,
    #[serde(default)]
    pub denied: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct AccountSummary {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    #[serde(default)]
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// Projections (deserialized from the `projections` map)
// ---------------------------------------------------------------------------

/// `author_view` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AuthorViewPayload {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub profile: ProfileCard,
    #[serde(default)]
    pub items: Vec<TimelineItem>,
    #[serde(default)]
    pub note_count: usize,
    #[serde(default)]
    pub note_count_display: String,
    #[serde(default)]
    pub primary_action: Option<ProfileAction>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileAction {
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub target_pubkey: String,
    #[serde(default)]
    pub icon_name: String,
    #[serde(default)]
    pub dispatch: Option<ProfileDispatchSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileDispatchSpec {
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub body_json: String,
}

/// `thread_view` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ThreadViewPayload {
    #[serde(default)]
    pub focused_event_id: String,
    #[serde(default)]
    pub root_event_id: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub items: Vec<TimelineItem>,
    #[serde(default)]
    pub previous_count: usize,
    #[serde(default)]
    pub next_count: usize,
    #[serde(default)]
    pub previous_count_label: String,
    #[serde(default)]
    pub next_count_label: String,
}

/// `mention_profiles` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct MentionProfilePayload {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
}

/// `action_lifecycle` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct LifecycleSnapshot {
    #[serde(default)]
    pub in_flight: Vec<LifecycleEntry>,
    #[serde(default)]
    pub recent_terminal: Vec<LifecycleEntry>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct LifecycleEntry {
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub terminal: bool,
}

/// `relay_edit_rows` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RelayEditRow {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub role_label: String,
    #[serde(default)]
    pub role_tint: String,
}

/// `nmp.feed.home` modular timeline projection (simplified).
///
/// We mirror only the fields the desktop shell needs; the full type lives in
/// `nmp-nip01` but is not directly accessible without adding a dependency.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ModularTimelineSnapshot {
    #[serde(default)]
    pub blocks: Vec<serde_json::Value>,
    #[serde(default)]
    pub cards: Vec<serde_json::Value>,
}

/// `nmp.nip17.dm_inbox` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct DmConversationSnapshot {
    #[serde(default)]
    pub conversations: Vec<DmConversation>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DmConversation {
    #[serde(default)]
    pub peer_pubkey: String,
    #[serde(default)]
    pub peer_display: String,
    #[serde(default)]
    pub messages: Vec<DmMessage>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct DmMessage {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub outgoing: bool,
}
