//! App-owned `nostr:` URI adapter for Gallery event embeds.
//!
//! The gallery resolves event embeds through the typed event-ref adapters. This
//! module converts a user/content URI into the raw resolver key plus metadata
//! those adapters expect, without exposing a generic stateless C helper.

use nmp_core::nip19::{self, NaddrData, NeventData, Nip19Entity};
use nmp_core::nip21::{self, NostrUri};
use serde::Serialize;
use std::ffi::{c_char, CStr, CString};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GalleryEventRefFromUri {
    pub key: String,
    pub metadata_json: String,
}

#[derive(Serialize)]
struct GalleryEventRefJson<'a> {
    key: &'a str,
    metadata_json: &'a str,
}

#[derive(Serialize)]
struct EventRefMetadata {
    hints: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<u32>,
}

pub fn event_ref_from_uri(uri: &str) -> Option<GalleryEventRefFromUri> {
    match decode_target(uri)? {
        DecodeTarget::Event {
            event_id,
            relays,
            author,
            kind,
        } => event_ref(event_id, relays, author, kind),
        DecodeTarget::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => event_ref(
            format!("{kind}:{pubkey}:{identifier}"),
            relays,
            None,
            Some(kind),
        ),
        DecodeTarget::Profile { .. } => None,
    }
}

fn event_ref(
    key: String,
    hints: Vec<String>,
    author: Option<String>,
    kind: Option<u32>,
) -> Option<GalleryEventRefFromUri> {
    let metadata = EventRefMetadata {
        hints,
        author,
        kind,
    };
    let metadata_json = serde_json::to_string(&metadata).ok()?;
    Some(GalleryEventRefFromUri { key, metadata_json })
}

enum DecodeTarget {
    Profile {
        _pubkey: String,
        _relays: Vec<String>,
    },
    Event {
        event_id: String,
        relays: Vec<String>,
        author: Option<String>,
        kind: Option<u32>,
    },
    Address {
        identifier: String,
        pubkey: String,
        kind: u32,
        relays: Vec<String>,
    },
}

fn decode_target(input: &str) -> Option<DecodeTarget> {
    if input.starts_with("nostr:") {
        return nip21::parse_nostr_uri(input)
            .ok()
            .map(target_from_nostr_uri);
    }
    nip19::parse(input).ok()?.try_into().ok()
}

impl TryFrom<Nip19Entity> for DecodeTarget {
    type Error = ();

    fn try_from(entity: Nip19Entity) -> Result<Self, Self::Error> {
        match entity {
            Nip19Entity::Nsec(_) => Err(()),
            Nip19Entity::Npub(pubkey) => Ok(Self::Profile {
                _pubkey: pubkey,
                _relays: Vec::new(),
            }),
            Nip19Entity::Nprofile(data) => Ok(Self::Profile {
                _pubkey: data.pubkey,
                _relays: data.relays,
            }),
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

fn target_from_nostr_uri(target: NostrUri) -> DecodeTarget {
    match target {
        NostrUri::Profile { pubkey, relays } => DecodeTarget::Profile {
            _pubkey: pubkey,
            _relays: relays,
        },
        NostrUri::Event {
            event_id,
            relays,
            author,
            kind,
        } => DecodeTarget::Event {
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
        } => DecodeTarget::Address {
            identifier,
            pubkey,
            kind,
            relays,
        },
    }
}

/// Decode a Gallery event-embed URI into `{"key":"...","metadata_json":"..."}`.
///
/// Returns NULL on invalid input, non-event targets, serialization failure, or
/// interior-NUL output. The caller owns non-NULL returns and must release them
/// with `nmp_free_string`.
#[cfg(feature = "native")]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_gallery_event_ref_from_uri(uri: *const c_char) -> *mut c_char {
    let Some(uri) = c_string(uri) else {
        return std::ptr::null_mut();
    };
    let Some(event_ref) = event_ref_from_uri(&uri) else {
        return std::ptr::null_mut();
    };
    let Ok(json) = serde_json::to_string(&GalleryEventRefJson {
        key: &event_ref.key,
        metadata_json: &event_ref.metadata_json,
    }) else {
        return std::ptr::null_mut();
    };
    CString::new(json)
        .map(CString::into_raw)
        .unwrap_or(std::ptr::null_mut())
}

#[cfg(feature = "native")]
fn c_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nip19::{encode_naddr, encode_nevent, NaddrData, NeventData};

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
        assert_eq!(
            event_ref.metadata_json,
            format!(
                r#"{{"hints":["wss://relay.example"],"author":"{PUBKEY}","kind":1}}"#
            )
        );
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
        assert_eq!(
            event_ref.metadata_json,
            r#"{"hints":["wss://relay.example"],"kind":30023}"#
        );
    }

    #[test]
    fn profile_uri_is_not_an_event_ref() {
        assert!(event_ref_from_uri(PUBKEY).is_none());
    }
}
