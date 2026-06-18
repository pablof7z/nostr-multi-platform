use super::*;

/// Deterministic 32-byte hex fixture (matches the module doctests).
const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
/// A second distinct deterministic 32-byte hex fixture (event id / author).
const ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

// ─── parse() polymorphic dispatcher ────────────────────────────────────

#[test]
fn parse_dispatches_npub_to_npub_variant() {
    let bech = encode_npub(PK).unwrap();
    assert_eq!(parse(&bech).unwrap(), Nip19Entity::Npub(PK.into()));
}

#[test]
fn parse_dispatches_note_to_note_variant() {
    let bech = encode_note(ID).unwrap();
    assert_eq!(parse(&bech).unwrap(), Nip19Entity::Note(ID.into()));
}

#[test]
fn parse_dispatches_nprofile_to_nprofile_variant() {
    let data = NprofileData {
        pubkey: PK.into(),
        relays: vec!["wss://relay.example".into()],
    };
    let bech = encode_nprofile(&data).unwrap();
    // The decoded relay round-trips through `nostr::RelayUrl`, which
    // normalises a trailing slash — compare the decoded entity to the
    // re-decoded form rather than the raw input.
    let expected = decode_nprofile(&bech).unwrap();
    assert_eq!(parse(&bech).unwrap(), Nip19Entity::Nprofile(expected));
}

// ─── nevent round-trip with author + kind (exercises the kind TLV) ─────

#[test]
fn nevent_round_trip_preserves_author_and_kind() {
    let data = NeventData {
        event_id: ID.into(),
        relays: vec!["wss://relay.example".into()],
        author: Some(PK.into()),
        kind: Some(1),
    };
    let bech = encode_nevent(&data).unwrap();
    assert!(bech.starts_with("nevent1"));
    let decoded = decode_nevent(&bech).unwrap();
    assert_eq!(decoded.event_id, data.event_id);
    assert_eq!(decoded.author, data.author);
    assert_eq!(decoded.kind, data.kind);
    // Relay URL normalises through `nostr::RelayUrl`; assert by host, not by
    // exact string, so a normalising trailing slash does not fail the test.
    assert_eq!(decoded.relays.len(), 1);
    assert!(decoded.relays[0].contains("relay.example"));
}

// ─── error paths — silent-failure classes ──────────────────────────────

#[test]
fn parse_non_bech32_input_errors_without_panic() {
    // No '1' separator at all — must be a graceful Err, never a panic.
    let err = parse("notbech32atall").unwrap_err();
    assert!(matches!(err, Nip19Error::Bech32(_)));
}

#[test]
fn parse_unknown_hrp_errors_without_panic() {
    // Syntactically bech32-shaped but an unrecognised HRP. The HRP is
    // surfaced verbatim so callers can log which prefix was rejected.
    let err = parse("xyz1qqqqqqqq").unwrap_err();
    assert!(matches!(err, Nip19Error::UnknownHrp(hrp) if hrp == "xyz"));
}

#[test]
fn decode_npub_rejects_cross_hrp_nprofile_string() {
    // Cross-HRP confusion is a real silent-routing bug class: an
    // nprofile string fed to decode_npub must not silently succeed.
    let nprofile = encode_nprofile(&NprofileData {
        pubkey: PK.into(),
        relays: vec![],
    })
    .unwrap();
    let err = decode_npub(&nprofile).unwrap_err();
    assert!(matches!(err, Nip19Error::UnknownHrp(hrp) if hrp == "nprofile"));
}

#[test]
fn encode_npub_rejects_non_hex_input() {
    let err = encode_npub("not-hex-and-wrong-length").unwrap_err();
    assert_eq!(err, Nip19Error::InvalidHex);
}

#[test]
fn encode_npub_rejects_short_hex() {
    // Short-but-hex input is still the wrong length for a 32-byte key.
    assert_eq!(encode_npub("deadbeef"), Err(Nip19Error::InvalidHex));
}

#[test]
fn decode_nprofile_on_garbage_payload_errors_without_panic() {
    // A valid `nprofile`-HRP prefix but a malformed TLV body must surface a
    // typed error, not panic or yield an empty-pubkey struct.
    let err = decode_nprofile("nprofile1qqqqqqqqqqqqqq").unwrap_err();
    assert!(
        matches!(err, Nip19Error::MalformedTlv(_) | Nip19Error::Bech32(_)),
        "expected MalformedTlv/Bech32, got {err:?}"
    );
}

#[test]
fn npub_round_trips_through_parse_and_format() {
    let bech = encode_npub(PK).unwrap();
    let entity = parse(&bech).unwrap();
    assert_eq!(format(&entity).unwrap(), bech);
}

// ─── adapter-boundary guards (the NMP surface is wider than nostr's) ─────

#[test]
fn encode_nevent_rejects_kind_above_u16() {
    // Nostr kinds are u16; the NMP surface is `Option<u32>`. A kind > 65535
    // must be a typed error, never a silent `as u16` truncation to a
    // different kind.
    let data = NeventData {
        event_id: ID.into(),
        relays: vec![],
        author: None,
        kind: Some(u32::from(u16::MAX) + 1),
    };
    assert!(matches!(
        encode_nevent(&data),
        Err(Nip19Error::MalformedTlv(_))
    ));
}

#[test]
fn encode_naddr_rejects_kind_above_u16() {
    let data = NaddrData {
        identifier: "d".into(),
        pubkey: PK.into(),
        kind: 70_000,
        relays: vec![],
    };
    assert!(matches!(
        encode_naddr(&data),
        Err(Nip19Error::MalformedTlv(_))
    ));
}

#[test]
fn encode_nprofile_rejects_oversized_relay() {
    // A relay URL over the 255-byte TLV limit would not round-trip; reject it.
    let long_relay = format!("wss://{}.example", "a".repeat(300));
    let data = NprofileData {
        pubkey: PK.into(),
        relays: vec![long_relay],
    };
    assert!(matches!(
        encode_nprofile(&data),
        Err(Nip19Error::MalformedTlv(_))
    ));
}

#[test]
fn encode_naddr_rejects_oversized_identifier() {
    let data = NaddrData {
        identifier: "x".repeat(256),
        pubkey: PK.into(),
        kind: 30023,
        relays: vec![],
    };
    assert!(matches!(
        encode_naddr(&data),
        Err(Nip19Error::MalformedTlv(_))
    ));
}
