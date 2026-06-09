//! Unit coverage for the H4 NIP-19 identity encoder.
//!
//! The make-or-break instance-identity proof (relay hints ingested through
//! the production `nmp_router::Kind10002Parser` and read back out by
//! `nmp_app_encode_profile`) lives in `nmp-testing`, which can depend on both
//! `nmp-router` and `nmp-ffi`. `nmp-ffi` itself does NOT depend on
//! `nmp-router`, so here we cover the encoder's branches against a hand-built
//! `MailboxCache` stub installed on a REAL `NmpApp` handle — exercising the
//! actual `encode_profile` core (cache read → truncate → encode), not a copy.

use super::encode_profile;
use crate::{nmp_app_free, nmp_app_new, NmpApp};
use nmp_core::nip19::{decode_npub, decode_nprofile};
use nmp_core::substrate::{
    MailboxCache, ParsedRelayList, RoutingPubkey as Pubkey, RoutingRelayUrl as RelayUrl,
};
use std::sync::Arc;

const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

/// Minimal in-test `MailboxCache` that returns a fixed write set for one
/// pubkey. Stands in for `nmp_router::InMemoryMailboxCache` (out of reach
/// from this crate's dep graph); the production-pipeline proof lives in
/// `nmp-testing`.
struct StubCache {
    pubkey: Pubkey,
    relays: Vec<RelayUrl>,
}

impl MailboxCache for StubCache {
    fn read_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        self.write_relays(author)
    }
    fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
        (author == &self.pubkey).then(|| self.relays.clone())
    }
    fn snapshot(&self, _author: &Pubkey) -> Option<ParsedRelayList> {
        None
    }
    fn snapshot_all(&self) -> Vec<(Pubkey, ParsedRelayList)> {
        Vec::new()
    }
    fn remove(&self, _author: &Pubkey) {}
    fn upsert(&self, _author: Pubkey, _list: ParsedRelayList) {}
}

/// Build a real `NmpApp` with the given stub cache installed as the encoder's
/// read side. Runs the closure against the real `encode_profile` core, then
/// frees the handle (the encoder is a synchronous cache read — no actor).
fn with_app_cache<R>(relays: &[&str], f: impl FnOnce(&NmpApp) -> R) -> R {
    let cache = StubCache {
        pubkey: PUBKEY.to_string(),
        relays: relays.iter().map(|r| (*r).to_string()).collect(),
    };
    let app: *mut NmpApp = nmp_app_new();
    // SAFETY: `app` is a valid handle from `nmp_app_new`.
    let app_ref: &NmpApp = unsafe { &*app };
    app_ref.set_mailbox_cache_reader(Arc::new(cache) as Arc<dyn MailboxCache>);
    let out = f(app_ref);
    nmp_app_free(app);
    out
}

#[test]
fn no_cache_falls_back_to_npub() {
    // `None` app handle == no installed mailbox-cache reader.
    let out = encode_profile(None, PUBKEY);
    assert!(out.starts_with("npub1"), "expected npub, got {out}");
    assert_eq!(decode_npub(&out).unwrap(), PUBKEY);
}

#[test]
fn relay_hints_present_prefers_nprofile() {
    let out = with_app_cache(&["wss://relay.one", "wss://relay.two"], |app| {
        encode_profile(Some(app), PUBKEY)
    });
    assert!(out.starts_with("nprofile1"), "expected nprofile, got {out}");
    let decoded = decode_nprofile(&out).unwrap();
    assert_eq!(decoded.pubkey, PUBKEY);
    assert_eq!(
        decoded.relays,
        vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()]
    );
}

#[test]
fn empty_relay_set_falls_back_to_npub() {
    let out = with_app_cache(&[], |app| encode_profile(Some(app), PUBKEY));
    assert!(out.starts_with("npub1"), "expected npub, got {out}");
    assert_eq!(decode_npub(&out).unwrap(), PUBKEY);
}

#[test]
fn over_three_relays_are_truncated() {
    let out = with_app_cache(
        &["wss://r1", "wss://r2", "wss://r3", "wss://r4", "wss://r5"],
        |app| encode_profile(Some(app), PUBKEY),
    );
    let decoded = decode_nprofile(&out).unwrap();
    assert_eq!(decoded.relays.len(), super::MAX_NPROFILE_RELAYS);
}

#[test]
fn unknown_pubkey_in_cache_falls_back_to_npub() {
    let other = "00".repeat(31) + "01";
    let out = with_app_cache(&["wss://relay.one"], |app| {
        encode_profile(Some(app), &other)
    });
    assert!(out.starts_with("npub1"), "expected npub, got {out}");
    assert_eq!(decode_npub(&out).unwrap(), other);
}

#[test]
fn invalid_pubkey_echoes_raw_input() {
    // Not 64-char hex → every encoder errors → D6 raw-echo fallback.
    let out = encode_profile(None, "not-a-pubkey");
    assert_eq!(out, "not-a-pubkey");
}
