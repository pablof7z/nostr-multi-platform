//! Builder → fake-stored → `try_from_event` round trip. Confirms the
//! encode/decode pair preserves the target, content, emoji, and `k` kind with
//! no loss across all three builders.

mod common;

use common::stored;
use nmp_relations::{try_from_event, GenericRepost, Reaction, ReactionTarget, Repost, SocialKind};

const AUTHOR: &str = "author-0000000000000000000000000000000000000000000000000000000000";
const TARGET: &str = "target-0000000000000000000000000000000000000000000000000000000000";
const TARGET_AUTHOR: &str = "ta-00000000000000000000000000000000000000000000000000000000000000";

#[test]
fn reaction_round_trip_preserves_content_target_emoji() {
    let unsigned = Reaction::to_event(TARGET, TARGET_AUTHOR)
        .content(":soapbox:")
        .emoji("soapbox", "https://x/soapbox.png")
        .build(AUTHOR, 1_700_000_000)
        .expect("reaction builds");

    let event = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags.clone(),
        &unsigned.content,
    );

    let r = try_from_event(&event).expect("round-trips");
    assert_eq!(r.target, ReactionTarget::Event(TARGET.to_string()));
    assert_eq!(r.target_author.as_deref(), Some(TARGET_AUTHOR));
    match r.kind {
        SocialKind::Reaction { content, emoji } => {
            assert_eq!(content, ":soapbox:");
            let e = emoji.expect("emoji preserved");
            assert_eq!(e.shortcode, "soapbox");
            assert_eq!(e.url, "https://x/soapbox.png");
        }
        _ => panic!("expected Reaction"),
    }
}

#[test]
fn repost_round_trip_preserves_embedded_json() {
    let json = r#"{"id":"abc","kind":1,"content":"hi"}"#;
    let unsigned = Repost::of(TARGET, TARGET_AUTHOR)
        .embed(json)
        .build(AUTHOR, 0)
        .unwrap();
    let event = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );
    let r = try_from_event(&event).unwrap();
    match r.kind {
        SocialKind::Repost { embedded } => assert_eq!(embedded, json),
        _ => panic!("expected Repost"),
    }
}

#[test]
fn generic_repost_round_trip_preserves_original_kind() {
    let unsigned = GenericRepost::of(TARGET, TARGET_AUTHOR, 30023)
        .build(AUTHOR, 0)
        .unwrap();
    let event = stored(
        &"a".repeat(64),
        &unsigned.pubkey,
        unsigned.kind,
        unsigned.created_at,
        unsigned.tags,
        &unsigned.content,
    );
    let r = try_from_event(&event).unwrap();
    match r.kind {
        SocialKind::GenericRepost { original_kind, .. } => {
            assert_eq!(original_kind, Some(30023));
        }
        _ => panic!("expected GenericRepost"),
    }
}
