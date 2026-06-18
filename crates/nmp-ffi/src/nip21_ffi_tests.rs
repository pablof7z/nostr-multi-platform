use super::{decode_uri_json, nmp_nip21_decode_uri};
use crate::free::nmp_free_string;
use nmp_core::nip19::{
    encode_naddr, encode_nevent, encode_note, encode_nprofile, encode_npub, encode_nsec, NaddrData,
    NeventData, NprofileData,
};
use serde_json::Value;
use std::ffi::{CStr, CString};

const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const EVENT_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

fn parse_json(output: &str) -> Value {
    serde_json::from_str(output).expect("decode output is valid JSON")
}

fn call_ffi(input: Option<&str>) -> Value {
    let input = input.map(|value| CString::new(value).expect("fixture has no NUL"));
    let ptr = nmp_nip21_decode_uri(
        input
            .as_ref()
            .map_or(std::ptr::null(), |value| value.as_ptr()),
    );
    assert!(!ptr.is_null());
    // SAFETY: `ptr` is a valid heap C string returned by the FFI under test.
    let output = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("decode output is UTF-8")
        .to_owned();
    nmp_free_string(ptr);
    parse_json(&output)
}

#[test]
fn decodes_nostr_prefixed_profile_uri() {
    let npub = encode_npub(PUBKEY).unwrap();
    let value = call_ffi(Some(&format!("nostr:{npub}")));
    assert_eq!(value["ok"], true);
    assert_eq!(value["target"], "profile");
    assert_eq!(value["pubkey"], PUBKEY);
    assert_eq!(value["relays"].as_array().unwrap().len(), 0);
}

#[test]
fn decodes_bare_profile_entity() {
    let nprofile = encode_nprofile(&NprofileData {
        pubkey: PUBKEY.to_string(),
        relays: vec!["wss://relay.example".to_string()],
    })
    .unwrap();
    let value = call_ffi(Some(&nprofile));
    assert_eq!(value["ok"], true);
    assert_eq!(value["target"], "profile");
    assert_eq!(value["pubkey"], PUBKEY);
    assert_eq!(value["relays"], serde_json::json!(["wss://relay.example"]));
}

#[test]
fn decodes_event_targets() {
    let note = encode_note(EVENT_ID).unwrap();
    let value = parse_json(&decode_uri_json(&format!("nostr:{note}")));
    assert_eq!(value["ok"], true);
    assert_eq!(value["target"], "event");
    assert_eq!(value["event_id"], EVENT_ID);
    assert!(value.get("author").is_none());
    assert!(value.get("kind").is_none());

    let nevent = encode_nevent(&NeventData {
        event_id: EVENT_ID.to_string(),
        relays: vec!["wss://relay.example".to_string()],
        author: Some(PUBKEY.to_string()),
        kind: Some(1),
    })
    .unwrap();
    let value = parse_json(&decode_uri_json(&nevent));
    assert_eq!(value["ok"], true);
    assert_eq!(value["target"], "event");
    assert_eq!(value["event_id"], EVENT_ID);
    assert_eq!(value["author"], PUBKEY);
    assert_eq!(value["kind"], 1);
    assert_eq!(value["relays"], serde_json::json!(["wss://relay.example"]));
}

#[test]
fn decodes_address_targets() {
    let naddr = encode_naddr(&NaddrData {
        identifier: "article-1".to_string(),
        pubkey: PUBKEY.to_string(),
        kind: 30_023,
        relays: vec!["wss://relay.example".to_string()],
    })
    .unwrap();
    let value = call_ffi(Some(&format!("nostr:{naddr}")));
    assert_eq!(value["ok"], true);
    assert_eq!(value["target"], "address");
    assert_eq!(value["identifier"], "article-1");
    assert_eq!(value["pubkey"], PUBKEY);
    assert_eq!(value["kind"], 30_023);
    assert_eq!(value["relays"], serde_json::json!(["wss://relay.example"]));
}

#[test]
fn rejects_nsec_without_echoing_secret_material() {
    let nsec = encode_nsec(PUBKEY).unwrap();
    let value = call_ffi(Some(&format!("nostr:{nsec}")));
    assert_eq!(
        value,
        serde_json::json!({"ok": false, "error": "nsec-forbidden"})
    );
    assert!(!value.to_string().contains(&nsec));

    let bare = call_ffi(Some(&nsec));
    assert_eq!(
        bare,
        serde_json::json!({"ok": false, "error": "nsec-forbidden"})
    );
}

#[test]
fn malformed_and_null_inputs_return_error_data() {
    assert_eq!(
        call_ffi(Some("not a nostr uri")),
        serde_json::json!({"ok": false, "error": "unparseable"})
    );
    assert_eq!(
        call_ffi(None),
        serde_json::json!({"ok": false, "error": "invalid-input"})
    );
}
