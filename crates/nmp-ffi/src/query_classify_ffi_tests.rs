use super::{classify_query, nmp_app_search_classify, QueryClass};
use crate::free::nmp_free_string;
use nmp_core::nip19::{encode_naddr, encode_nevent, encode_note, encode_npub, encode_nsec, NaddrData, NeventData};
use serde_json::Value;
use std::ffi::{CStr, CString};

const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const EVENT_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

fn call_ffi(input: &str) -> Value {
    let c = CString::new(input).expect("fixture has no NUL");
    let ptr = nmp_app_search_classify(c.as_ptr());
    assert!(!ptr.is_null());
    // SAFETY: `ptr` is a valid heap C string returned by the FFI under test.
    let out = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("classify output is UTF-8")
        .to_owned();
    nmp_free_string(ptr);
    serde_json::from_str(&out).expect("classify output is valid JSON")
}

#[test]
fn npub_classifies_as_profile() {
    let npub = encode_npub(PUBKEY).unwrap();
    assert_eq!(
        classify_query(&npub),
        QueryClass::Profile {
            pubkey: PUBKEY.to_string(),
            relays: Vec::new()
        }
    );
    // `nostr:` prefix routes identically.
    assert_eq!(
        classify_query(&format!("nostr:{npub}")),
        QueryClass::Profile {
            pubkey: PUBKEY.to_string(),
            relays: Vec::new()
        }
    );
}

#[test]
fn note_and_nevent_classify_as_event() {
    let note = encode_note(EVENT_ID).unwrap();
    assert_eq!(
        classify_query(&note),
        QueryClass::Event {
            event_id: EVENT_ID.to_string(),
            relays: Vec::new(),
            author: None,
            event_kind: None
        }
    );
    let nevent = encode_nevent(&NeventData {
        event_id: EVENT_ID.to_string(),
        relays: vec!["wss://relay.example".to_string()],
        author: Some(PUBKEY.to_string()),
        kind: Some(1),
    })
    .unwrap();
    assert_eq!(
        classify_query(&nevent),
        QueryClass::Event {
            event_id: EVENT_ID.to_string(),
            relays: vec!["wss://relay.example".to_string()],
            author: Some(PUBKEY.to_string()),
            event_kind: Some(1)
        }
    );
}

#[test]
fn naddr_and_nsec_are_unsupported() {
    let naddr = encode_naddr(&NaddrData {
        identifier: "a".to_string(),
        pubkey: PUBKEY.to_string(),
        kind: 30_023,
        relays: Vec::new(),
    })
    .unwrap();
    assert_eq!(
        classify_query(&naddr),
        QueryClass::Unsupported {
            reason: "addressable-unsupported".to_string()
        }
    );
    let nsec = encode_nsec(PUBKEY).unwrap();
    assert_eq!(
        classify_query(&nsec),
        QueryClass::Unsupported {
            reason: "nsec-forbidden".to_string()
        }
    );
    // The secret material is never echoed back in the classification.
    assert!(!serde_json::to_string(&classify_query(&nsec))
        .unwrap()
        .contains(&nsec));
}

#[test]
fn hashtag_forms_normalize() {
    assert_eq!(
        classify_query("#Nostr"),
        QueryClass::Hashtag {
            tag: "nostr".to_string()
        }
    );
    // A single bare token (no whitespace, no `@`) is a hashtag too.
    assert_eq!(
        classify_query("  bitcoin "),
        QueryClass::Hashtag {
            tag: "bitcoin".to_string()
        }
    );
}

#[test]
fn nip05_identifier_is_recognized() {
    assert_eq!(
        classify_query("Alice@Example.com"),
        QueryClass::Nip05 {
            identifier: "alice@example.com".to_string()
        }
    );
    // A bare `_@domain` root identifier is valid NIP-05.
    assert_eq!(
        classify_query("_@nostr.example.org"),
        QueryClass::Nip05 {
            identifier: "_@nostr.example.org".to_string()
        }
    );
}

#[test]
fn multiword_and_malformed_at_fall_back_to_search() {
    assert_eq!(
        classify_query("hello nostr world"),
        QueryClass::Freetext {
            query: "hello nostr world".to_string()
        }
    );
    // An `@` without a dotted domain is not NIP-05 → free text.
    assert_eq!(
        classify_query("@bob"),
        QueryClass::Freetext {
            query: "@bob".to_string()
        }
    );
}

#[test]
fn empty_input_is_unsupported() {
    assert_eq!(
        classify_query("   "),
        QueryClass::Unsupported {
            reason: "empty".to_string()
        }
    );
}

#[test]
fn ffi_emits_kind_tagged_json_and_frees_cleanly() {
    let v = call_ffi("#nostr");
    assert_eq!(v["kind"], "hashtag");
    assert_eq!(v["tag"], "nostr");

    let v = call_ffi("hello world");
    assert_eq!(v["kind"], "search");
    assert_eq!(v["query"], "hello world");

    let npub = encode_npub(PUBKEY).unwrap();
    let v = call_ffi(&npub);
    assert_eq!(v["kind"], "profile");
    assert_eq!(v["pubkey"], PUBKEY);
}
