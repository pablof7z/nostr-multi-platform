use nmp_core::tags::{EventRef, Nip10Refs};
use nmp_nip01::NoteRecord;
use serde_json::{json, Value};

use super::{
    publish_note_spec, react_spec, repost_spec, send_dm_spec, zap_identifier_spec, zap_spec,
};

fn publish_raw_body(spec_body: &str) -> Value {
    serde_json::from_str::<Value>(spec_body).unwrap()["PublishRaw"].clone()
}

#[test]
fn publish_note_spec_reply_uses_full_nip10_root_reply_and_p_tags() {
    let parent = NoteRecord {
        event_id: "reply-id".to_string(),
        author: "bob".to_string(),
        created_at: 42,
        content: "parent".to_string(),
        refs: Nip10Refs {
            root: Some(EventRef {
                id: "root-id".to_string(),
                relay: Some("wss://root.example".to_string()),
                marker: Some("root".to_string()),
            }),
            reply: None,
            mentions: Vec::new(),
            mentioned_pubkeys: vec!["alice".to_string(), "bob".to_string(), "carol".to_string()],
        },
    };

    let spec = publish_note_spec("reply body", Some(&parent)).unwrap();
    assert_eq!(spec.namespace, "nmp.publish");
    let body = publish_raw_body(&spec.body_json);
    assert_eq!(body["kind"], 1);
    assert_eq!(body["content"], "reply body");
    assert_eq!(
        body["tags"],
        json!([
            ["e", "root-id", "wss://root.example", "root"],
            ["e", "reply-id", "", "reply"],
            ["p", "bob"],
            ["p", "alice"],
            ["p", "carol"]
        ])
    );
}

#[test]
fn publish_note_spec_root_note_has_no_tags_and_preserves_control_chars() {
    let content = "quotes \" slash \\ newline\n tab\t nul\u{0000} ctrl\u{0001}";
    let spec = publish_note_spec(content, None).unwrap();
    assert_eq!(spec.namespace, "nmp.publish");
    let body = publish_raw_body(&spec.body_json);
    assert_eq!(body["kind"], 1);
    assert_eq!(body["content"], content);
    assert_eq!(body["tags"], json!([]));
}

#[test]
fn repost_spec_uses_kind6_target_tags_and_empty_content() {
    let spec = repost_spec("event-id", "author-pubkey");
    assert_eq!(spec.namespace, "nmp.publish");
    let body = publish_raw_body(&spec.body_json);
    assert_eq!(body["kind"], 6);
    assert_eq!(body["content"], "");
    assert_eq!(body["target"], "Auto");
    assert_eq!(
        body["tags"],
        json!([["e", "event-id"], ["p", "author-pubkey"]])
    );
}

#[test]
fn react_spec_targets_event_and_omits_null_author() {
    let spec = react_spec("event-id", "+");
    assert_eq!(spec.namespace, "nmp.nip25.react");
    let body: Value = serde_json::from_str(&spec.body_json).unwrap();
    assert_eq!(body["target_event_id"], "event-id");
    assert_eq!(body["reaction"], "+");
    // `target_author_pubkey: None` is dropped from the body JSON.
    assert!(body.get("target_author_pubkey").is_none());
}

#[test]
fn send_dm_spec_drops_absent_reply_to() {
    let spec = send_dm_spec("recipient", "hello", None);
    assert_eq!(spec.namespace, "nmp.nip17.send");
    let body: Value = serde_json::from_str(&spec.body_json).unwrap();
    assert_eq!(body["recipient_pubkey"], "recipient");
    assert_eq!(body["content"], "hello");
    assert!(body.get("reply_to").is_none());
}

#[test]
fn zap_spec_preserves_amount_and_drops_empty_optionals() {
    let spec = zap_spec("recipient", 21_000, Some("target"), None, None, Vec::new());
    assert_eq!(spec.namespace, "nmp.nip57.zap");
    let body: Value = serde_json::from_str(&spec.body_json).unwrap();
    assert_eq!(body["recipient_pubkey"], "recipient");
    assert_eq!(body["amount_msats"], 21_000);
    assert_eq!(body["target_event_id"], "target");
    assert!(body.get("lnurl").is_none());
    assert!(body.get("comment").is_none());
}

#[test]
fn zap_identifier_spec_carries_raw_identifier() {
    let spec = zap_identifier_spec("alice@example.com", 21_000, None, Some("hi"));
    assert_eq!(spec.namespace, "nmp.app.chirp.zap_identifier");
    let body: Value = serde_json::from_str(&spec.body_json).unwrap();
    assert_eq!(body["recipient_identifier"], "alice@example.com");
    assert_eq!(body["amount_msats"], 21_000);
    assert_eq!(body["comment"], "hi");
    assert!(body.get("target_event_id").is_none());
}
