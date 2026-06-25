use nmp_core::tags::{EventRef, Nip10Refs};
use nmp_nip01::NoteRecord;
use serde_json::{json, Value};

use super::{
    action_spec_for_intent_json, action_spec_json_for_intent, publish_note_spec, repost_spec,
};

fn publish_raw_body(spec_body: &str) -> Value {
    serde_json::from_str::<Value>(spec_body).unwrap()["PublishRaw"].clone()
}

#[test]
fn publish_reply_intent_uses_full_nip10_root_reply_and_p_tags() {
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
fn publish_minimal_reply_intent_builds_marked_root_and_reply_tags() {
    let spec = action_spec_for_intent_json(
        r#"{"type":"publish_note","content":"reply","reply_to_event_id":"parent-id"}"#,
    )
    .unwrap();
    let body = publish_raw_body(&spec.body_json);
    assert_eq!(
        body["tags"],
        json!([
            ["e", "parent-id", "", "root"],
            ["e", "parent-id", "", "reply"]
        ])
    );
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
fn action_spec_round_trips_json_escaping_and_control_chars() {
    let content = "quotes \" slash \\ newline\n tab\t nul\u{0000} ctrl\u{0001}";
    let intent = json!({ "type": "publish_note", "content": content }).to_string();
    let spec_json = action_spec_json_for_intent(&intent);
    let spec: Value = serde_json::from_str(&spec_json).unwrap();
    assert_eq!(spec["namespace"], "nmp.publish");

    let body = publish_raw_body(spec["body_json"].as_str().unwrap());
    assert_eq!(body["content"], content);
}

#[test]
fn native_facing_spec_reports_malformed_intent_as_error_json() {
    let spec_json = action_spec_json_for_intent(r#"{"type":"publish_note","content":}"#);
    let spec: Value = serde_json::from_str(&spec_json).unwrap();
    assert!(spec["error"]
        .as_str()
        .unwrap()
        .contains("invalid Chirp action intent JSON"));
}
