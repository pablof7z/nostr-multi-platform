use super::{nmp_content_tokenize_text, tokenize_text_json};
use crate::free::nmp_free_string;
use nmp_core::nip19::{encode_nevent, encode_npub, NeventData};
use serde_json::Value;
use std::ffi::{CStr, CString};

const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const EVENT_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

fn parse_json(output: &str) -> Value {
    serde_json::from_str(output).expect("tokenize output is valid JSON")
}

fn call_ffi(content: Option<&str>, tags: Option<&str>, mode: i32, kind: u32) -> Value {
    let content = content.map(|value| CString::new(value).expect("fixture has no NUL"));
    let tags = tags.map(|value| CString::new(value).expect("fixture has no NUL"));
    let ptr = nmp_content_tokenize_text(
        content
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
        tags.as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
        mode,
        kind,
    );
    assert!(!ptr.is_null());
    // SAFETY: `ptr` is a valid heap C string returned by the FFI under test.
    let output = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("tokenize output is UTF-8")
        .to_owned();
    nmp_free_string(ptr);
    parse_json(&output)
}

#[test]
fn tokenizes_profile_and_event_refs_to_content_tree_wire() {
    let npub = encode_npub(PUBKEY).unwrap();
    let nevent = encode_nevent(&NeventData {
        event_id: EVENT_ID.to_string(),
        relays: vec!["wss://relay.example".to_string()],
        author: Some(PUBKEY.to_string()),
        kind: Some(1),
    })
    .unwrap();

    let value = call_ffi(Some(&format!("hi nostr:{npub} and @{nevent}")), None, 0, 1);

    assert_eq!(value["ok"], true);
    assert_eq!(value["tree"]["mode"], "Plain");
    let nodes = value["tree"]["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|node| {
        node["kind"] == "mention"
            && node["uri"]["kind"] == "profile"
            && node["uri"]["primary_id"] == PUBKEY
    }));
    assert!(nodes.iter().any(|node| {
        node["kind"] == "event_ref"
            && node["uri"]["kind"] == "event"
            && node["uri"]["primary_id"] == EVENT_ID
            && node["uri"]["relays"] == serde_json::json!(["wss://relay.example"])
    }));
}

#[test]
fn resolves_emoji_from_optional_tags_json() {
    let tags = r#"[["emoji","ostrich","https://x.test/ostrich.png"]]"#;
    let value = call_ffi(Some("hello :ostrich:"), Some(tags), 0, 1);
    let nodes = value["tree"]["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|node| {
        node["kind"] == "emoji"
            && node["shortcode"] == "ostrich"
            && node["url"] == "https://x.test/ostrich.png"
    }));
}

#[test]
fn auto_mode_uses_kind_to_select_markdown() {
    let value = call_ffi(Some("# Title\n\nbody"), None, 2, 30_023);
    assert_eq!(value["ok"], true);
    assert_eq!(value["tree"]["mode"], "Markdown");
    let nodes = value["tree"]["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["kind"] == "heading"));
}

#[test]
fn invalid_inputs_return_error_data() {
    assert_eq!(
        call_ffi(None, None, 0, 1),
        serde_json::json!({"ok": false, "error": "invalid-input"})
    );
    assert_eq!(
        call_ffi(Some("hi"), Some("not-json"), 0, 1),
        serde_json::json!({"ok": false, "error": "invalid-tags"})
    );
    assert_eq!(
        call_ffi(Some("hi"), None, 99, 1),
        serde_json::json!({"ok": false, "error": "invalid-mode"})
    );
}

#[test]
fn pure_helper_treats_null_tags_as_empty() {
    let content = CString::new("#nostr").unwrap();
    let out = tokenize_text_json(content.as_ptr(), std::ptr::null(), 0, 1).unwrap();
    let value = parse_json(&out);
    assert_eq!(value["ok"], true);
}
