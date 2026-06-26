//! #2085 end-to-end proof: the NIP-65 mailbox cache handle surfaced by
//! `nmp_defaults::register_defaults_with_handles` is the SAME instance the
//! production composition root wires into the kind:10002 parser writer, the
//! routing factory, and the NIP-19 encoder reader.
//!
//! ## Why this test lives in `nmp-testing`
//!
//! Closing the loop needs three crates at once: `nmp-defaults` (the composition
//! root that returns the handle), `nmp-router` (the real `Kind10002Parser`), and
//! `nmp-ffi` (the `nmp_app_encode_profile` reader). Only `nmp-testing` depends on
//! all three, so it is the only place that can prove instance identity across the
//! full pipeline rather than against a hand-wired spy cache.
//!
//! The substrate-tier unit test
//! (`nmp-defaults/tests/substrate_coverage_gate.rs::register_substrate_installs_shared_cache_parser_floor`)
//! already proves `handle == parser-writer == routing-cache` via `Arc::ptr_eq`.
//! This test adds the last edge: `handle == encoder-reader`, exercised through
//! the public FFI encoder, against the real `register_defaults_with_handles`
//! composition root an app-core crate actually calls.

use std::ffi::{c_char, CStr, CString};

use nmp_core::nip19::decode_nprofile;
use nmp_core::substrate::ParsedRelayList;
use nmp_defaults::{register_defaults_with_handles, NmpDefaults};
use nmp_ffi::{nmp_app_encode_profile, nmp_app_free, nmp_app_new, nmp_free_string, NmpApp};

const PUBKEY: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

/// Call `nmp_app_encode_profile`, copy the result, and free the FFI C string.
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
    drop(c_pubkey);
    out
}

#[test]
fn register_defaults_handle_is_the_encoder_read_cache() {
    let app: *mut NmpApp = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new returned null");

    // The composition root an app-core crate calls. It returns the runtime read
    // handles, including the NIP-65 mailbox cache (#2085).
    let handles =
        register_defaults_with_handles(unsafe { &mut *app }, NmpDefaults::default());
    let cache = handles
        .mailbox_cache
        .expect("register_defaults_with_handles must surface the mailbox cache handle");

    // Write a relay list through the handle the app-core crate received. If the
    // handle is the SAME instance the encoder reads, the NIP-19 encoder must now
    // prefer `nprofile` and carry exactly these relays. (`upsert` here stands in
    // for the kind:10002 ingest write; the substrate unit test proves the parser
    // shares this very instance.)
    cache.upsert(
        PUBKEY.to_string(),
        ParsedRelayList {
            read: vec![],
            write: vec![],
            both: vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        },
    );

    let nprofile = encode(app, PUBKEY);
    assert!(
        nprofile.starts_with("nprofile1"),
        "the handle must be the encoder's read cache (instance identity); got {nprofile}"
    );
    let decoded = decode_nprofile(&nprofile).expect("valid nprofile round-trips");
    assert_eq!(decoded.pubkey, PUBKEY);
    assert_eq!(
        decoded.relays,
        vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        "nprofile carries exactly the relays written through the returned handle"
    );

    // The handle also preserves the read/write/both role shape — the exact read
    // an app-core relay-import preview performs.
    let snapshot = cache
        .snapshot(&PUBKEY.to_string())
        .expect("the handle observes its own write");
    assert_eq!(
        snapshot.both,
        vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        "the handle snapshot preserves the both-role relays"
    );

    nmp_app_free(app);
}
