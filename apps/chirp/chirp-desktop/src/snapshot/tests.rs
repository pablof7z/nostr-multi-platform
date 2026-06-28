//! Deserialisation regression tests for the desktop snapshot mirrors.
//!
//! Split out of `snapshot.rs` so the data-type module stays under the 500-LOC
//! hard ceiling (AGENTS.md). Included via `#[path]` from `snapshot.rs`.

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

/// The desktop `ProfileCard` mirror (now populated from `refs.profile` via
/// `RefProfileStore`, ADR-0063) deserialises a `pubkey -> ProfileCard` map so
/// the Home tab can resolve display names.
#[test]
fn profile_card_map_decodes_profile_cards() {
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
        serde_json::from_value(json).expect("refs_profiles map deserializes");
    assert_eq!(
        map.get("deadbeef").and_then(|p| p.display_name.as_deref()),
        Some("Alice")
    );
}
