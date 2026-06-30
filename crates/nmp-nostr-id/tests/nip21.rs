//! Integration tests for NIP-21 (`nostr:` URI scheme).
//!
//! Split out of `nip19_nip21.rs` (file-size hard-cap, AGENTS.md 500-LOC
//! ceiling). NIP-19 entity codec tests stay in `nip19_nip21.rs`; the
//! `nostr:` URI scheme tests live here.

use nmp_nostr_id::{
    encode_naddr, encode_nevent, encode_note, encode_nprofile, encode_nsec, NaddrData, NeventData,
    NprofileData,
};
use nmp_nostr_id::{format_nostr_uri, parse_nostr_uri, Nip21Error, NostrUri};

// ─── Test vectors (shared with nip19_nip21.rs) ─────────────────────────────

const FIATJAF_HEX: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const FIATJAF_NPUB: &str = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
const ZERO_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const NSEC_HEX: &str = "b94f6f125c79e3a5ffaa826f584a243b527be8d9bbad37f12f4a9a363b1c9456";

#[test]
fn nip21_rejects_missing_scheme() {
    assert_eq!(
        parse_nostr_uri(FIATJAF_NPUB),
        Err(Nip21Error::MissingScheme)
    );
}

#[test]
fn nip21_rejects_wrong_scheme() {
    let uri = format!("https:{FIATJAF_NPUB}");
    assert_eq!(parse_nostr_uri(&uri), Err(Nip21Error::MissingScheme));
}

#[test]
fn nip21_rejects_nsec() {
    let uri = format!("nostr:{}", encode_nsec(NSEC_HEX).unwrap());
    assert_eq!(parse_nostr_uri(&uri), Err(Nip21Error::NsecForbidden));
}

// ─── NIP-21: entity parsing ───────────────────────────────────────────────

#[test]
fn nip21_parses_npub_uri() {
    let uri = format!("nostr:{FIATJAF_NPUB}");
    let NostrUri::Profile { pubkey, relays } = parse_nostr_uri(&uri).unwrap() else {
        panic!("expected Profile");
    };
    assert_eq!(pubkey, FIATJAF_HEX);
    assert!(relays.is_empty());
}

#[test]
fn nip21_npub_uri_round_trip() {
    let uri = format!("nostr:{FIATJAF_NPUB}");
    let target = parse_nostr_uri(&uri).unwrap();
    assert_eq!(format_nostr_uri(&target).unwrap(), uri);
}

#[test]
fn nip21_parses_nprofile_uri() {
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec!["wss://relay.damus.io".into()],
    };
    let uri = format!("nostr:{}", encode_nprofile(&data).unwrap());
    let NostrUri::Profile { pubkey, relays } = parse_nostr_uri(&uri).unwrap() else {
        panic!("expected Profile");
    };
    assert_eq!(pubkey, FIATJAF_HEX);
    assert_eq!(relays, vec!["wss://relay.damus.io"]);
}

#[test]
fn nip21_parses_note_uri() {
    let uri = format!("nostr:{}", encode_note(ZERO_HEX).unwrap());
    let NostrUri::Event {
        event_id,
        relays,
        author,
        kind,
    } = parse_nostr_uri(&uri).unwrap()
    else {
        panic!("expected Event");
    };
    assert_eq!(event_id, ZERO_HEX);
    assert!(relays.is_empty() && author.is_none() && kind.is_none());
}

#[test]
fn nip21_note_uri_round_trip() {
    let uri = format!("nostr:{}", encode_note(ZERO_HEX).unwrap());
    let target = parse_nostr_uri(&uri).unwrap();
    assert_eq!(format_nostr_uri(&target).unwrap(), uri);
}

#[test]
fn nip21_parses_nevent_uri() {
    let data = NeventData {
        event_id: FIATJAF_HEX.into(),
        relays: vec!["wss://nos.lol".into()],
        author: Some(ZERO_HEX.into()),
        kind: Some(1),
    };
    let uri = format!("nostr:{}", encode_nevent(&data).unwrap());
    let NostrUri::Event {
        event_id,
        relays,
        author,
        kind,
    } = parse_nostr_uri(&uri).unwrap()
    else {
        panic!("expected Event");
    };
    assert_eq!(event_id, FIATJAF_HEX);
    assert_eq!(relays, vec!["wss://nos.lol"]);
    assert_eq!(author, Some(ZERO_HEX.to_string()));
    assert_eq!(kind, Some(1));
}

#[test]
fn nip21_parses_naddr_uri() {
    let data = NaddrData {
        identifier: "hello-world".into(),
        pubkey: FIATJAF_HEX.into(),
        kind: 30023,
        relays: vec![],
    };
    let uri = format!("nostr:{}", encode_naddr(&data).unwrap());
    let NostrUri::Address {
        identifier,
        pubkey,
        kind,
        ..
    } = parse_nostr_uri(&uri).unwrap()
    else {
        panic!("expected Address");
    };
    assert_eq!(identifier, "hello-world");
    assert_eq!(pubkey, FIATJAF_HEX);
    assert_eq!(kind, 30023);
}

#[test]
fn nip21_naddr_uri_round_trip() {
    let data = NaddrData {
        identifier: "test-article".into(),
        pubkey: ZERO_HEX.into(),
        kind: 30023,
        relays: vec!["wss://relay.damus.io".into()],
    };
    let uri = format!("nostr:{}", encode_naddr(&data).unwrap());
    let target = parse_nostr_uri(&uri).unwrap();
    let formatted = format_nostr_uri(&target).unwrap();
    assert_eq!(parse_nostr_uri(&formatted).unwrap(), target);
}

// ─── NIP-21: known vectors from spec ─────────────────────────────────────

#[test]
fn nip21_spec_npub_example() {
    let uri = "nostr:npub1sn0wdenkukak0d9dfczzeacvhkrgz92ak56egt7vdgzn8pv2wfqqhrjdv9";
    assert!(matches!(
        parse_nostr_uri(uri).unwrap(),
        NostrUri::Profile { .. }
    ));
}

#[test]
fn nip21_spec_nprofile_example() {
    let uri = "nostr:nprofile1qqsrhuxx8l9ex335q7he0f09aej04zpazpl0ne2cgukyawd24mayt8gpp4mhxue69uhhytnc9e3k7mgpz4mhxue69uhkg6nzv9ejuumpv34kytnrdaksjlyr9p";
    let NostrUri::Profile { relays, .. } = parse_nostr_uri(uri).unwrap() else {
        panic!("expected Profile");
    };
    assert!(!relays.is_empty());
}

// ─── NIP-21: format_nostr_uri selection ──────────────────────────────────

#[test]
fn format_profile_no_relays_uses_npub() {
    let target = NostrUri::Profile {
        pubkey: FIATJAF_HEX.into(),
        relays: vec![],
    };
    assert!(format_nostr_uri(&target)
        .unwrap()
        .starts_with("nostr:npub1"));
}

#[test]
fn format_profile_with_relays_uses_nprofile() {
    let target = NostrUri::Profile {
        pubkey: FIATJAF_HEX.into(),
        relays: vec!["wss://relay.io".into()],
    };
    assert!(format_nostr_uri(&target)
        .unwrap()
        .starts_with("nostr:nprofile1"));
}

#[test]
fn format_event_no_extras_uses_note() {
    let target = NostrUri::Event {
        event_id: ZERO_HEX.into(),
        relays: vec![],
        author: None,
        kind: None,
    };
    assert!(format_nostr_uri(&target)
        .unwrap()
        .starts_with("nostr:note1"));
}

#[test]
fn format_event_with_relay_uses_nevent() {
    let target = NostrUri::Event {
        event_id: ZERO_HEX.into(),
        relays: vec!["wss://r.io".into()],
        author: None,
        kind: None,
    };
    assert!(format_nostr_uri(&target)
        .unwrap()
        .starts_with("nostr:nevent1"));
}
