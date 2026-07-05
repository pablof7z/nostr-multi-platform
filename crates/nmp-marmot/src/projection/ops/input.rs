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

/// The `d` tag value of a kind:30443 key-package event, if present.
pub(super) fn key_package_d_tag(event: &nostr::Event) -> Option<&str> {
    event.tags.iter().find_map(|t| {
        let slice = t.as_slice();
        (slice.first().map(String::as_str) == Some("d"))
            .then(|| slice.get(1).map(String::as_str))
            .flatten()
    })
}

/// #3057 round-7 — select the FRESHEST key package per invitee.
///
/// A peer's relay history commonly holds MULTIPLE kind:30443 events (stale ones
/// from prior sessions under distinct `d` tags + the current one). Only the
/// LATEST publish matches the private half in the peer's live MLS store, so the
/// Welcome MUST be built against it — otherwise the invitee's `process_welcome`
/// fails "No matching key package was found in the key store". This dedupes
/// `kp_events` by author, keeping the max-`created_at` event (covers both the
/// in-memory cache path and an explicit / LMDB-queried key package the host may
/// pass), and logs the selected key package (id + `d` tag) — the nak-provable
/// selection point that feeds `mdk_core::key_packages::decode`.
pub(super) fn select_freshest_key_packages(kp_events: Vec<nostr::Event>) -> Vec<nostr::Event> {
    use std::collections::hash_map::Entry;
    use std::collections::HashMap;
    let mut by_author: HashMap<String, nostr::Event> = HashMap::new();
    for ev in kp_events {
        match by_author.entry(ev.pubkey.to_hex()) {
            Entry::Occupied(mut slot) => {
                if ev.created_at > slot.get().created_at {
                    slot.insert(ev);
                }
            }
            Entry::Vacant(slot) => {
                slot.insert(ev);
            }
        }
    }
    let selected: Vec<nostr::Event> = by_author.into_values().collect();
    for ev in &selected {
        tracing::info!(
            target: "nmp_marmot::publish",
            invitee = %ev.pubkey.to_hex(),
            key_package_id = %ev.id.to_hex(),
            d_tag = key_package_d_tag(ev).unwrap_or("<none>"),
            created_at = ev.created_at.as_secs(),
            "welcome publish: SELECTED invitee key package (newest by created_at) to build the Welcome against"
        );
    }
    selected
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
