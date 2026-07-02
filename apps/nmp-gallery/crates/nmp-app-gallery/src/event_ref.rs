use nmp_nostr_id::{nip21, NaddrData, NeventData, Nip19Entity, NostrUri};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryEventRefFromUri {
    pub key: String,
    pub hints: Vec<String>,
    pub event_author: Option<String>,
}

pub fn event_ref_from_uri(uri: &str) -> Option<GalleryEventRefFromUri> {
    match decode_target(uri.trim())? {
        DecodeTarget::Event {
            event_id,
            relays,
            author,
        } => Some(GalleryEventRefFromUri {
            key: event_id,
            hints: relays,
            event_author: author,
        }),
        DecodeTarget::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => Some(GalleryEventRefFromUri {
            key: format!("{kind}:{pubkey}:{identifier}"),
            hints: relays,
            event_author: None,
        }),
        DecodeTarget::Profile => None,
    }
}

enum DecodeTarget {
    Profile,
    Event {
        event_id: String,
        relays: Vec<String>,
        author: Option<String>,
    },
    Address {
        identifier: String,
        pubkey: String,
        kind: u32,
        relays: Vec<String>,
    },
}

fn decode_target(input: &str) -> Option<DecodeTarget> {
    if input.is_empty() {
        return None;
    }
    if input.starts_with("nostr:") {
        return nip21::parse_nostr_uri(input)
            .ok()
            .map(target_from_nostr_uri);
    }
    nmp_nostr_id::parse(input).ok()?.try_into().ok()
}

impl TryFrom<Nip19Entity> for DecodeTarget {
    type Error = ();

    fn try_from(entity: Nip19Entity) -> Result<Self, Self::Error> {
        match entity {
            Nip19Entity::Nsec(_) => Err(()),
            Nip19Entity::Npub(_) | Nip19Entity::Nprofile(_) => Ok(Self::Profile),
            Nip19Entity::Note(event_id) => Ok(Self::Event {
                event_id,
                relays: Vec::new(),
                author: None,
            }),
            Nip19Entity::Nevent(NeventData {
                event_id,
                relays,
                author,
                ..
            }) => Ok(Self::Event {
                event_id,
                relays,
                author,
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

fn target_from_nostr_uri(target: NostrUri) -> DecodeTarget {
    match target {
        NostrUri::Profile { .. } => DecodeTarget::Profile,
        NostrUri::Event {
            event_id,
            relays,
            author,
            ..
        } => DecodeTarget::Event {
            event_id,
            relays,
            author,
        },
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => DecodeTarget::Address {
            identifier,
            pubkey,
            kind,
            relays,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nostr_id::{encode_naddr, encode_nevent};

    const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    const EVENT_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    #[test]
    fn note_uri_decodes_to_event_key() {
        let nevent = encode_nevent(&NeventData {
            event_id: EVENT_ID.to_string(),
            relays: vec!["wss://relay.example".to_string()],
            author: Some(PUBKEY.to_string()),
            kind: Some(1),
        })
        .unwrap();

        let event_ref = event_ref_from_uri(&format!("nostr:{nevent}")).unwrap();
        assert_eq!(event_ref.key, EVENT_ID);
        assert_eq!(event_ref.hints, ["wss://relay.example"]);
        assert_eq!(event_ref.event_author.as_deref(), Some(PUBKEY));
    }

    #[test]
    fn naddr_decodes_to_coordinate_key() {
        let naddr = encode_naddr(&NaddrData {
            identifier: "article-1".to_string(),
            pubkey: PUBKEY.to_string(),
            kind: 30_023,
            relays: vec!["wss://relay.example".to_string()],
        })
        .unwrap();

        let event_ref = event_ref_from_uri(&naddr).unwrap();
        assert_eq!(event_ref.key, format!("30023:{PUBKEY}:article-1"));
        assert_eq!(event_ref.hints, ["wss://relay.example"]);
        assert_eq!(event_ref.event_author, None);
    }

    #[test]
    fn profile_uri_is_not_an_event_ref() {
        assert!(event_ref_from_uri(PUBKEY).is_none());
    }
}
