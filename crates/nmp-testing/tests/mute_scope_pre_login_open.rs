//! #2930 — `difference(active_user().follows(), list_members(mute))` must
//! open BEFORE an active account exists, exactly like plain
//! `active_user().follows()` does, instead of hard-failing with
//! `ScopeNotSupportedYet { scope: "ListMembers-active-mute-no-active-account" }`.
//!
//! The active-mute `ListMembers` source used to be `RequireActive`
//! (`nmp_feed_session::nip51_sources::resolve_active_mute_list_members`), so
//! composing it into a home-feed `Difference` scope made the WHOLE open fail
//! pre-login even though the plain-follows half of the scope opens fine on
//! its own. The fix reclassifies it `AllowMissingActive`: with no active
//! account it resolves to an empty mute set, so `difference(follows, mute)`
//! reduces to permissive follows (fail-open, not fail-closed) — matching
//! `crates/nmp-testing/tests/empty_home_feed_difference_repro.rs`'s
//! already-proven post-login read-model soundness.

#[path = "common/mod.rs"]
mod common;
#[path = "reduced_source_relay_e2e/support.rs"]
mod support;

use std::time::{Duration, Instant};

use common::recording_relay::{has_author, has_kind, RecordingRelay};
use support::*;

fn feed_ids_within(app: &nmp_native_runtime::NmpApp, key: &str, secs: u64) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut last = flat_feed_ids(app, key);
    while Instant::now() < deadline {
        last = flat_feed_ids(app, key);
        if !last.is_empty() {
            return last;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    last
}

/// The crux of #2930: opening the composed scope with NO active account must
/// succeed (previously: `Err(ScopeNotSupportedYet)`), and must degrade to an
/// empty feed rather than panicking or leaking a partially-registered source.
#[test]
fn difference_follows_minus_mute_opens_before_sign_in() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let app = new_started_reduced_source_app();
    let app_ref = unsafe { &*app };

    let key = "test.2930.pre-login-open";
    let handle = app_ref
        .open_feed(&difference_follows_minus_mute_params(key))
        .expect("difference(follows, mute) must open before sign-in (#2930)");

    assert_eq!(
        flat_feed_ids(app_ref, key),
        Vec::<String>::new(),
        "no active account -> empty follows AND empty mute set -> empty feed, not an error"
    );

    app_ref.close_feed(&handle);
    let _ = rx;
    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

/// End-to-end: the SAME session that opened pre-login must recover once
/// sign-in resolves the active account, proving the pre-login open isn't a
/// dead session — it is the live, view-driven `ActiveUserFollows` pattern
/// (#2930's stated symmetry goal) extended to the mute-composed scope.
#[test]
fn difference_follows_minus_mute_opens_pre_login_then_populates_after_sign_in() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(171);
    let bob = keys_from_byte(172);
    let bob_pk = bob.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();

    let contacts = signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let bob_note = signed_note(&bob, "pre-login-then-signed-in bob", 110);
    let bob_note_id = bob_note.id.to_hex();

    let mut relay = RecordingRelay::spawn(vec![contacts, bob_note]);
    let app = new_reduced_source_app_before_start();
    let app_ref = unsafe { &*app };
    start_app(app);
    add_relay(app, relay.url());

    let key = "test.2930.pre-login-then-signed-in";
    let _handle = app_ref
        .open_feed(&difference_follows_minus_mute_params(key))
        .expect("difference(follows, mute) must open before sign-in (#2930)");

    assert_eq!(
        flat_feed_ids(app_ref, key),
        Vec::<String>::new(),
        "pre-login: no active account -> empty feed, no error"
    );

    // Sign-in resolves the active account for BOTH children of the
    // difference: the follow set (permissive side) and the mute set
    // (exclusion side, still absent on the relay).
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice_pk);

    relay.wait_req("alice kind:3 (follows)", |f| {
        has_kind(f, 3) && has_author(f, &alice_pk)
    });
    relay.wait_req("alice kind:10000 (mute)", |f| {
        has_kind(f, 10_000) && has_author(f, &alice_pk)
    });
    relay.wait_req("bob kind:1", |f| has_kind(f, 1) && has_author(f, &bob_pk));

    let ids = feed_ids_within(app_ref, key, 8);
    assert_eq!(
        ids,
        vec![bob_note_id],
        "after sign-in, follows resolve Bob and the absent mute list excludes nobody (got {ids:?})"
    );

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}
