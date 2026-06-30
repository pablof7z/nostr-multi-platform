use serde_json::Value;
use std::collections::BTreeSet;

use mdk_core::prelude::GroupId;
use nostr::{PublicKey, RelayUrl};

use crate::projection::state::{parse_signed_event, InnerHandle};

/// Decode a hex MLS group id into a `GroupId`.
pub(super) fn group_id_from_hex(hex: &str) -> Result<GroupId, String> {
    let bytes = decode_hex(hex).ok_or_else(|| "group_id_hex is not valid hex".to_string())?;
    Ok(GroupId::from_slice(&bytes))
}

pub(super) fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Resolve the invitee npub list from EITHER the typed array
/// (`invitee_npubs`) OR a free-form text field (`invitee_text`) the UI
/// captures verbatim. Splits on whitespace, comma, semicolon, newline;
/// trims each token; drops empties. Validation (npub/hex parse) stays in
/// the per-op pipeline — this is just the input-adapter step Rust owns
/// per aim.md §4.5 / §6.
pub(super) fn resolve_invitees(
    invitee_text: Option<&str>,
    invitee_npubs: Option<&[String]>,
) -> Vec<String> {
    if let Some(arr) = invitee_npubs {
        if !arr.is_empty() {
            return arr.to_vec();
        }
    }
    let Some(text) = invitee_text else {
        return Vec::new();
    };
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(super) fn parse_pubkeys(npubs: &[String]) -> Result<Vec<PublicKey>, String> {
    npubs
        .iter()
        .map(|s| PublicKey::parse(s).map_err(|e| format!("bad pubkey `{s}`: {e}")))
        .collect()
}

pub(super) fn parse_relays(urls: &[String]) -> Result<Vec<RelayUrl>, String> {
    urls.iter()
        .map(|s| RelayUrl::parse(s).map_err(|e| format!("bad relay `{s}`: {e}")))
        .collect()
}

/// Resolve the write-relay set for relay-bearing ops.
///
/// The app-wired NIP-65 write relays (`h.write_relay_urls()`) are authoritative
/// for hosted runtime paths. When the projection is driven without an app wired
/// (for example in direct protocol tests), fall back to the envelope `relays`
/// array. This keeps `nmp-marmot::projection` host-agnostic: relays come from
/// the kernel when available, otherwise from the caller's op envelope.
pub(super) fn resolve_write_relays(h: &InnerHandle<'_>, relays: &[String]) -> Vec<String> {
    let app_relays = h.write_relay_urls();
    if !app_relays.is_empty() {
        return app_relays;
    }
    relays.to_vec()
}

/// Pull `signed_key_package_events_json` (array of signed kind:30443
/// event JSON strings OR objects) — the KeyPackage-cache seam escape hatch.
pub(super) fn signed_key_package_events(arr: &[Value]) -> Result<Vec<nostr::Event>, String> {
    let mut out = Vec::with_capacity(arr.len());
    for item in arr.iter().cloned() {
        let json = match item {
            Value::String(s) => s,
            other => {
                serde_json::to_string(&other).map_err(|e| format!("re-encode kp event: {e}"))?
            }
        };
        out.push(parse_signed_event(&json)?);
    }
    Ok(out)
}

pub(super) fn fill_key_packages_from_cache(
    h: &InnerHandle<'_>,
    invitee_npubs: &[String],
    kp_events: &mut Vec<nostr::Event>,
) -> (Vec<String>, Vec<PublicKey>) {
    let valid_pubkeys = invitee_npubs
        .iter()
        .filter_map(|s| PublicKey::parse(s).ok())
        .collect::<Vec<_>>();
    let cached = h.service().cached_key_packages(&valid_pubkeys);
    let mut present = kp_events
        .iter()
        .map(|event| event.pubkey.to_hex())
        .collect::<BTreeSet<_>>();
    for event in cached {
        if present.insert(event.pubkey.to_hex()) {
            kp_events.push(event);
        }
    }

    let mut needs = Vec::new();
    let mut fetch_pubkeys = Vec::new();
    for invitee in invitee_npubs {
        match PublicKey::parse(invitee) {
            Ok(pk) if present.contains(&pk.to_hex()) => {}
            Ok(pk) => {
                needs.push(invitee.clone());
                fetch_pubkeys.push(pk);
            }
            Err(_) => needs.push(invitee.clone()),
        }
    }
    (needs, fetch_pubkeys)
}
