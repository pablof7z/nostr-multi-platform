//! Integration tests for NIP-19 (bech32 entity codec). The NIP-21 (`nostr:`
//! URI scheme) tests live in the sibling `nip21.rs` (file-size hard-cap split).

use nmp_nip19::{
    decode_naddr, decode_nevent, decode_note, decode_nprofile, decode_npub, decode_nsec,
    encode_naddr, encode_nevent, encode_note, encode_nprofile, encode_npub, encode_nsec, NaddrData,
    NeventData, Nip19Entity, Nip19Error, NprofileData,
};

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
fn nevent_kind_max_valid() {
    // Nostr event kinds are u16 (0..=65535) per the protocol; the canonical
    // `nostr` NIP-19 codec encodes the kind as a u16. The NMP surface keeps
    // `kind: Option<u32>` for ergonomics, but the round-trippable ceiling is
    // `u16::MAX` — the largest kind a real Nostr event can carry. (#1493:
    // delegating to rust-nostr replaced the prior hand-rolled 4-byte-u32 TLV.)
    let data = NeventData {
        event_id: FF_HEX.into(),
        relays: vec![],
        author: None,
        kind: Some(u32::from(u16::MAX)),
    };
    assert_eq!(
        decode_nevent(&encode_nevent(&data).unwrap()).unwrap().kind,
        Some(u32::from(u16::MAX))
    );
}

#[test]
fn nevent_rejects_missing_event_id() {
    // A valid `nevent`-HRP bech32m body that carries only a relay TLV (type 1)
    // and no `special` event-id TLV must surface a typed decode error, not a
    // panic or an empty-id struct. (Built with the raw `bech32` crate since
    // the NIP-19 codec internals are no longer exposed — see #1493.)
    use bech32::{Bech32m, Hrp};
    let relay = b"wss://relay.io";
    let mut tlv = Vec::new();
    tlv.push(1u8); // TLV type: relay
    tlv.push(relay.len() as u8);
    tlv.extend_from_slice(relay);
    let hrp = Hrp::parse("nevent").unwrap();
    let bech = bech32::encode::<Bech32m>(hrp, &tlv).unwrap();
    assert!(
        matches!(
            decode_nevent(&bech),
            Err(Nip19Error::MalformedTlv(_) | Nip19Error::Bech32(_))
        ),
        "nevent without a special event-id TLV must be a typed error"
    );
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
        relays: vec!["wss://relay.damus.io".into()],
    };
    assert_eq!(decode_naddr(&encode_naddr(&data).unwrap()).unwrap(), data);
}

#[test]
fn naddr_missing_author_is_error() {
    // A valid `naddr`-HRP bech32m body carrying a `special` identifier TLV
    // (type 0) and a `kind` TLV (type 3) but NO author TLV (type 2) must be a
    // typed decode error. (Raw `bech32` build — the codec internals are no
    // longer exposed; see #1493.)
    use bech32::{Bech32m, Hrp};
    let id = b"test-id";
    let mut tlv = Vec::new();
    tlv.push(0u8); // TLV type: special (identifier)
    tlv.push(id.len() as u8);
    tlv.extend_from_slice(id);
    let kind = 30_023u32.to_be_bytes();
    tlv.push(3u8); // TLV type: kind
    tlv.push(kind.len() as u8);
    tlv.extend_from_slice(&kind);
    let hrp = Hrp::parse("naddr").unwrap();
    let bech = bech32::encode::<Bech32m>(hrp, &tlv).unwrap();
    assert!(
        matches!(
            decode_naddr(&bech),
            Err(Nip19Error::MalformedTlv(_) | Nip19Error::Bech32(_))
        ),
        "naddr without an author TLV must be a typed error"
    );
}

// ─── NIP-19: polymorphic parse / format ───────────────────────────────────

#[test]
fn parse_dispatches_npub() {
    assert!(matches!(
        nmp_nip19::parse(FIATJAF_NPUB).unwrap(),
        Nip19Entity::Npub(_)
    ));
}

#[test]
fn parse_dispatches_nsec() {
    let bech = encode_nsec(NSEC_HEX).unwrap();
    assert!(matches!(nmp_nip19::parse(&bech).unwrap(), Nip19Entity::Nsec(_)));
}

#[test]
fn parse_dispatches_note() {
    let bech = encode_note(ZERO_HEX).unwrap();
    assert!(matches!(nmp_nip19::parse(&bech).unwrap(), Nip19Entity::Note(_)));
}

#[test]
fn parse_dispatches_nprofile() {
    let data = NprofileData {
        pubkey: FIATJAF_HEX.into(),
        relays: vec![],
    };
    let bech = encode_nprofile(&data).unwrap();
    assert!(matches!(
        nmp_nip19::parse(&bech).unwrap(),
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
        nmp_nip19::parse(&bech).unwrap(),
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
        nmp_nip19::parse(&bech).unwrap(),
        Nip19Entity::Naddr(_)
    ));
}

#[test]
fn parse_unknown_hrp_is_error() {
    assert!(matches!(
        nmp_nip19::parse("nrelay1qq28qqqqg"),
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
    let entity = nmp_nip19::parse(&bech).unwrap();
    assert_eq!(nmp_nip19::format(&entity).unwrap(), bech);
}
