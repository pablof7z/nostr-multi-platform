//! Shared fixtures for the kernel ingest-handler tests: test pubkeys, the
//! unsigned-event builder (`ingest_contacts`/`ingest_timeline_event` never
//! re-verify `sig`), and NIP-01/NIP-65/NIP-17 tag builders.

use super::*;

// 64-char hex pubkeys — `is_hex_pubkey` requires exactly 64 ascii hex digits,
// so the `p`-tag filter in `ingest_contacts` only keeps well-formed values.
pub(super) const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const FOLLOW_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
pub(super) const FOLLOW_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// Build a `NostrEvent` of `kind` for `pubkey` with the supplied tags.
///
/// `sig` is a placeholder — the ingest methods never read it (they run
/// post-verification).
pub(super) fn make_event(
    id: &str,
    pubkey: &str,
    created_at: u64,
    kind: u32,
    tags: Vec<Vec<String>>,
) -> NostrEvent {
    NostrEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind,
        tags,
        content: String::new(),
        sig: String::new(),
    }
}

/// A single NIP-65 `r` tag: `["r", url]` or `["r", url, marker]`.
///
/// Retained for the commented-out V-40 migration block below (the live
/// equivalent now lives in `crates/nmp-router/src/ingest.rs`).
#[allow(dead_code)]
pub(super) fn r_tag(url: &str, marker: Option<&str>) -> Vec<String> {
    match marker {
        Some(m) => vec!["r".to_string(), url.to_string(), m.to_string()],
        None => vec!["r".to_string(), url.to_string()],
    }
}

/// A single kind:3 `p` tag: `["p", pubkey]`.
pub(super) fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

/// A single NIP-17 kind:10050 `relay` tag: `["relay", url]`.
///
/// Retained for the commented-out V-40 migration block below (the live
/// equivalent now lives in `crates/nmp-nip17/src/kind10050_parser.rs`).
#[allow(dead_code)]
pub(super) fn relay_tag(url: &str) -> Vec<String> {
    vec!["relay".to_string(), url.to_string()]
}

/// Build one real Schnorr-signed kind:1 event in the `NostrEvent` shape the
/// kernel ingest path consumes after JSON decoding.
///
/// `ingest_timeline_event` routes through `store.insert` →
/// `VerifiedEvent::try_from_raw`, which performs real signature verification —
/// the unsigned `make_event` fixture would be dropped at that gate, so timeline
/// tests must sign. Mirrors `clock_injection_tests.rs::signed_note`; the
/// `expect` cannot fail with a freshly-generated keypair.
pub(super) fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}
