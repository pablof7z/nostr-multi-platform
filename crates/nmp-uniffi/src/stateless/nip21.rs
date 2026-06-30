//! Stateless NIP-21 / bare NIP-19 decode — UniFFI surface (M14-C1).
//!
//! ## Core-fn provenance
//!
//! Calls `nmp_nostr_id::parse_nostr_uri` (for `nostr:` prefixed URIs) and
//! `nmp_nostr_id::parse` (for bare bech32), returning a typed
//! `NostrUriTarget`.
//!
//! ## D6
//!
//! `nsec` inputs are rejected with `NmpError::NsecForbidden`; the original
//! secret key is NEVER present in the error (same as the C-ABI guarantee).

use nmp_nostr_id::{NaddrData, NeventData, Nip19Entity, NprofileData};
use nmp_nostr_id::{nip21, Nip21Error, NostrUri};

use crate::stateless::NmpError;

/// The decoded target of a `nostr:` URI or bare NIP-19 bech32 input.
///
/// Corresponds to the `"profile"`, `"event"`, and `"address"` target set.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum NostrUriTarget {
    /// An `npub` / `nprofile` — identifies a Nostr public key.
    Profile {
        pubkey: String,
        relays: Vec<String>,
    },
    /// A `note` / `nevent` — identifies a Nostr event.
    Event {
        event_id: String,
        relays: Vec<String>,
        author: Option<String>,
        kind: Option<u32>,
    },
    /// An `naddr` — identifies a parameterised replaceable event.
    Address {
        identifier: String,
        pubkey: String,
        kind: u32,
        relays: Vec<String>,
    },
}

/// Decode a `nostr:` URI or bare NIP-19 bech32 into a typed target.
///
/// Accepted inputs: `npub`, `nprofile`, `note`, `nevent`, `naddr`, and their
/// `nostr:` prefixed forms.
///
/// Rejected: any `nsec` form (returns `NmpError::NsecForbidden`; the key is
/// never echoed). Malformed inputs return `NmpError::Unparseable`.
///
/// Stateless: no kernel IO, no actor round-trip.
#[uniffi::export]
pub fn decode_nostr_uri(input: String) -> Result<NostrUriTarget, NmpError> {
    decode_uri_impl(&input)
}

fn decode_uri_impl(input: &str) -> Result<NostrUriTarget, NmpError> {
    if input.starts_with("nostr:") {
        return nip21::parse_nostr_uri(input)
            .map(target_from_nostr_uri)
            .map_err(error_from_nip21);
    }
    nmp_nostr_id::parse(input)
        .map_err(|_| NmpError::Unparseable)?
        .try_into()
}

impl TryFrom<Nip19Entity> for NostrUriTarget {
    type Error = NmpError;

    fn try_from(entity: Nip19Entity) -> Result<Self, Self::Error> {
        match entity {
            Nip19Entity::Nsec(_) => Err(NmpError::NsecForbidden),
            Nip19Entity::Npub(pubkey) => Ok(Self::Profile {
                pubkey,
                relays: Vec::new(),
            }),
            Nip19Entity::Nprofile(NprofileData { pubkey, relays }) => {
                Ok(Self::Profile { pubkey, relays })
            }
            Nip19Entity::Note(event_id) => Ok(Self::Event {
                event_id,
                relays: Vec::new(),
                author: None,
                kind: None,
            }),
            Nip19Entity::Nevent(NeventData {
                event_id,
                relays,
                author,
                kind,
            }) => Ok(Self::Event {
                event_id,
                relays,
                author,
                kind,
            }),
            Nip19Entity::Naddr(NaddrData {
                identifier,
                pubkey,
                kind,
                relays,
            }) => Ok(Self::Address {
                identifier,
                pubkey,
                kind,
                relays,
            }),
        }
    }
}

fn target_from_nostr_uri(target: NostrUri) -> NostrUriTarget {
    match target {
        NostrUri::Profile { pubkey, relays } => NostrUriTarget::Profile { pubkey, relays },
        NostrUri::Event {
            event_id,
            relays,
            author,
            kind,
        } => NostrUriTarget::Event {
            event_id,
            relays,
            author,
            kind,
        },
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => NostrUriTarget::Address {
            identifier,
            pubkey,
            kind,
            relays,
        },
    }
}

fn error_from_nip21(error: Nip21Error) -> NmpError {
    match error {
        Nip21Error::NsecForbidden => NmpError::NsecForbidden,
        Nip21Error::MissingScheme | Nip21Error::Nip19(_) => NmpError::Unparseable,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nostr_id::{
        encode_naddr, encode_nevent, encode_npub, encode_nprofile, encode_nsec,
        NaddrData, NeventData, NprofileData,
    };

    const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const EVENT_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    // Parity: this surface calls `nmp_nostr_id::parse` and
    // `nmp_nostr_id::parse_nostr_uri`; the tests cover each supported
    // target shape directly.

    #[test]
    fn parity_bare_npub_decodes_to_profile() {
        let npub = encode_npub(PUBKEY).unwrap();
        let target = decode_nostr_uri(npub).unwrap();
        let NostrUriTarget::Profile { pubkey, relays } = target else {
            panic!("expected Profile variant");
        };
        assert_eq!(pubkey, PUBKEY);
        assert!(relays.is_empty());
    }

    #[test]
    fn parity_nostr_prefixed_npub_decodes_to_profile() {
        let npub = encode_npub(PUBKEY).unwrap();
        let uri = format!("nostr:{npub}");
        let target = decode_nostr_uri(uri).unwrap();
        let NostrUriTarget::Profile { pubkey, relays } = target else {
            panic!("expected Profile variant");
        };
        assert_eq!(pubkey, PUBKEY);
        assert!(relays.is_empty());
    }

    #[test]
    fn parity_nprofile_carries_relays() {
        let nprofile = encode_nprofile(&NprofileData {
            pubkey: PUBKEY.to_string(),
            relays: vec!["wss://relay.example".to_string()],
        })
        .unwrap();
        let target = decode_nostr_uri(nprofile).unwrap();
        let NostrUriTarget::Profile { pubkey, relays } = target else {
            panic!("expected Profile variant");
        };
        assert_eq!(pubkey, PUBKEY);
        assert_eq!(relays, vec!["wss://relay.example".to_string()]);
    }

    #[test]
    fn parity_nevent_carries_all_fields() {
        let nevent = encode_nevent(&NeventData {
            event_id: EVENT_ID.to_string(),
            relays: vec!["wss://relay.example".to_string()],
            author: Some(PUBKEY.to_string()),
            kind: Some(1),
        })
        .unwrap();
        let target = decode_nostr_uri(nevent).unwrap();
        let NostrUriTarget::Event {
            event_id,
            relays,
            author,
            kind,
        } = target
        else {
            panic!("expected Event variant");
        };
        assert_eq!(event_id, EVENT_ID);
        assert_eq!(relays, vec!["wss://relay.example".to_string()]);
        assert_eq!(author, Some(PUBKEY.to_string()));
        assert_eq!(kind, Some(1));
    }

    #[test]
    fn parity_naddr_carries_all_fields() {
        let naddr = encode_naddr(&NaddrData {
            identifier: "article-1".to_string(),
            pubkey: PUBKEY.to_string(),
            kind: 30_023,
            relays: vec!["wss://relay.example".to_string()],
        })
        .unwrap();
        let target = decode_nostr_uri(naddr).unwrap();
        let NostrUriTarget::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } = target
        else {
            panic!("expected Address variant");
        };
        assert_eq!(identifier, "article-1");
        assert_eq!(pubkey, PUBKEY);
        assert_eq!(kind, 30_023);
        assert_eq!(relays, vec!["wss://relay.example".to_string()]);
    }

    #[test]
    fn parity_nsec_rejected_without_echoing_key() {
        // C-ABI: returns {"ok":false,"error":"nsec-forbidden"}; key never echoed.
        // UniFFI: returns Err(NmpError::NsecForbidden); key never echoed.
        let nsec = encode_nsec(PUBKEY).unwrap();
        let err = decode_nostr_uri(nsec.clone()).unwrap_err();
        assert!(
            matches!(err, NmpError::NsecForbidden),
            "expected NsecForbidden, got {err:?}"
        );
        // D6: the nsec string must NOT appear in the error description.
        assert!(!err.to_string().contains(&nsec));
    }

    #[test]
    fn parity_nostr_nsec_uri_rejected() {
        let nsec = encode_nsec(PUBKEY).unwrap();
        let uri = format!("nostr:{nsec}");
        let err = decode_nostr_uri(uri).unwrap_err();
        assert!(matches!(err, NmpError::NsecForbidden));
    }

    #[test]
    fn parity_malformed_input_returns_unparseable() {
        // C-ABI: returns {"ok":false,"error":"unparseable"}.
        let err = decode_nostr_uri("not-a-nostr-thing".to_string()).unwrap_err();
        assert!(matches!(err, NmpError::Unparseable));
    }
}
