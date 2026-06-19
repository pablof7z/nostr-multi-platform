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

    /// Host-registered and built-in projections (thread_view, author_view,
    /// nmp.feed.home, configured_relays, action_lifecycle, mention_profiles, …).
    #[serde(default)]
    pub projections: HashMap<String, serde_json::Value>,

    /// Pre-resolved embed map (issue #1283 Phase 1), keyed by `primary_id`.
    /// Decoded from the typed `claimed_event_embeds` (`NEMB`) sidecar in
    /// `snapshot_decode::decode_snapshot_typed` — desktop is a typed-frame shell
    /// (no JSON `payload`), so it consumes the SAME typed sidecar Chirp iOS does.
    /// `#[serde(default)]`: never present in the JSON envelope; the typed decode
    /// populates it. `render::note_body` looks an `EventRef` up here by
    /// `primary_id` to render the embedded note instead of a `↗ note` placeholder.
    #[serde(default)]
    pub embeds: HashMap<String, nmp_content::EmbeddedEventEnvelope>,
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
    pub picture_url: Option<String>,
    #[serde(default)]
    pub nip05: String,
    #[serde(default)]
    pub about: String,
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

// V-112 (ADR-0042): AuthorViewPayload, ThreadViewPayload, ProfileAction,
// ProfileDispatchSpec deleted — the author_view / thread_view kernel projections
// are removed.  Author and thread screens now read from the dynamic flat-feed
// projections "nmp.feed.author.<pubkey>" / "nmp.feed.thread.<event_id>"
// (ModularTimelineSnapshot).

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
/// name via the snapshot's `resolved_profiles` map (aim.md §2). We keep the
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The `nmp.feed.home` projection (typed `OpFeedSnapshot`) ships its cards
    /// **wrapped** — each `cards[]` entry is a `RootCard` `{ "card": …,
    /// "attribution": [...] }`, not a bare event card. This is the exact shape
    /// `decode_op_feed_snapshot` → `serde_json::to_value` emits in the desktop
    /// bridge. Regression guard for issue #920: an earlier mirror assumed bare
    /// cards (`Vec<TimelineEventCard>`), which deserialized every entry to
    /// all-defaults and left the Home tab blank.
    #[test]
    fn home_feed_decodes_wrapped_root_cards() {
        let json = serde_json::json!({
            "cards": [
                {
                    "card": {
                        "id": "abc123",
                        "author_pubkey": "deadbeef",
                        "kind": 1,
                        "created_at": 1_700_000_000_u64,
                        "content": "hello nostr",
                        "content_tree": { "nodes": [] },
                        "relation_counts": {}
                    },
                    "attribution": []
                }
            ],
            "page": null,
            "metrics": null
        });

        let feed: ModularTimelineSnapshot =
            serde_json::from_value(json).expect("wrapped root cards deserialize");

        assert_eq!(feed.cards.len(), 1, "one root card");
        let card = &feed.cards[0].card;
        assert_eq!(card.id, "abc123");
        assert_eq!(card.author_pubkey, "deadbeef");
        assert_eq!(card.content, "hello nostr");
        assert_eq!(card.created_at, 1_700_000_000);
        assert_eq!(
            card.relation_counts.summary(),
            "reply ...  react ...  repost ...  zap ..."
        );
        assert!(card.reposted_by.is_none(), "ordinary note: no repost");
    }

    /// A repost-surfaced card carries `reposted_by` with the reposter's raw
    /// pubkey and the original note's publish time.
    #[test]
    fn home_feed_decodes_repost_attribution() {
        let json = serde_json::json!({
            "cards": [
                {
                    "card": {
                        "id": "note1",
                        "author_pubkey": "originalauthor",
                        "kind": 1,
                        "created_at": 1_700_000_500_u64,
                        "content": "the original note",
                        "reposted_by": {
                            "author_pubkey": "thereposter",
                            "note_created_at": 1_700_000_100_u64
                        }
                    },
                    "attribution": []
                }
            ]
        });

        let feed: ModularTimelineSnapshot =
            serde_json::from_value(json).expect("repost card deserializes");
        let repost = feed.cards[0]
            .card
            .reposted_by
            .as_ref()
            .expect("reposted_by present");
        assert_eq!(repost.author_pubkey, "thereposter");
        assert_eq!(repost.note_created_at, 1_700_000_100);
    }

    /// An empty feed (no cards yet — the "connecting" state) deserializes to an
    /// empty `cards` vec, never an error.
    #[test]
    fn empty_home_feed_is_empty_cards() {
        let json = serde_json::json!({ "cards": [], "page": null, "metrics": null });
        let feed: ModularTimelineSnapshot =
            serde_json::from_value(json).expect("empty feed deserializes");
        assert!(feed.cards.is_empty());
        assert!(feed.page.is_none());
    }

    #[test]
    fn home_feed_decodes_has_more_page_flag() {
        let json = serde_json::json!({
            "cards": [],
            "page": { "limit": 80, "has_more": true, "total_blocks": 120 }
        });

        let feed: ModularTimelineSnapshot =
            serde_json::from_value(json).expect("feed page deserializes");
        assert!(feed.page.as_ref().is_some_and(|page| page.has_more));
    }

    /// `resolved_profiles` decodes into the desktop's `ProfileCard` mirror,
    /// keyed by hex pubkey, so the Home tab can resolve display names.
    #[test]
    fn resolved_profiles_decodes_profile_cards() {
        let json = serde_json::json!({
            "deadbeef": {
                "pubkey": "deadbeef",
                "npub": "npub1deadbeef",
                "display_name": "Alice",
                "picture_url": null,
                "nip05": "",
                "about": "",
                "lnurl": null
            }
        });
        let map: std::collections::HashMap<String, ProfileCard> =
            serde_json::from_value(json).expect("resolved_profiles deserializes");
        assert_eq!(
            map.get("deadbeef").and_then(|p| p.display_name.as_deref()),
            Some("Alice")
        );
    }
}
