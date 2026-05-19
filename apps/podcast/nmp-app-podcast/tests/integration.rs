//! Integration tests for the podcast app FFI surface.
//!
//! These tests exercise the full path from FFI entry point to domain-store
//! persistence and back out through the snapshot. They verify the contract
//! the iOS and Android shells depend on:
//!
//! * `nmp_app_podcast_subscribe` → domain store row exists.
//! * `nmp_app_podcast_snapshot` → `LibraryView` JSON with the subscribed row.
//! * `nmp_app_podcast_unsubscribe` → row removed, snapshot is empty.
//!
//! T-podcast-gap-1: these are the integration tests required by the gap
//! resolution spec — subscribe a podcast, observe the row appear in the
//! kernel ViewModule snapshot (the domain-store-backed state, not the
//! retired `Mutex<Vec<PodcastRecord>>`).

use std::ffi::{CStr, CString};

use nmp_app_podcast::{
    nmp_app_podcast_register, nmp_app_podcast_snapshot, nmp_app_podcast_snapshot_free,
    nmp_app_podcast_subscribe, nmp_app_podcast_unregister, nmp_app_podcast_unsubscribe,
};
use nmp_core::{nmp_app_free, nmp_app_new};
use podcast_core::views::LibraryView;

fn cstr(s: &str) -> CString {
    CString::new(s).expect("no internal nul")
}

fn decode_snapshot(ptr: *mut std::ffi::c_char) -> LibraryView {
    assert!(!ptr.is_null(), "snapshot returned null");
    let json = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .expect("valid UTF-8")
        .to_owned();
    nmp_app_podcast_snapshot_free(ptr);
    serde_json::from_str(&json).expect("valid LibraryView JSON")
}

/// Core gap-1 contract: subscribe via FFI → row appears in snapshot.
#[test]
fn subscribe_via_ffi_row_appears_in_domain_backed_snapshot() {
    let app = nmp_app_new();
    let handle = nmp_app_podcast_register(app);
    assert!(!handle.is_null());

    // Before subscribe: empty library.
    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert!(view.podcasts.is_empty(), "initial library must be empty");

    // Subscribe a podcast.
    let feed = cstr("https://feeds.megaphone.fm/lex-fridman");
    let title = cstr("Lex Fridman Podcast");
    let author = cstr("Lex Fridman");
    nmp_app_podcast_subscribe(handle, feed.as_ptr(), title.as_ptr(), author.as_ptr());

    // Domain-store-backed snapshot must reflect the new row.
    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert_eq!(view.podcasts.len(), 1, "one podcast after subscribe");
    assert_eq!(view.podcasts[0].title, "Lex Fridman Podcast");
    assert_eq!(view.podcasts[0].author, "Lex Fridman");
    assert!(!view.podcasts[0].id.is_empty(), "podcast id must be populated");

    nmp_app_podcast_unregister(handle);
    nmp_app_free(app);
}

/// Unsubscribe via FFI → row removed from domain-store-backed snapshot.
#[test]
fn unsubscribe_via_ffi_row_removed_from_snapshot() {
    let app = nmp_app_new();
    let handle = nmp_app_podcast_register(app);

    let feed = cstr("https://changelog.com/gotime/feed");
    nmp_app_podcast_subscribe(handle, feed.as_ptr(), std::ptr::null(), std::ptr::null());

    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert_eq!(view.podcasts.len(), 1);
    let podcast_id = view.podcasts[0].id.clone();

    // Unsubscribe by the ULID returned in the snapshot row.
    let id_cstr = cstr(&podcast_id);
    nmp_app_podcast_unsubscribe(handle, id_cstr.as_ptr());

    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert!(view.podcasts.is_empty(), "library must be empty after unsubscribe");

    nmp_app_podcast_unregister(handle);
    nmp_app_free(app);
}

/// Two separate subscribes to different feeds must produce two rows.
#[test]
fn two_subscribes_produce_two_rows() {
    let app = nmp_app_new();
    let handle = nmp_app_podcast_register(app);

    let feed1 = cstr("https://feeds.example.com/show-a.xml");
    let feed2 = cstr("https://feeds.example.com/show-b.xml");
    let title1 = cstr("Show A");
    let title2 = cstr("Show B");

    nmp_app_podcast_subscribe(handle, feed1.as_ptr(), title1.as_ptr(), std::ptr::null());
    nmp_app_podcast_subscribe(handle, feed2.as_ptr(), title2.as_ptr(), std::ptr::null());

    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert_eq!(view.podcasts.len(), 2, "two distinct podcasts");

    let titles: Vec<&str> = view.podcasts.iter().map(|p| p.title.as_str()).collect();
    assert!(titles.contains(&"Show A"));
    assert!(titles.contains(&"Show B"));

    nmp_app_podcast_unregister(handle);
    nmp_app_free(app);
}

/// Duplicate feed_url subscribe dedupes: the snapshot still has one row.
#[test]
fn duplicate_subscribe_dedupes_in_domain_store() {
    let app = nmp_app_new();
    let handle = nmp_app_podcast_register(app);

    let feed = cstr("https://feeds.example.com/unique.xml");
    nmp_app_podcast_subscribe(handle, feed.as_ptr(), std::ptr::null(), std::ptr::null());
    nmp_app_podcast_subscribe(handle, feed.as_ptr(), std::ptr::null(), std::ptr::null());

    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert_eq!(view.podcasts.len(), 1, "duplicate subscribe must dedupe");

    nmp_app_podcast_unregister(handle);
    nmp_app_free(app);
}

/// Unsubscribing an unknown id is a silent no-op; existing rows survive.
#[test]
fn unsubscribe_unknown_id_is_noop() {
    let app = nmp_app_new();
    let handle = nmp_app_podcast_register(app);

    let feed = cstr("https://feeds.example.com/keeper.xml");
    nmp_app_podcast_subscribe(handle, feed.as_ptr(), std::ptr::null(), std::ptr::null());

    let unknown = cstr("01HZFAKE000000000000000000");
    nmp_app_podcast_unsubscribe(handle, unknown.as_ptr());

    let view = decode_snapshot(nmp_app_podcast_snapshot(handle));
    assert_eq!(view.podcasts.len(), 1, "existing row must survive unknown id unsubscribe");

    nmp_app_podcast_unregister(handle);
    nmp_app_free(app);
}

/// The `{"podcasts":[...]}` JSON shape must be preserved — iOS/Android pin
/// this key.
#[test]
fn snapshot_json_shape_matches_library_view_contract() {
    let app = nmp_app_new();
    let handle = nmp_app_podcast_register(app);

    let snap = nmp_app_podcast_snapshot(handle);
    assert!(!snap.is_null());
    let raw = unsafe { CStr::from_ptr(snap) }
        .to_str()
        .expect("valid UTF-8")
        .to_owned();
    nmp_app_podcast_snapshot_free(snap);

    // The top-level key must be "podcasts" — shells pin this.
    let value: serde_json::Value = serde_json::from_str(&raw).expect("valid JSON");
    assert!(
        value.get("podcasts").is_some(),
        "snapshot must carry 'podcasts' key: {raw}"
    );

    nmp_app_podcast_unregister(handle);
    nmp_app_free(app);
}
