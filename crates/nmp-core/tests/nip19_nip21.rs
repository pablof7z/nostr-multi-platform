//! Integration tests for NIP-19 (bech32 entities) and NIP-21 (`nostr:` URI scheme).

use nmp_core::nip19::{
    self, decode_naddr, decode_nevent, decode_note, decode_nprofile, decode_npub, decode_nsec,
    encode_naddr, encode_nevent, encode_note, encode_nprofile, encode_npub, encode_nsec, NaddrData,
    NeventData, Nip19Entity, Nip19Error, NprofileData,
};
use nmp_core::nip21::{format_nostr_uri, parse_nostr_uri, Nip21Error, NostrUri};

// ─── Test vectors ──────────────────────────────────────────────────────────

// From the NIP-19 spec.
const FIATJAF_HEX: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const FIATJAF_NPUB: &str = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
const ZERO_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000000";
const FF_HEX: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const NSEC_HEX: &str = "b94f6f125c79e3a5ffaa826f584a243b527be8d9bbad37f12f4a9a363b1c9456";

// ─── NIP-19: npub ─────────────────────────────────────────────────────────

#[test]
fn npub_encode_known_vector() {
    assert_eq!(encode_npub(FIATJAF_HEX).unwrap(), FIATJAF_NPUB);
}

#[test]
fn npub_decode_known_vector() {
    assert_eq!(decode_npub(FIATJAF_NPUB).unwrap(), FIATJAF_HEX);
}

#[test]
fn npub_round_trip_zero() {
    let bech = encode_npub(ZERO_HEX).unwrap();
    assert!(bech.starts_with("npub1"));
    assert_eq!(decode_npub(&bech).unwrap(), ZERO_HEX);
}

#[test]
fn npub_round_trip_ff() {
    let bech = encode_npub(FF_HEX).unwrap();
    assert_eq!(decode_npub(&bech).unwrap(), FF_HEX);
}

#[test]
fn npub_rejects_short_hex() {
    assert_eq!(encode_npub("deadbeef"), Err(Nip19Error::InvalidHex));
}

#[test]
fn npub_rejects_nonhex_chars() {
    let bad = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
    assert_eq!(encode_npub(bad), Err(Nip19Error::InvalidHex));
}

// ─── NIP-19: nsec ─────────────────────────────────────────────────────────

#[test]
fn nsec_round_trip() {
    let bech = encode_nsec(NSEC_HEX).unwrap();
    assert!(bech.starts_with("nsec1"));
    assert_eq!(decode_nsec(&bech).unwrap(), NSEC_HEX);
}

#[test]
fn nsec_rejects_npub_bech() {
    assert!(matches!(
        decode_nsec(FIATJAF_NPUB),
        Err(Nip19Error::UnknownHrp(_))
    ));
}

// ─── NIP-19: note ─────────────────────────────────────────────────────────

#[test]
fn note_round_trip() {
    let hex = "aabbccdd".repeat(8);
    let bech = encode_note(&hex).unwrap();
    assert!(bech.starts_with("note1"));
    assert_eq!(decode_note(&bech).unwrap(), hex);
}

#[test]
fn note_rejects_wrong_hrp() {
    let bech_with_wrong_hrp = encode_npub(ZERO_HEX).unwrap().replace("npub1", "note1");
    assert!(decode_note(&bech_with_wrong_hrp).is_err());
}

// ─── NIP-19: nprofile ─────────────────────────────────────────────────────

#[test]
fn nprofile_no_relays_round_trip() {
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec![],
    };
    let bech = encode_nprofile(&data).unwrap();
    assert!(bech.starts_with("nprofile1"));
    assert_eq!(decode_nprofile(&bech).unwrap(), data);
}

#[test]
fn nprofile_with_relays_round_trip() {
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec!["wss://relay.damus.io".into(), "wss://nos.lol".into()],
    };
    assert_eq!(
        decode_nprofile(&encode_nprofile(&data).unwrap()).unwrap(),
        data
    );
}

#[test]
fn nprofile_relay_order_preserved() {
    let data = NprofileData {
        pubkey: ZERO_HEX.into(),
        relays: vec![
            "wss://a.io".into(),
            "wss://b.io".into(),
            "wss://c.io".into(),
        ],
    };
    let decoded = decode_nprofile(&encode_nprofile(&data).unwrap()).unwrap();
    assert_eq!(decoded.relays, data.relays);
}

#[test]
fn nprofile_rejects_garbage() {
    assert!(decode_nprofile("nprofile1qqsgarbagedata").is_err());
}

#[test]
fn nprofile_unknown_tlv_ignored() {
    use bech32::Bech32m;
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec![],
    };
    let bech = encode_nprofile(&data).unwrap();
    let (hrp, mut bytes) = bech32::decode(&bech).unwrap();
    bytes.extend_from_slice(&[99u8, 1u8, 42u8]); // unknown TLV type
    let new_bech = bech32::encode::<Bech32m>(hrp, &bytes).unwrap();
    assert_eq!(decode_nprofile(&new_bech).unwrap().pubkey, data.pubkey);
}

// ─── NIP-19: nevent ───────────────────────────────────────────────────────

#[test]
fn nevent_minimal_round_trip() {
    let data = NeventData {
        event_id: FIATJAF_HEX.into(),
        relays: vec![],
        author: None,
        kind: None,
    };
    let bech = encode_nevent(&data).unwrap();
    assert!(bech.starts_with("nevent1"));
    assert_eq!(decode_nevent(&bech).unwrap(), data);
}

#[test]
fn nevent_full_round_trip() {
    let data = NeventData {
        event_id: FIATJAF_HEX.into(),
        relays: vec!["wss://relay.snort.social".into()],
        author: Some(ZERO_HEX.into()),
        kind: Some(1),
    };
    assert_eq!(decode_nevent(&encode_nevent(&data).unwrap()).unwrap(), data);
}

#[test]
fn nevent_kind_max_u32() {
    let data = NeventData {
        event_id: FF_HEX.into(),
        relays: vec![],
        author: None,
        kind: Some(u32::MAX),
    };
    assert_eq!(
        decode_nevent(&encode_nevent(&data).unwrap()).unwrap().kind,
        Some(u32::MAX)
    );
}

#[test]
fn nevent_rejects_missing_event_id() {
    use bech32::{Bech32m, Hrp};
    use nmp_core::nip19::{tlv_append, TLV_RELAY};
    let mut tlv = Vec::new();
    tlv_append(&mut tlv, TLV_RELAY, b"wss://relay.io");
    let hrp = Hrp::parse("nevent").unwrap();
    let bech = bech32::encode::<Bech32m>(hrp, &tlv).unwrap();
    assert!(matches!(
        decode_nevent(&bech),
        Err(Nip19Error::MissingField(_))
    ));
}

// ─── NIP-19: naddr ────────────────────────────────────────────────────────

#[test]
fn naddr_round_trip_simple() {
    let data = NaddrData {
        identifier: "my-article".into(),
        pubkey: FIATJAF_HEX.into(),
        kind: 30023,
        relays: vec![],
    };
    let bech = encode_naddr(&data).unwrap();
    assert!(bech.starts_with("naddr1"));
    assert_eq!(decode_naddr(&bech).unwrap(), data);
}

#[test]
fn naddr_empty_identifier() {
    let data = NaddrData {
        identifier: "".into(),
        pubkey: ZERO_HEX.into(),
        kind: 1,
        relays: vec![],
    };
    assert_eq!(
        decode_naddr(&encode_naddr(&data).unwrap())
            .unwrap()
            .identifier,
        ""
    );
}

#[test]
fn naddr_with_relays() {
    let data = NaddrData {
        identifier: "hello-world".into(),
        pubkey: FF_HEX.into(),
        kind: 30023,
        relays: vec!["wss://relay.nostr.band".into()],
    };
    assert_eq!(decode_naddr(&encode_naddr(&data).unwrap()).unwrap(), data);
}

#[test]
fn naddr_missing_author_is_error() {
    use bech32::{Bech32m, Hrp};
    use nmp_core::nip19::{tlv_append, TLV_KIND, TLV_SPECIAL};
    let mut tlv = Vec::new();
    tlv_append(&mut tlv, TLV_SPECIAL, b"test-id");
    tlv_append(&mut tlv, TLV_KIND, &30023u32.to_be_bytes());
    let hrp = Hrp::parse("naddr").unwrap();
    let bech = bech32::encode::<Bech32m>(hrp, &tlv).unwrap();
    assert!(matches!(
        decode_naddr(&bech),
        Err(Nip19Error::MissingField(_))
    ));
}

// ─── NIP-19: polymorphic parse / format ───────────────────────────────────

#[test]
fn parse_dispatches_npub() {
    assert!(matches!(
        nip19::parse(FIATJAF_NPUB).unwrap(),
        Nip19Entity::Npub(_)
    ));
}

#[test]
fn parse_dispatches_nsec() {
    let bech = encode_nsec(NSEC_HEX).unwrap();
    assert!(matches!(nip19::parse(&bech).unwrap(), Nip19Entity::Nsec(_)));
}

#[test]
fn parse_dispatches_note() {
    let bech = encode_note(ZERO_HEX).unwrap();
    assert!(matches!(nip19::parse(&bech).unwrap(), Nip19Entity::Note(_)));
}

#[test]
fn parse_dispatches_nprofile() {
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec![],
    };
    let bech = encode_nprofile(&data).unwrap();
    assert!(matches!(
        nip19::parse(&bech).unwrap(),
        Nip19Entity::Nprofile(_)
    ));
}

#[test]
fn parse_dispatches_nevent() {
    let data = NeventData {
        event_id: FIATJAF_HEX.into(),
        relays: vec![],
        author: None,
        kind: None,
    };
    let bech = encode_nevent(&data).unwrap();
    assert!(matches!(
        nip19::parse(&bech).unwrap(),
        Nip19Entity::Nevent(_)
    ));
}

#[test]
fn parse_dispatches_naddr() {
    let data = NaddrData {
        identifier: "x".into(),
        pubkey: ZERO_HEX.into(),
        kind: 30023,
        relays: vec![],
    };
    let bech = encode_naddr(&data).unwrap();
    assert!(matches!(
        nip19::parse(&bech).unwrap(),
        Nip19Entity::Naddr(_)
    ));
}

#[test]
fn parse_unknown_hrp_is_error() {
    assert!(matches!(
        nip19::parse("nrelay1qq28qqqqg"),
        Err(Nip19Error::UnknownHrp(_))
    ));
}

#[test]
fn format_inverts_parse() {
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec!["wss://relay.io".into()],
    };
    let bech = encode_nprofile(&data).unwrap();
    let entity = nip19::parse(&bech).unwrap();
    assert_eq!(nip19::format(&entity).unwrap(), bech);
}

// ─── NIP-21: scheme gate ──────────────────────────────────────────────────

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
        relays: vec!["wss://relay.nostr.band".into()],
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
