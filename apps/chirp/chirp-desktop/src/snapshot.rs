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

use crate::relation_counts::RelationCounts;

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
    pub active_account: Option<String>,
    #[serde(default)]
    pub accounts: Vec<AccountSummary>,

    /// Host-registered and built-in projections (`nmp.feed.*`,
    /// configured_relays, action_lifecycle, ...).
    #[serde(default)]
    pub projections: HashMap<String, serde_json::Value>,

    /// Pre-resolved embed map (issue #1283 Phase 1), keyed by `primary_id`.
    /// Decoded from the typed `refs.event.envelopes` (`NEMB`) sidecar in
    /// `snapshot_decode::decode_snapshot_typed` — desktop is a typed-frame shell
    /// (no JSON `payload`), so it consumes the SAME typed sidecar Chirp iOS does.
    /// `#[serde(default)]`: never present in the JSON envelope; the typed decode
    /// populates it. `render::note_body` looks an `EventRef` up here by
    /// `primary_id` to render the embedded note instead of a `↗ note` placeholder.
    #[serde(default)]
    pub embeds: HashMap<String, nmp_content::EmbeddedEventEnvelope>,

    /// ADR-0063 (#1671 Lane F) — `pubkey -> ProfileCard` materialised from the
    /// kernel's `refs.profile` row-delta projection (the resolve_ref output),
    /// merged into a persistent [`nmp_core::refs::RefProfileStore`] held by the
    /// reader thread. Replaces the old `resolved_profiles` projection read for
    /// avatar/name display. `#[serde(default)]`: never in the JSON envelope; the
    /// reader thread sets it after merging the row-delta into the store.
    #[serde(default)]
    pub refs_profiles: HashMap<String, ProfileCard>,
}

impl Snapshot {
    /// Pull a typed projection out of the host-extensible map.
    pub fn projection<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.projections
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

// ---------------------------------------------------------------------------
// Built-in kernel fields (mirrors from nmp-core::kernel::types)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProfileCard {
    #[serde(default)]
    pub pubkey: String,
    #[serde(default)]
    pub npub: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub raw_display_name: Option<String>,
    #[serde(default)]
    pub display_name_camel: Option<String>,
    #[serde(default)]
    pub picture_url: Option<String>,
    #[serde(default)]
    pub banner: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub nip05: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub lud16: Option<String>,
    #[serde(default)]
    pub lud06: Option<String>,
    #[serde(default)]
    pub lnurl: Option<String>,
}

impl ProfileCard {
    /// ADR-0063 (#1671 Lane F) — build a desktop `ProfileCard` from the typed
    /// `refs.profile` row (`nmp_core`'s `ProfileCardModel`). Field-identical
    /// mapping; the desktop owns no second profile representation (D4).
    #[must_use]
    pub fn from_model(model: nmp_core::typed_projections::ProfileCardModel) -> Self {
        Self {
            npub: nmp_core::display::to_npub(&model.pubkey),
            pubkey: model.pubkey,
            display_name: model.display_name,
            name: model.name,
            raw_display_name: model.raw_display_name,
            display_name_camel: model.display_name_camel,
            picture_url: model.picture_url,
            banner: model.banner,
            website: model.website,
            nip05: model.nip05,
            about: model.about,
            lud16: model.lud16,
            lud06: model.lud06,
            lnurl: model.lnurl,
        }
    }
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

// V-112 (ADR-0042): AuthorViewPayload, ThreadViewPayload, ProfileAction,
// ProfileDispatchSpec deleted — the author_view / thread_view kernel projections
// are removed.  Author and thread screens now read from the dynamic flat-feed
// projections "nmp.feed.author.<pubkey>" / "nmp.feed.thread.<event_id>"
// (ModularTimelineSnapshot).

/// Payload for the former `mention_profiles` projection (retired in ADR-0063
/// Lane H; profiles now flow through the `refs.profile` row-delta).
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

/// `configured_relays` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AppRelay {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub role: String,
}

/// `nmp.feed.home` OP-centric home-feed projection (simplified mirror).
///
/// The kernel ships this projection as the typed `OpFeedSnapshot`
/// (`nmp_feed::RootFeedSnapshot<TimelineEventCard, …>`): a `cards` array whose
/// every entry is a `RootCard` wrapper — `{ "card": <event card>,
/// "attribution": [...] }` — not a bare event card. We mirror only the
/// `card` payload the desktop renders; the `attribution` list (reply
/// provenance) and paging envelope are ignored. Every field is
/// `#[serde(default)]` so a forward-compatible kernel never breaks the shell.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ModularTimelineSnapshot {
    #[serde(default)]
    pub cards: Vec<RootCard>,
    #[serde(default)]
    pub page: Option<FeedPage>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FeedPage {
    #[serde(default)]
    pub has_more: bool,
}

/// One entry in the `nmp.feed.home` `cards` array — the `RootCard` wrapper
/// (`nmp_feed::RootCard`). The desktop only reads the inner render card.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RootCard {
    #[serde(default)]
    pub card: TimelineEventCard,
}

/// `nmp.follow_list` projection payload.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct FollowListSnapshot {
    #[serde(default)]
    pub follows: Vec<FollowEntry>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct FollowEntry {
    #[serde(default)]
    pub pubkey: String,
}

/// Desktop-local mirror of `nmp_nip01::TimelineEventCard` (post-#922 shape).
///
/// Raw protocol data only — `author_pubkey` as hex, `created_at` as Unix
/// seconds, `content` verbatim. The presentation layer resolves the display
/// name via the snapshot's `refs_profiles` map (populated by `resolve_ref`,
/// ADR-0063 #1671 Lane F; aim.md §2). We keep the
/// mirror desktop-local (rather than importing the nmp-nip01 type) so the
/// shell's decode surface stays decoupled from the projection's internal
/// type. `content_tree` is omitted — the desktop renders rich text from
/// `content`. Every field is `#[serde(default)]`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TimelineEventCard {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub author_pubkey: String,
    #[serde(default)]
    pub kind: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub relation_counts: RelationCounts,
    /// Post-#922 cards carry no relay-count field; kept for forward
    /// compatibility, defaults to 0 (the relay-multiplier badge never shows).
    #[serde(default)]
    pub relay_count: u32,
    #[serde(default)]
    pub relay_provenance: Vec<String>,
    /// `Some` when this card surfaced because a NIP-18 repost superseded the
    /// original note. `author_pubkey` / `content` name the *original* note;
    /// this names the reposter.
    #[serde(default)]
    pub reposted_by: Option<RepostAttribution>,
}

/// Attribution payload for a repost-surfaced card (mirror of
/// `nmp_nip01::RepostAttribution`). The reposter's raw hex pubkey and the
/// original note's publish time.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RepostAttribution {
    #[serde(default)]
    pub author_pubkey: String,
    #[serde(default)]
    pub note_created_at: u64,
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

/// `bunker_handshake` projection payload — NIP-46 connect-QR progress.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BunkerHandshakeStatus {
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub is_in_flight: bool,
    #[serde(default)]
    pub is_terminal_success: bool,
    #[serde(default)]
    pub is_failed: bool,
    #[serde(default)]
    pub can_cancel: bool,
    #[serde(default)]
    pub message: Option<String>,
}

/// `signer_state` projection payload — unified remote-signer health status.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct SignerStatus {
    #[serde(default)]
    pub signer_kind: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub is_ready: bool,
    #[serde(default)]
    pub is_failed: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

/// `action_stages` projection payload — publish lifecycle rows.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ActionStageRow {
    #[serde(default)]
    pub correlation_id: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub reason: Option<String>,
}

// Deserialisation regression tests live in the sibling `snapshot/tests.rs`
// (kept out of this file so the data-type module stays under the 500-LOC hard
// ceiling, AGENTS.md).
#[cfg(test)]
#[path = "snapshot/tests.rs"]
mod tests;
