//! NIP-19 profile encoder — UniFFI surface (M14-C1).
//!
//! Replaces the retired C-ABI `nmp_app_encode_profile` door.
//!
//! ## Core-fn provenance
//!
//! This UniFFI wrapper keeps the retired C-ABI helper logic on the public
//! native path and calls the same two `nmp_nip19` primitives:
//! - `encode_npub` — fallback path (no relay hints).
//! - `encode_nprofile` — preferred path (relay hints from `mailbox_cache_reader`).
//!
//! The `encode_profile` fn below preserves the same core semantics on the typed
//! UniFFI surface.
//!
//! ## D6
//!
//! Never returns an error — degrades to a bare `npub1…` on missing relay hints,
//! or echoes the raw input when the pubkey hex is invalid.

use std::sync::Arc;

use nmp_nip19::{encode_nprofile, encode_npub, NprofileData};

use crate::NmpApp;

/// Max relay hints embedded in an `nprofile` TLV string.
/// Matches the retired C-ABI helper cap.
const MAX_NPROFILE_RELAYS: usize = 3;

/// Encode a 64-char hex pubkey as a NIP-19 display identifier.
///
/// Prefers `nprofile1…` (pubkey + relay TLVs) when the runtime already holds
/// kind:10002 relay hints in the mailbox cache. Falls back to a bare `npub1…`
/// when no hints are cached (or when `app` has no mailbox cache configured).
///
/// D6: never throws. An invalid or unrecognisable `pubkey_hex` degrades to
/// returning the raw input string — same fallback as the retired C-ABI helper.
///
/// Preserves the retired C-ABI helper behavior: same mailbox-cache read, same
/// `MAX_NPROFILE_RELAYS` truncation, same D6 fallback chain.
#[uniffi::export]
pub fn encode_profile(app: Arc<NmpApp>, pubkey_hex: String) -> String {
    let relays = app
        .inner
        .mailbox_cache_reader()
        .and_then(|cache| cache.write_relays(&pubkey_hex))
        .filter(|r| !r.is_empty());

    match relays {
        Some(mut relays) => {
            relays.truncate(MAX_NPROFILE_RELAYS);
            let data = NprofileData {
                pubkey: pubkey_hex.clone(),
                relays,
            };
            encode_nprofile(&data).unwrap_or_else(|_| pubkey_hex)
        }
        None => encode_npub(&pubkey_hex).unwrap_or_else(|_| pubkey_hex),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_nip19::{decode_nprofile, decode_npub};

    const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

    // The retired C-ABI helper with no cache called `encode_npub(pubkey)`.
    // These tests verify the UniFFI `encode_profile` with a freshly-constructed
    // (cache-less) NmpApp preserves that output.

    #[test]
    fn parity_no_cache_produces_npub() {
        let app = crate::NmpApp::new();
        let result = encode_profile(app, PUBKEY.to_string());

        // Retired C-ABI parity: no-cache encode_profile also called encode_npub.
        let expected = encode_npub(PUBKEY).unwrap();
        assert_eq!(
            result, expected,
            "UniFFI must preserve the retired C-ABI npub path"
        );
        assert!(
            result.starts_with("npub1"),
            "expected npub1 prefix, got {result}"
        );
    }

    #[test]
    fn parity_no_cache_result_round_trips() {
        let app = crate::NmpApp::new();
        let result = encode_profile(app, PUBKEY.to_string());
        let decoded = decode_npub(&result).unwrap();
        assert_eq!(decoded, PUBKEY);
    }

    #[test]
    fn parity_invalid_pubkey_echoes_raw_input() {
        // D6: invalid hex → every encoder returns Err → fallback echoes input.
        // Retired C-ABI parity: encode_profile(None, "not-a-pubkey") returned
        // "not-a-pubkey".
        let app = crate::NmpApp::new();
        let result = encode_profile(app, "not-a-pubkey".to_string());
        assert_eq!(result, "not-a-pubkey");
    }

    #[test]
    fn with_relay_hints_prefers_nprofile() {
        use nmp_core::substrate::{MailboxCache, ParsedRelayList};
        use std::sync::Arc as StdArc;

        // Stub cache that returns relay hints for our test pubkey.
        struct StubCache {
            pubkey: String,
            relays: Vec<String>,
        }
        impl MailboxCache for StubCache {
            fn read_relays(&self, author: &String) -> Option<Vec<String>> {
                self.write_relays(author)
            }
            fn write_relays(&self, author: &String) -> Option<Vec<String>> {
                (author == &self.pubkey).then(|| self.relays.clone())
            }
            fn snapshot(&self, _: &String) -> Option<ParsedRelayList> {
                None
            }
            fn snapshot_all(&self) -> Vec<(String, ParsedRelayList)> {
                Vec::new()
            }
            fn remove(&self, _: &String) {}
            fn upsert(&self, _: String, _: ParsedRelayList) {}
        }

        let cache = StubCache {
            pubkey: PUBKEY.to_string(),
            relays: vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        };

        // Use Arc<NmpApp> inner to install the cache (same as nip19_ffi_tests.rs).
        let app = crate::NmpApp::new();
        app.inner
            .set_mailbox_cache_reader(StdArc::new(cache) as StdArc<dyn MailboxCache>);

        let result = encode_profile(Arc::clone(&app), PUBKEY.to_string());
        assert!(
            result.starts_with("nprofile1"),
            "expected nprofile1, got {result}"
        );
        let decoded = decode_nprofile(&result).unwrap();
        assert_eq!(decoded.pubkey, PUBKEY);
        assert_eq!(
            decoded.relays,
            vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()]
        );
    }

    #[test]
    fn relay_count_is_capped_at_three() {
        use nmp_core::substrate::{MailboxCache, ParsedRelayList};
        use std::sync::Arc as StdArc;

        struct FiveRelayCache;
        impl MailboxCache for FiveRelayCache {
            fn read_relays(&self, a: &String) -> Option<Vec<String>> {
                self.write_relays(a)
            }
            fn write_relays(&self, _: &String) -> Option<Vec<String>> {
                Some(vec![
                    "wss://r1".into(),
                    "wss://r2".into(),
                    "wss://r3".into(),
                    "wss://r4".into(),
                    "wss://r5".into(),
                ])
            }
            fn snapshot(&self, _: &String) -> Option<ParsedRelayList> {
                None
            }
            fn snapshot_all(&self) -> Vec<(String, ParsedRelayList)> {
                Vec::new()
            }
            fn remove(&self, _: &String) {}
            fn upsert(&self, _: String, _: ParsedRelayList) {}
        }

        let app = crate::NmpApp::new();
        app.inner
            .set_mailbox_cache_reader(StdArc::new(FiveRelayCache) as StdArc<dyn MailboxCache>);

        let result = encode_profile(Arc::clone(&app), PUBKEY.to_string());
        let decoded = decode_nprofile(&result).unwrap();
        assert_eq!(
            decoded.relays.len(),
            MAX_NPROFILE_RELAYS,
            "relay count must be capped at MAX_NPROFILE_RELAYS"
        );
    }

    #[test]
    fn register_defaults_handle_is_the_encoder_read_cache() {
        use nmp_core::substrate::ParsedRelayList;

        let mut app = crate::NmpApp::new();
        let app_mut = Arc::get_mut(&mut app).expect("fresh test app has one Arc owner");
        let handles = nmp_defaults::register_defaults_with_handles(
            &mut app_mut.inner,
            nmp_defaults::NmpDefaults::default(),
        );
        let cache = handles
            .mailbox_cache
            .expect("register_defaults_with_handles must surface the mailbox cache handle");

        cache.upsert(
            PUBKEY.to_string(),
            ParsedRelayList {
                read: Vec::new(),
                write: Vec::new(),
                both: vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
            },
        );

        let result = encode_profile(Arc::clone(&app), PUBKEY.to_string());
        assert!(
            result.starts_with("nprofile1"),
            "returned defaults handle must be the cache encode_profile reads; got {result}"
        );
        let decoded = decode_nprofile(&result).expect("valid nprofile round-trips");
        assert_eq!(decoded.pubkey, PUBKEY);
        assert_eq!(
            decoded.relays,
            vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
            "nprofile carries exactly the relays written through the returned handle"
        );
    }
}
