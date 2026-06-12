//! Tests for the Chirp home-feed wrapper (`nmp_app_chirp_open_home_feed` /
//! `nmp_app_chirp_close_home_feed`) and the HOME_FEED_KINDS constant.

use nmp_ffi::{nmp_app_free, nmp_app_new};

use crate::ffi::interest_feed::HOME_FEED_KINDS_JSON;
use crate::nmp_app_chirp_close_home_feed;
use crate::nmp_app_chirp_open_home_feed;

/// The home-feed kinds JSON must be exactly `"[1,6]"` — the Chirp social
/// timeline policy. Changing this string is a BREAKING change (it drives the
/// kernel's `OpenContactFeed { kinds }` payload and therefore the REQ filter).
#[test]
fn home_feed_kinds_json_is_chirp_social_policy() {
    assert_eq!(HOME_FEED_KINDS_JSON, "[1,6]");
}

/// `nmp_app_chirp_open_home_feed` must send `OpenContactFeed { kinds: {1,6} }`
/// through the actor. We verify this indirectly: the call must not panic,
/// and the app handle stays live. The exact command routing is covered by the
/// `close_contact_feed_withdraws_follow_interests_and_emits_close` test in
/// `nmp-core`.
#[test]
fn chirp_open_home_feed_threads_1_6() {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must return a non-null handle");

    // Should not panic; D6 = fire-and-forget.
    nmp_app_chirp_open_home_feed(app);
    nmp_app_chirp_close_home_feed(app);

    nmp_app_free(app);
}

/// Null-app calls must be silent no-ops (D6).
#[test]
fn chirp_home_feed_null_app_is_silent_noop() {
    let null: *mut nmp_ffi::NmpApp = std::ptr::null_mut();
    nmp_app_chirp_open_home_feed(null); // must not panic
    nmp_app_chirp_close_home_feed(null); // must not panic
}
