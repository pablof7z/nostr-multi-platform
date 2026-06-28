//! #1740 steps 7 + 9 — the ONE public app-facing feed doorway.
//!
//! These prove the public C-ABI surface (`nmp_app_open_feed` /
//! `nmp_app_close_feed`) opens EVERY app feed through one typed entry and
//! fails closed on bad input, and that the matrix of app primary content kinds
//! (Chirp `[1]`, Olas `[20]`, longform `[30023]`) all open through it.
//!
//! The negative (raw public symbols are GONE) is asserted by the grep-gate in
//! `nmp-testing/tests/feed_public_surface_retired.rs`; here we assert the
//! POSITIVE end-to-end (open returns a real handle; close tears it down) plus
//! fail-closed rejection of wrapper/delete primary kinds at the public boundary.

use std::ffi::{c_void, CStr, CString};
use std::sync::mpsc::{channel, Receiver, Sender};

use nmp_ffi::{
    nmp_app_free, nmp_app_load_older_feed, nmp_app_new, nmp_app_set_update_callback, nmp_app_start,
    nmp_free_string, NmpApp,
};
use serde_json::Value;

use super::{nmp_app_close_feed, nmp_app_open_feed};

extern "C" fn update_noop(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {}

fn start_app() -> (*mut NmpApp, Receiver<()>, Box<Sender<()>>) {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must succeed");
    let (tx, rx) = channel::<()>();
    let tx_box = Box::new(tx);
    let ctx = tx_box.as_ref() as *const Sender<()> as *mut c_void;
    nmp_app_set_update_callback(app, ctx, Some(update_noop));
    nmp_app_start(app, 80, 4);
    // The scope compiler anchors a session on the active account (the reactive
    // perspective owner); set one directly (the same affordance the tag-feed
    // tests use) so the matrix exercises the open path, not the no-account
    // fail-closed guard.
    let app_ref: &NmpApp = unsafe { &*app };
    *app_ref.active_account_handle().lock().expect("active slot") =
        Some(nostr::Keys::generate().public_key().to_hex());
    (app, rx, tx_box)
}

/// A `FeedParams` JSON for a primary-kind set over the account-independent
/// `Tag` scope — so the matrix exercises the public doorway for each app's
/// PRIMARY content kind without needing a signed-in account (the wrapper/delete
/// derivation under test is identical regardless of scope).
fn tag_params_json(primary_kinds: &str, projection: &str) -> String {
    format!(
        r#"{{
          "primary_kinds": {primary_kinds},
          "render": "OpCentric",
          "acquisition": {{ "Tag": {{ "term": "nostr" }} }},
          "admission": "All",
          "ranking": "ChronologicalDesc",
          "window": {{ "initial_limit": 80 }},
          "projection": "{projection}"
        }}"#
    )
}

fn home_params_json() -> String {
    r#"{
      "primary_kinds": [1],
      "render": "OpCentric",
      "acquisition": "ActiveUserFollows",
      "admission": "All",
      "ranking": "ChronologicalDesc",
      "window": { "initial_limit": 80 },
      "projection": "nmp.feed.home"
    }"#
    .to_string()
}

fn author_params_json(pubkey: &str) -> String {
    format!(
        r#"{{
          "primary_kinds": [1],
          "render": "Flat",
          "acquisition": {{ "Authors": {{ "authors": ["{pubkey}"] }} }},
          "admission": "All",
          "ranking": "ChronologicalDesc",
          "window": {{ "initial_limit": 80 }},
          "projection": "nmp.feed.author.{pubkey}"
        }}"#
    )
}

fn thread_params_json(event_id: &str) -> String {
    format!(
        r#"{{
          "primary_kinds": [1],
          "render": "Flat",
          "acquisition": {{ "Referrer": {{ "event_id": "{event_id}" }} }},
          "admission": "All",
          "ranking": "ChronologicalDesc",
          "window": {{ "initial_limit": 80 }},
          "projection": "nmp.feed.thread.{event_id}"
        }}"#
    )
}

/// Call `nmp_app_open_feed` with `params_json` and return the parsed JSON
/// envelope, freeing the heap string the ABI returns.
fn open_feed(app: *mut NmpApp, params_json: &str) -> Value {
    let c = CString::new(params_json).expect("params JSON has no NUL");
    let raw = nmp_app_open_feed(app, c.as_ptr());
    assert!(
        !raw.is_null(),
        "open_feed must never return NULL for a non-null app"
    );
    let json = unsafe { CStr::from_ptr(raw) }
        .to_str()
        .expect("open_feed returns UTF-8")
        .to_string();
    nmp_free_string(raw);
    serde_json::from_str(&json).expect("open_feed returns a JSON object")
}

fn close_feed(app: *mut NmpApp, handle: &Value) {
    let handle_json = serde_json::to_string(handle).expect("re-serialize handle");
    let c = CString::new(handle_json).expect("handle JSON has no NUL");
    nmp_app_close_feed(app, c.as_ptr());
}

#[test]
fn public_open_feed_chirp_home_author_thread_all_open_and_close_by_handle() {
    let (app, _rx, _tx) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };
    let author = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let event_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    for (label, params, key) in [
        ("home", home_params_json(), "nmp.feed.home".to_string()),
        (
            "author",
            author_params_json(author),
            format!("nmp.feed.author.{author}"),
        ),
        (
            "thread",
            thread_params_json(event_id),
            format!("nmp.feed.thread.{event_id}"),
        ),
    ] {
        let envelope = open_feed(app, &params);
        assert!(
            envelope.get("error").is_none(),
            "{label} feed must open through generic nmp_app_open_feed, got {envelope}"
        );
        assert_eq!(envelope["projection_key"], key);
        assert_eq!(
            app_ref.live_feed_session_count(),
            1,
            "{label} open registers one live session"
        );

        close_feed(app, &envelope);
        assert_eq!(
            app_ref.live_feed_session_count(),
            0,
            "{label} close tears down by opaque handle"
        );
    }

    nmp_app_free(app);
}

#[test]
fn public_load_older_reaches_handle_opened_author_feed_controller() {
    let (app, _rx, _tx) = start_app();
    let author = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let key = CString::new(format!("nmp.feed.author.{author}")).unwrap();
    let envelope = open_feed(app, &author_params_json(author));
    assert!(
        envelope.get("error").is_none(),
        "author feed must open through generic nmp_app_open_feed"
    );

    let _ = nmp_app_load_older_feed(app, key.as_ptr());

    close_feed(app, &envelope);
    let app_ref: &NmpApp = unsafe { &*app };
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "author load-older session still tears down by handle"
    );
    nmp_app_free(app);
}

#[test]
fn public_open_feed_returns_handle_and_close_tears_down() {
    let (app, _rx, _tx) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "no sessions before open"
    );

    let envelope = open_feed(app, &tag_params_json("[1]", "nmp.feed.tag.nostr"));
    assert!(
        envelope.get("error").is_none(),
        "a valid [1] Tag feed must open, got: {envelope}"
    );
    assert_eq!(
        envelope["projection_key"], "nmp.feed.tag.nostr",
        "the handle carries the declared projection key"
    );
    assert!(
        envelope["session_id"].as_u64().is_some(),
        "the handle carries a minted opaque session id"
    );
    assert_eq!(
        app_ref.live_feed_session_count(),
        1,
        "open registered exactly one live session"
    );

    close_feed(app, &envelope);
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "close_feed tears the session down (registry entry freed, not flagged)"
    );

    nmp_app_free(app);
}

#[test]
fn public_open_feed_matrix_chirp_olas_longform_all_open() {
    // The acceptance matrix: every app's PRIMARY content kind opens through the
    // ONE public doorway. Wrapper/delete derivation happens below the boundary.
    let (app, _rx, _tx) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };

    for (label, primary, key) in [
        ("chirp", "[1]", "nmp.feed.tag.chirp"),
        ("olas", "[20]", "nmp.feed.tag.olas"),
        ("longform", "[30023]", "nmp.feed.tag.longform"),
    ] {
        let envelope = open_feed(app, &tag_params_json(primary, key));
        assert!(
            envelope.get("error").is_none(),
            "{label} primary {primary} must open via the public open_feed, got: {envelope}"
        );
        assert_eq!(
            envelope["projection_key"], key,
            "{label} session emits under its declared projection key"
        );
        close_feed(app, &envelope);
    }
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "every matrix session closed cleanly"
    );

    nmp_app_free(app);
}

#[test]
fn public_open_feed_fails_closed_on_wrapper_primary_kind() {
    // The negative the issue mandates: wrapper kinds 6/16 + delete kind 5 are
    // compiler-derived acquisition, NEVER valid app PRIMARY input. The public
    // boundary rejects them with a typed error, registering nothing.
    let (app, _rx, _tx) = start_app();
    let app_ref: &NmpApp = unsafe { &*app };

    for bad in ["[1, 6]", "[20, 16]", "[30023, 16]", "[1, 5]"] {
        let envelope = open_feed(app, &tag_params_json(bad, "nmp.feed.tag.bad"));
        assert_eq!(
            envelope["error"], "invalid_primary_kinds",
            "wrapper/delete primary {bad} must fail closed at the public boundary"
        );
    }
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "a rejected open registers no session (nothing leaks)"
    );

    nmp_app_free(app);
}

#[test]
fn public_open_feed_malformed_json_fails_closed() {
    let (app, _rx, _tx) = start_app();
    let envelope = open_feed(app, "{ not json");
    assert_eq!(
        envelope["error"], "bad_params",
        "malformed params JSON fails closed, never a panic across the ABI"
    );
    nmp_app_free(app);
}

#[test]
fn public_open_feed_null_app_returns_error_envelope_not_null() {
    // D6 — a null app returns a data-shaped error, never NULL or a panic.
    let params = CString::new(tag_params_json("[1]", "nmp.feed.tag.nostr")).unwrap();
    let raw = nmp_app_open_feed(std::ptr::null_mut(), params.as_ptr());
    assert!(
        !raw.is_null(),
        "null app must still return a non-null error envelope"
    );
    let json = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_string();
    nmp_free_string(raw);
    let envelope: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(envelope["error"], "null_app");
}

#[test]
fn public_close_feed_null_and_malformed_are_silent_noops() {
    let (app, _rx, _tx) = start_app();
    // Null app: no-op (no panic).
    nmp_app_close_feed(std::ptr::null_mut(), std::ptr::null());
    // Malformed handle JSON: no-op (no panic).
    let bad = CString::new("{ not json").unwrap();
    nmp_app_close_feed(app, bad.as_ptr());
    nmp_app_free(app);
}
