//! H4 make-or-break proof: `nmp_app_encode_profile` prefers `nprofile` from
//! kind:10002 relay hints, and falls back to `npub` otherwise.
//!
//! ## Why this test lives in `nmp-testing`
//!
//! The whole point of H4 is **mailbox-cache instance identity**: the encoder
//! must read the SAME `InMemoryMailboxCache` that the production
//! `nmp_router::Kind10002Parser` writes on kind:10002 ingest. `nmp-ffi` does
//! NOT depend on `nmp-router`, so the only place that can exercise the real
//! ingest pipeline AND the FFI encoder against ONE shared cache instance is
//! `nmp-testing` (it depends on both).
//!
//! The two assertions below are the ballgame:
//! 1. A real kind:10002 for pubkey P fed through `Kind10002Parser` into a
//!    cache installed via `set_mailbox_cache_reader` → `nmp_app_encode_profile(P)`
//!    returns `nprofile1…` decoding back to P with the expected relays.
//! 2. A different pubkey with no kind:10002 → `npub1…` decoding back to it.
//!
//! If assertion (1) fails, the cache instance is NOT shared — the nprofile
//! branch is silently dead and the helper always returns npub.

use std::ffi::{c_char, CStr, CString};
use std::sync::Arc;

use nmp_core::nip19::{decode_nprofile, decode_npub};
use nmp_store::{RawEvent, VerifiedEvent};
use nmp_router::{InMemoryMailboxCache, Kind10002Parser};
use nmp_ffi::{nmp_app_encode_profile, nmp_app_free, nmp_app_new, nmp_free_string, NmpApp};

const PUBKEY_WITH_RELAYS: &str =
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const PUBKEY_NO_RELAYS: &str =
    "0000000000000000000000000000000000000000000000000000000000000001";

/// Build a real kind:10002 (NIP-65) relay-list event with the given `r` tags.
fn kind_10002(pubkey: &str, relays: &[&str]) -> VerifiedEvent {
    let tags: Vec<Vec<String>> = relays
        .iter()
        .map(|url| vec!["r".to_string(), (*url).to_string()])
        .collect();
    VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "00".repeat(32),
        pubkey: pubkey.to_string(),
        created_at: 1_700_000_000,
        kind: 10_002,
        tags,
        content: String::new(),
        sig: "ab".repeat(64),
    })
}

/// Call `nmp_app_encode_profile`, copy the result to an owned `String`, and
/// free the C string the FFI handed back (`nmp_free_string`).
fn encode(app: *mut NmpApp, pubkey: &str) -> String {
    let c_pubkey = CString::new(pubkey).expect("hex has no interior NUL");
    let ptr: *mut c_char = nmp_app_encode_profile(app, c_pubkey.as_ptr());
    assert!(!ptr.is_null(), "encoder never returns NULL (D6)");
    // SAFETY: `ptr` is a valid heap C string minted by `nmp_app_encode_profile`.
    let out = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("encoder output is valid UTF-8")
        .to_owned();
    nmp_free_string(ptr);
    // `c_pubkey` is held live across the FFI call by being named here.
    drop(c_pubkey);
    out
}

#[test]
fn encode_profile_prefers_nprofile_from_kind10002_then_npub_fallback() {
    // ── Make-or-break wiring: ONE cache instance, shared three ways. ──────
    // (Here the parser-write side and the FFI-read side; the routing factory
    // is the third clone in the production composition root.) If these were
    // two different `InMemoryMailboxCache`s, branch (1) would silently fall
    // back to npub.
    let cache: Arc<InMemoryMailboxCache> = Arc::new(InMemoryMailboxCache::new());

    // Production ingest path: feed a real kind:10002 through the parser, which
    // upserts the resolved relay list into the shared cache.
    let parser = Kind10002Parser::new(Arc::clone(&cache));
    parser.parse_event(&kind_10002(
        PUBKEY_WITH_RELAYS,
        &["wss://relay.one", "wss://relay.two"],
    ));

    // Install the SAME Arc as the FFI encoder's read side.
    let app: *mut NmpApp = nmp_app_new();
    // SAFETY: `app` is a valid handle from `nmp_app_new`.
    let app_ref: &NmpApp = unsafe { &*app };
    app_ref.set_mailbox_cache_reader(
        Arc::clone(&cache) as Arc<dyn nmp_core::substrate::MailboxCache>,
    );

    // ── Assertion 1 — nprofile branch (the ballgame). ────────────────────
    let nprofile = encode(app, PUBKEY_WITH_RELAYS);
    assert!(
        nprofile.starts_with("nprofile1"),
        "expected nprofile (cache instance must be shared); got {nprofile}"
    );
    let decoded = decode_nprofile(&nprofile).expect("valid nprofile round-trips");
    assert_eq!(decoded.pubkey, PUBKEY_WITH_RELAYS);
    assert_eq!(
        decoded.relays,
        vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        "nprofile carries exactly the kind:10002 write relays"
    );

    // ── Assertion 2 — npub fallback for an author with no kind:10002. ────
    let npub = encode(app, PUBKEY_NO_RELAYS);
    assert!(
        npub.starts_with("npub1"),
        "no kind:10002 → bare npub; got {npub}"
    );
    assert_eq!(decode_npub(&npub).expect("valid npub round-trips"), PUBKEY_NO_RELAYS);

    nmp_app_free(app);
}
