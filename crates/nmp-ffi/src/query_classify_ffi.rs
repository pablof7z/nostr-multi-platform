//! Stateless "go-to box" query classifier FFI.
//!
//! A search/navigate box accepts ONE pasted-or-typed string and must decide
//! where it points: a profile, an event/thread, a hashtag feed, a NIP-05
//! identifier, or free-text search. Chirp is a thin shell (zero domain logic),
//! so ALL of that classification lives here — the host only switches on the
//! returned `kind`. This mirrors [`crate::nmp_nip21_decode_uri`] (stateless,
//! never routed through the kernel) but is broader: it also recognizes
//! hashtags, NIP-05 identifiers, and free text, and folds the nip19/nip21
//! decode in as one branch.
//!
//! Precedence (first match wins): NIP-19/21 entity → NIP-05 (`name@domain`) →
//! hashtag (`#tag` or a single bare token) → free text. `naddr` (addressable)
//! and `nsec` (secret) are classified `unsupported` rather than routed.

use crate::c_string_argument;
use nmp_core::nip19::{self, NaddrData, NeventData, Nip19Entity, NprofileData};
use nmp_core::nip21::{self, NostrUri};
use serde::Serialize;
use std::ffi::{c_char, CString};

/// The classified destination of a go-to-box query. Serialized with an internal
/// `kind` tag so the host decodes one shape and switches on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QueryClass {
    /// A profile (`npub` / `nprofile`).
    Profile {
        pubkey: String,
        relays: Vec<String>,
    },
    /// An event / thread (`note` / `nevent`). `event_kind` is renamed from the
    /// entity's kind to avoid colliding with the `kind` discriminator tag.
    Event {
        event_id: String,
        relays: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        author: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        event_kind: Option<u32>,
    },
    /// A NIP-12 hashtag feed. `tag` is normalized (lowercased, no leading `#`).
    Hashtag { tag: String },
    /// A NIP-05 identifier (`name@domain`). Resolution to a pubkey is the host's
    /// next step (not yet wired — surfaced so the box can say so).
    Nip05 { identifier: String },
    /// Free-text NIP-50 search.
    #[serde(rename = "search")]
    Freetext { query: String },
    /// A recognized-but-unrouteable input (e.g. `naddr` addressable, `nsec`).
    Unsupported { reason: String },
}

/// Classify a raw go-to-box query string. Pure (no kernel/network); NIP-05 and
/// free-text branches only *recognize* — they do not resolve or search.
#[must_use]
pub fn classify_query(input: &str) -> QueryClass {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return QueryClass::Unsupported {
            reason: "empty".to_string(),
        };
    }

    if let Some(entity) = classify_entity(trimmed) {
        return entity;
    }

    // An `@` means the user meant an identifier, not a tag — route to NIP-05
    // when well-formed, otherwise fall through to free text.
    if trimmed.contains('@') {
        return if looks_like_nip05(trimmed) {
            QueryClass::Nip05 {
                identifier: trimmed.to_lowercase(),
            }
        } else {
            QueryClass::Freetext {
                query: trimmed.to_string(),
            }
        };
    }

    if let Some(tag) = classify_hashtag(trimmed) {
        return QueryClass::Hashtag { tag };
    }

    QueryClass::Freetext {
        query: trimmed.to_string(),
    }
}

/// Decode a `nostr:` URI or bare NIP-19 entity. Returns `None` when the input is
/// not a parseable entity (so the caller falls through to the other branches).
fn classify_entity(input: &str) -> Option<QueryClass> {
    if input.starts_with("nostr:") {
        return Some(match nip21::parse_nostr_uri(input).ok()? {
            NostrUri::Profile { pubkey, relays } => QueryClass::Profile { pubkey, relays },
            NostrUri::Event {
                event_id,
                relays,
                author,
                kind,
            } => QueryClass::Event {
                event_id,
                relays,
                author,
                event_kind: kind,
            },
            NostrUri::Address { .. } => QueryClass::Unsupported {
                reason: "addressable-unsupported".to_string(),
            },
        });
    }

    match nip19::parse(input).ok()? {
        Nip19Entity::Nsec(_) => Some(QueryClass::Unsupported {
            reason: "nsec-forbidden".to_string(),
        }),
        Nip19Entity::Npub(pubkey) => Some(QueryClass::Profile {
            pubkey,
            relays: Vec::new(),
        }),
        Nip19Entity::Nprofile(NprofileData { pubkey, relays }) => {
            Some(QueryClass::Profile { pubkey, relays })
        }
        Nip19Entity::Note(event_id) => Some(QueryClass::Event {
            event_id,
            relays: Vec::new(),
            author: None,
            event_kind: None,
        }),
        Nip19Entity::Nevent(NeventData {
            event_id,
            relays,
            author,
            kind,
        }) => Some(QueryClass::Event {
            event_id,
            relays,
            author,
            event_kind: kind,
        }),
        Nip19Entity::Naddr(NaddrData { .. }) => Some(QueryClass::Unsupported {
            reason: "addressable-unsupported".to_string(),
        }),
    }
}

/// A hashtag is either an explicit `#tag` or a single bare token (no
/// whitespace). Returns the normalized tag (lowercased, no `#`) or `None` when
/// the input has whitespace (→ free text) or normalizes empty.
fn classify_hashtag(input: &str) -> Option<String> {
    let body = input.strip_prefix('#').unwrap_or(input);
    if body.is_empty() || body.chars().any(char::is_whitespace) {
        return None;
    }
    let tag = body.trim_start_matches('#').to_lowercase();
    (!tag.is_empty()).then_some(tag)
}

/// Cheap `local@domain.tld` shape check (no network, no regex dependency):
/// exactly one `@`, a non-empty local part, and a dotted domain whose last
/// label is ≥2 chars. Intentionally permissive — real resolution happens later.
fn looks_like_nip05(input: &str) -> bool {
    let mut parts = input.splitn(2, '@');
    let (Some(local), Some(domain)) = (parts.next(), parts.next()) else {
        return false;
    };
    if local.is_empty() || domain.contains('@') || domain.chars().any(char::is_whitespace) {
        return false;
    }
    let mut labels = domain.split('.');
    let has_dot = domain.contains('.');
    let tld_ok = labels
        .next_back()
        .map(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap_or(false);
    has_dot && tld_ok && domain.split('.').all(|label| !label.is_empty())
}

/// Classify a go-to-box query into bounded app-neutral JSON.
///
/// The returned C string is heap-owned by Rust and MUST be released through
/// `nmp_free_string`. D6: never returns NULL; invalid input degrades to an
/// `{"kind":"unsupported","reason":"empty"}` object.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_search_classify(input: *const c_char) -> *mut c_char {
    let class = match c_string_argument(input) {
        Some(raw) => classify_query(&raw),
        None => QueryClass::Unsupported {
            reason: "invalid-input".to_string(),
        },
    };
    into_c_string(serde_json::to_string(&class).unwrap_or_else(|_| {
        r#"{"kind":"unsupported","reason":"serialization-failed"}"#.to_string()
    }))
}

fn into_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new(r#"{"kind":"unsupported","reason":"serialization-failed"}"#)
            .expect("static JSON contains no NUL")
            .into_raw(),
    }
}

#[cfg(test)]
#[path = "query_classify_ffi_tests.rs"]
mod tests;
