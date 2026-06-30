//! Property-based integration tests for NIP-19 entities used by NIP-21.

use nmp_nostr_id::{
    decode_naddr, decode_nevent, decode_nprofile, decode_npub, encode_naddr, encode_nevent,
    encode_nprofile, encode_npub, NaddrData, NeventData, NprofileData,
};
use proptest::prelude::*;

fn hex32() -> impl Strategy<Value = String> {
    proptest::collection::vec(any::<u8>(), 32)
        .prop_map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn relay_url() -> impl Strategy<Value = String> {
    "[a-z]{3,8}".prop_map(|s| format!("wss://{s}.io"))
}

/// Nostr event kinds are u16 (0..=65535) per the protocol, and the canonical
/// `nostr` NIP-19 codec NMP delegates to (#1493) encodes the kind as a u16.
/// The NMP surface keeps `kind: Option<u32>` for ergonomics; the
/// round-trippable domain is the valid kind range.
fn valid_kind() -> impl Strategy<Value = u32> {
    any::<u16>().prop_map(u32::from)
}

proptest! {
    #[test]
    fn prop_npub_round_trip(hex in hex32()) {
        let bech = encode_npub(&hex).unwrap();
        prop_assert_eq!(decode_npub(&bech).unwrap(), hex);
    }

    #[test]
    fn prop_nprofile_round_trip(
        hex in hex32(),
        relays in proptest::collection::vec(relay_url(), 0..=3)
    ) {
        let data = NprofileData { pubkey: hex, relays };
        let bech = encode_nprofile(&data).unwrap();
        prop_assert_eq!(decode_nprofile(&bech).unwrap(), data);
    }

    #[test]
    fn prop_nevent_round_trip(
        hex in hex32(),
        author_hex in hex32(),
        kind in valid_kind(),
        relays in proptest::collection::vec(relay_url(), 0..=2)
    ) {
        let data = NeventData {
            event_id: hex,
            relays,
            author: Some(author_hex),
            kind: Some(kind),
        };
        let bech = encode_nevent(&data).unwrap();
        prop_assert_eq!(decode_nevent(&bech).unwrap(), data);
    }

    #[test]
    fn prop_naddr_round_trip(
        id in "[a-z0-9-]{0,40}",
        hex in hex32(),
        kind in valid_kind(),
        relays in proptest::collection::vec(relay_url(), 0..=2)
    ) {
        let data = NaddrData { identifier: id, pubkey: hex, kind, relays };
        let bech = encode_naddr(&data).unwrap();
        prop_assert_eq!(decode_naddr(&bech).unwrap(), data);
    }
}
