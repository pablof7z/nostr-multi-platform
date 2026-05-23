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
    assert_eq!(parse(&bech).unwrap(), Nip19Entity::Nprofile(data));
}

// ─── nevent round-trip with author + kind (exercises 4-byte TLV_KIND) ──

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
    assert_eq!(decoded, data);
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
    // Syntactically bech32-shaped but an unrecognised HRP.
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
fn decode_nprofile_missing_special_tlv_errors() {
    // A valid nprofile-HRP bech32m payload that omits TLV_SPECIAL must
    // surface MissingField rather than yielding an empty-pubkey struct.
    let mut tlv = Vec::new();
    tlv_append(&mut tlv, TLV_RELAY, b"wss://relay.example");
    let bech = encode_tlv(HRP_NPROFILE, &tlv).unwrap();
    let err = decode_nprofile(&bech).unwrap_err();
    assert_eq!(err, Nip19Error::MissingField("special/pubkey"));
}
