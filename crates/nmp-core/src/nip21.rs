//! NIP-21: `nostr:` URI scheme.
//!
//! Wraps NIP-19 entity parsing behind the canonical `nostr:` URI prefix.
//! `nsec` identifiers are intentionally **excluded** from `nostr:` handling
//! per the spec: "except nsec".
//!
//! # Example
//! ```
//! use nmp_core::nip21::{parse_nostr_uri, NostrUri};
//!
//! let uri = "nostr:npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
//! let target = parse_nostr_uri(uri).unwrap();
//! assert!(matches!(target, NostrUri::Profile { .. }));
//! ```

use crate::nip19::{self, NaddrData, NeventData, Nip19Entity, Nip19Error, NprofileData};

const SCHEME: &str = "nostr:";

/// Canonical routing target for a parsed `nostr:` URI.
#[derive(Debug, Clone, PartialEq)]
pub enum NostrUri {
    /// A user profile identified by pubkey (hex), with optional relay hints.
    ///
    /// Source entities: `npub` (no relays) and `nprofile` (with relays).
    Profile {
        /// 32-byte pubkey as a lowercase hex string.
        pubkey: String,
        /// Zero or more relay hints.
        relays: Vec<String>,
    },
    /// A note/event identified by event id (hex), with optional relay hints.
    ///
    /// Source entities: `note` (no relays) and `nevent` (with relays/author/kind).
    Event {
        /// 32-byte event id as a lowercase hex string.
        event_id: String,
        /// Zero or more relay hints.
        relays: Vec<String>,
        /// Optional author pubkey (hex).
        author: Option<String>,
        /// Optional event kind.
        kind: Option<u32>,
    },
    /// An addressable / parameterised-replaceable event coordinate.
    ///
    /// Source entity: `naddr`.
    Address {
        /// The `d` tag identifier.
        identifier: String,
        /// Author pubkey (hex).
        pubkey: String,
        /// Event kind.
        kind: u32,
        /// Zero or more relay hints.
        relays: Vec<String>,
    },
}

/// Errors produced by NIP-21 URI parsing.
#[derive(Debug, PartialEq)]
pub enum Nip21Error {
    /// The URI does not start with `nostr:`.
    MissingScheme,
    /// `nsec` entities are not allowed in `nostr:` URIs per the spec.
    NsecForbidden,
    /// The NIP-19 entity portion could not be decoded.
    Nip19(Nip19Error),
}

impl std::fmt::Display for Nip21Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingScheme => write!(f, "URI must start with 'nostr:'"),
            Self::NsecForbidden => write!(f, "nsec entities are not permitted in nostr: URIs"),
            Self::Nip19(e) => write!(f, "NIP-19 error: {e}"),
        }
    }
}

impl std::error::Error for Nip21Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Nip19(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Nip19Error> for Nip21Error {
    fn from(e: Nip19Error) -> Self {
        Self::Nip19(e)
    }
}

/// Parse a `nostr:` URI string into a [`NostrUri`] routing target.
///
/// Returns `Err(Nip21Error::MissingScheme)` if the prefix is absent.
/// Returns `Err(Nip21Error::NsecForbidden)` if the entity is an `nsec`.
///
/// # Example — nprofile
/// ```
/// use nmp_core::nip21::{parse_nostr_uri, NostrUri};
///
/// let uri = "nostr:nprofile1qqsrhuxx8l9ex335q7he0f09aej04zpazpl0ne2cgukyawd24mayt8gpp4mhxue69uhhytnc9e3k7mgpz4mhxue69uhkg6nzv9ejuumpv34kytnrdaksjlyr9p";
/// let target = parse_nostr_uri(uri).unwrap();
/// if let NostrUri::Profile { pubkey, relays } = target {
///     assert_eq!(pubkey.len(), 64);
///     assert!(!relays.is_empty());
/// }
/// ```
#[must_use]
pub fn parse_nostr_uri(uri: &str) -> Result<NostrUri, Nip21Error> {
    let bech = uri.strip_prefix(SCHEME).ok_or(Nip21Error::MissingScheme)?;
    match nip19::parse(bech)? {
        Nip19Entity::Nsec(_) => Err(Nip21Error::NsecForbidden),
        Nip19Entity::Npub(pubkey) => Ok(NostrUri::Profile {
            pubkey,
            relays: vec![],
        }),
        Nip19Entity::Nprofile(NprofileData { pubkey, relays }) => {
            Ok(NostrUri::Profile { pubkey, relays })
        }
        Nip19Entity::Note(event_id) => Ok(NostrUri::Event {
            event_id,
            relays: vec![],
            author: None,
            kind: None,
        }),
        Nip19Entity::Nevent(NeventData {
            event_id,
            relays,
            author,
            kind,
        }) => Ok(NostrUri::Event {
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
        }) => Ok(NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        }),
    }
}

/// Format a [`NostrUri`] back to a canonical `nostr:` URI string.
///
/// The inverse of [`parse_nostr_uri`].
#[must_use]
pub fn format_nostr_uri(target: &NostrUri) -> Result<String, Nip19Error> {
    let entity = match target {
        NostrUri::Profile { pubkey, relays } => {
            if relays.is_empty() {
                Nip19Entity::Npub(pubkey.clone())
            } else {
                Nip19Entity::Nprofile(NprofileData {
                    pubkey: pubkey.clone(),
                    relays: relays.clone(),
                })
            }
        }
        NostrUri::Event {
            event_id,
            relays,
            author,
            kind,
        } => {
            if relays.is_empty() && author.is_none() && kind.is_none() {
                Nip19Entity::Note(event_id.clone())
            } else {
                Nip19Entity::Nevent(NeventData {
                    event_id: event_id.clone(),
                    relays: relays.clone(),
                    author: author.clone(),
                    kind: *kind,
                })
            }
        }
        NostrUri::Address {
            identifier,
            pubkey,
            kind,
            relays,
        } => Nip19Entity::Naddr(NaddrData {
            identifier: identifier.clone(),
            pubkey: pubkey.clone(),
            kind: *kind,
            relays: relays.clone(),
        }),
    };
    Ok(format!("{SCHEME}{}", nip19::format(&entity)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nip19::{encode_note, encode_nprofile, encode_npub, encode_nsec};

    /// Deterministic 32-byte hex fixture (matches the module doctests).
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    /// A second distinct deterministic 32-byte hex fixture (event id).
    const ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    // ─── happy paths — routing targets ─────────────────────────────────────

    #[test]
    fn npub_uri_routes_to_profile_target() {
        let uri = format!("nostr:{}", encode_npub(PK).unwrap());
        let target = parse_nostr_uri(&uri).unwrap();
        assert_eq!(
            target,
            NostrUri::Profile {
                pubkey: PK.into(),
                relays: vec![],
            }
        );
    }

    #[test]
    fn note_uri_routes_to_event_target() {
        let uri = format!("nostr:{}", encode_note(ID).unwrap());
        let target = parse_nostr_uri(&uri).unwrap();
        assert_eq!(
            target,
            NostrUri::Event {
                event_id: ID.into(),
                relays: vec![],
                author: None,
                kind: None,
            }
        );
    }

    #[test]
    fn nprofile_uri_routes_to_profile_with_relay_hints() {
        // nprofile carries TLV relay hints — the variant the task calls out.
        let bech = encode_nprofile(&NprofileData {
            pubkey: PK.into(),
            relays: vec!["wss://relay.example".into()],
        })
        .unwrap();
        let target = parse_nostr_uri(&format!("nostr:{bech}")).unwrap();
        assert_eq!(
            target,
            NostrUri::Profile {
                pubkey: PK.into(),
                relays: vec!["wss://relay.example".into()],
            }
        );
    }

    // ─── error paths — graceful, no panic ──────────────────────────────────

    #[test]
    fn missing_scheme_errors_not_panic() {
        // A bare NIP-19 entity (no `nostr:` prefix) is not a NIP-21 URI.
        let bare = encode_npub(PK).unwrap();
        assert_eq!(parse_nostr_uri(&bare), Err(Nip21Error::MissingScheme));
    }

    #[test]
    fn nsec_uri_is_forbidden() {
        // `nostr:` URIs must reject nsec per the NIP-21 spec ("except nsec").
        let uri = format!("nostr:{}", encode_nsec(PK).unwrap());
        assert_eq!(parse_nostr_uri(&uri), Err(Nip21Error::NsecForbidden));
    }

    #[test]
    fn unknown_scheme_entity_falls_back_gracefully() {
        // A `nostr:`-prefixed but unrecognised entity must surface a typed
        // Nip19 error, never panic or silently route somewhere.
        let target = parse_nostr_uri("nostr:xyz123notreal");
        assert!(matches!(target, Err(Nip21Error::Nip19(_))));
    }

    #[test]
    fn garbage_after_scheme_errors_not_panic() {
        // Non-bech32 garbage behind a valid scheme is a graceful error.
        let target = parse_nostr_uri("nostr:!!!not-bech32!!!");
        assert!(matches!(target, Err(Nip21Error::Nip19(_))));
    }
}
