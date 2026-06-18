//! Stateless NIP-21 / bare NIP-19 decode FFI.
//!
//! This is intentionally not routed through `KernelAction::OpenUri`: opening a
//! URI mutates kernel/view state, while this symbol only classifies a share
//! target for hosts that need decode-only behavior.

use crate::c_string_argument;
use nmp_core::nip19::{self, NaddrData, NeventData, Nip19Entity, NprofileData};
use nmp_core::nip21::{self, Nip21Error, NostrUri};
use serde::Serialize;
use std::ffi::{c_char, CString};

#[derive(Serialize)]
#[serde(tag = "target", rename_all = "snake_case")]
enum DecodeTarget {
    Profile {
        pubkey: String,
        relays: Vec<String>,
    },
    Event {
        event_id: String,
        relays: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        author: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind: Option<u32>,
    },
    Address {
        identifier: String,
        pubkey: String,
        kind: u32,
        relays: Vec<String>,
    },
}

#[derive(Serialize)]
struct DecodeSuccess {
    ok: bool,
    #[serde(flatten)]
    target: DecodeTarget,
}

#[derive(Serialize)]
struct DecodeError {
    ok: bool,
    error: &'static str,
}

/// Decode a `nostr:` URI or bare NIP-19 entity into bounded app-neutral JSON.
///
/// Accepted targets are profile (`npub`/`nprofile`), event (`note`/`nevent`),
/// and address (`naddr`). Secret-bearing `nsec` values are rejected as data.
///
/// The returned C string is heap-owned by Rust and MUST be released through
/// `nmp_free_string`. D6: never returns NULL; malformed input returns a small
/// `{"ok":false,"error":"..."}` object.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_nip21_decode_uri(input: *const c_char) -> *mut c_char {
    let value = c_string_argument(input);
    let output = match value.as_deref() {
        Some(raw) => decode_uri_json(raw),
        None => error_json("invalid-input"),
    };
    into_c_string(output)
}

fn decode_uri_json(input: &str) -> String {
    match decode_uri(input) {
        Ok(target) => success_json(target),
        Err(error) => error_json(error),
    }
}

fn decode_uri(input: &str) -> Result<DecodeTarget, &'static str> {
    if input.starts_with("nostr:") {
        return nip21::parse_nostr_uri(input)
            .map(target_from_nostr_uri)
            .map_err(error_code);
    }

    nip19::parse(input).map_err(|_| "unparseable")?.try_into()
}

impl TryFrom<Nip19Entity> for DecodeTarget {
    type Error = &'static str;

    fn try_from(entity: Nip19Entity) -> Result<Self, Self::Error> {
        match entity {
            Nip19Entity::Nsec(_) => Err("nsec-forbidden"),
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

fn target_from_nostr_uri(target: NostrUri) -> DecodeTarget {
    match target {
        NostrUri::Profile { pubkey, relays } => DecodeTarget::Profile { pubkey, relays },
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

fn error_code(error: Nip21Error) -> &'static str {
    match error {
        Nip21Error::NsecForbidden => "nsec-forbidden",
        Nip21Error::MissingScheme | Nip21Error::Nip19(_) => "unparseable",
    }
}

fn success_json(target: DecodeTarget) -> String {
    serde_json::to_string(&DecodeSuccess { ok: true, target })
        .unwrap_or_else(|_| error_json("serialization-failed"))
}

fn error_json(error: &'static str) -> String {
    serde_json::to_string(&DecodeError { ok: false, error })
        .unwrap_or_else(|_| r#"{"ok":false,"error":"serialization-failed"}"#.to_string())
}

fn into_c_string(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"error":"serialization-failed"}"#)
            .expect("static JSON contains no NUL")
            .into_raw(),
    }
}

#[cfg(test)]
#[path = "nip21_ffi_tests.rs"]
mod tests;
