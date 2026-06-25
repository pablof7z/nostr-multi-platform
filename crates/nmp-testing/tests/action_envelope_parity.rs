//! Cross-platform action envelope parity tests.
//!
//! Calls the real production builder functions from `nmp_app_chirp::typed_api`
//! and asserts on the actual (namespace, JSON) output they produce.  These are
//! NOT tautological: the expected values were verified against the production
//! code in `apps/chirp/crates/nmp-app-chirp/src/typed_api.rs`.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test action_envelope_parity
//! ```

use nmp_app_chirp::typed_api::{
    follow_action, publish_note_action, react_action, send_dm_action, unfollow_action,
};
use nmp_core::tags::Nip10Refs;
use nmp_nip01::NoteRecord;

/// A minimal thread-root `NoteRecord` (no `Nip10Refs`), mirroring what a
/// chirp shell captures from a selected timeline row before replying.
fn parent_record(event_id: &str, author: &str) -> NoteRecord {
    NoteRecord {
        event_id: event_id.to_string(),
        author: author.to_string(),
        created_at: 0,
        content: "parent".to_string(),
        refs: Nip10Refs::default(),
    }
}

// ---------------------------------------------------------------------------
// publish_note_action
// ---------------------------------------------------------------------------

#[test]
fn publish_note_action_has_correct_namespace_and_content() {
    let (ns, json) = publish_note_action("hello world", None).unwrap();
    assert_eq!(ns, "nmp.publish");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // A note is a kind:1 PublishRaw event.
    assert_eq!(v["PublishRaw"]["kind"], 1);
    assert_eq!(v["PublishRaw"]["content"], "hello world");
    // Root note (no reply) carries no tags.
    assert_eq!(v["PublishRaw"]["tags"], serde_json::json!([]));
    // target is always present (string-form unit variant "Auto").
    assert!(v["PublishRaw"]["target"].is_string());
}

#[test]
fn publish_note_action_with_reply_has_full_nip10_tag_set() {
    // Reply to a thread-root note by alice. The canonical `nmp_nip01::Note`
    // builder emits the full NIP-10 set: marked-form root + reply `e` tags
    // (parent is its own root here) and a `p` re-notification for the author.
    let parent = parent_record("abc123", "alice");
    let (ns, json) = publish_note_action("reply", Some(&parent)).unwrap();
    assert_eq!(ns, "nmp.publish");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["PublishRaw"]["kind"], 1);
    assert_eq!(v["PublishRaw"]["content"], "reply");
    assert_eq!(
        v["PublishRaw"]["tags"],
        serde_json::json!([
            ["e", "abc123", "", "root"],
            ["e", "abc123", "", "reply"],
            ["p", "alice"]
        ])
    );
}

#[test]
fn publish_note_action_rejects_empty_content() {
    // The builder is fail-closed on empty content (D6): the error surfaces as
    // a string rather than a panic, and no envelope is produced.
    let err = publish_note_action("   ", None).unwrap_err();
    assert!(
        err.contains("non-empty content"),
        "unexpected error: {err}"
    );
}

/// Dispatch-correctness guard: the JSON the builder emits must deserialize
/// into the exact `PublishAction::PublishRaw` variant the kernel accepts.
/// Shape-only assertions above prove the key names; this proves the envelope
/// is wire-compatible with the post-#916 kernel action enum.
#[test]
fn publish_note_action_deserializes_into_publish_raw_variant() {
    use nmp_core::publish::{PublishAction, PublishTarget};

    let parent = parent_record("abc123", "alice");
    let (_ns, json) = publish_note_action("hello world", Some(&parent)).unwrap();
    let action: PublishAction =
        serde_json::from_str(&json).expect("builder JSON must deserialize into PublishAction");
    match action {
        PublishAction::PublishRaw {
            kind,
            tags,
            content,
            target,
            signer_pubkey,
        } => {
            assert_eq!(kind, 1);
            assert_eq!(content, "hello world");
            assert_eq!(
                tags,
                vec![
                    vec!["e", "abc123", "", "root"],
                    vec!["e", "abc123", "", "reply"],
                    vec!["p", "alice"],
                ]
            );
            assert_eq!(target, PublishTarget::Auto);
            // The note-builder envelope omits `signer_pubkey`, so
            // `#[serde(default)]` must deserialize it to `None` (active account).
            assert_eq!(signer_pubkey, None);
        }
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// react_action
// ---------------------------------------------------------------------------

#[test]
fn react_action_has_correct_event_id_and_reaction() {
    let (ns, json) = react_action("eventabc", "+");
    assert_eq!(ns, "nmp.nip25.react");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["target_event_id"], "eventabc");
    assert_eq!(v["reaction"], "+");
}

// ---------------------------------------------------------------------------
// follow_action
// ---------------------------------------------------------------------------

#[test]
fn follow_action_has_correct_pubkey() {
    let (ns, json) = follow_action("pubkey123");
    assert_eq!(ns, "nmp.follow");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["pubkey"], "pubkey123");
}

// ---------------------------------------------------------------------------
// unfollow_action
// ---------------------------------------------------------------------------

#[test]
fn unfollow_action_has_correct_namespace_and_pubkey() {
    let (ns, json) = unfollow_action("pubkey456");
    assert_eq!(ns, "nmp.unfollow");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["pubkey"], "pubkey456");
}

// ---------------------------------------------------------------------------
// send_dm_action
// ---------------------------------------------------------------------------

#[test]
fn send_dm_action_has_correct_namespace_and_fields() {
    let (ns, json) = send_dm_action("recipientpubkey", "secret message");
    assert_eq!(ns, "nmp.nip17.send");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    // The key is recipient_pubkey, not recipient
    assert_eq!(v["recipient_pubkey"], "recipientpubkey");
    assert_eq!(v["content"], "secret message");
}

