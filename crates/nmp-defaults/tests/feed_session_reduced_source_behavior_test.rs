//! Reduced-source feed-session behavior (#2092 M3).
//!
//! These tests drive the real `NmpApp::open_feed` path for
//! dynamic feed scopes: a source event reduces to a pubkey set,
//! which recompiles the session-owned dependent acquisition set and cache-serves
//! member timelines. The app never observes the concrete author expansion.

#[path = "feed_session_reduced_source_support.rs"]
mod support;

use nmp_ffi::nmp_app_new;
use support::*;

#[test]
fn active_user_follows_prelogin_signin_replays_cached_follow_set_and_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(21);
    let bob = keys_from_byte(22);
    let stranger = keys_from_byte(23);
    let bob_pk = bob.public_key().to_hex();

    let (contacts_id, contacts_json) =
        signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let (bob_note_id, bob_note_json) = signed_note(&bob, "cached follow note", 110);
    let (stranger_note_id, stranger_note_json) = signed_note(&stranger, "outside follows", 120);
    inject_event(app, &rx, app_ref, &contacts_id, &contacts_json);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &stranger_note_id, &stranger_note_json);

    let key = "test.feed.active-follows.prelogin";
    let _handle = app_ref
        .open_feed(&active_follows_params(key), &compiler)
        .expect("active-follows feed opens before sign-in");
    assert_eq!(
        flat_feed_ids(app_ref, key),
        Vec::<String>::new(),
        "pre-login active-follows source must fail closed, not wildcard"
    );

    sign_in(app, &alice);
    wait_for(&rx, "sign-in replays cached active follows", || {
        app_ref.active_account_handle().lock().unwrap().as_deref()
            == Some(alice.public_key().to_hex().as_str())
            && flat_feed_ids(app_ref, key) == std::slice::from_ref(&bob_note_id)
    });

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn active_user_follows_replacement_recompiles_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(24);
    let bob = keys_from_byte(25);
    let carol = keys_from_byte(26);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice.public_key().to_hex());

    let key = "test.feed.active-follows.replace";
    let _handle = app_ref
        .open_feed(&active_follows_params(key), &compiler)
        .expect("active-follows feed opens");

    let (bob_note_id, bob_note_json) = signed_note(&bob, "first follow", 110);
    let (carol_note_id, carol_note_json) = signed_note(&carol, "replacement follow", 120);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &carol_note_id, &carol_note_json);

    let (contacts_id, contacts_json) = signed_contact_list(&alice, &[bob_pk], 130);
    inject_event(app, &rx, app_ref, &contacts_id, &contacts_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let (replacement_id, replacement_json) = signed_contact_list(&alice, &[carol_pk], 140);
    inject_event(app, &rx, app_ref, &replacement_id, &replacement_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&carol_note_id));

    let (clear_id, clear_json) = signed_contact_list(&alice, &[], 150);
    inject_event(app, &rx, app_ref, &clear_id, &clear_json);
    wait_feed_ids(&rx, app_ref, key, &[]);

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn active_user_follows_account_switch_replays_new_account_source() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(27);
    let bob = keys_from_byte(28);
    let carol = keys_from_byte(29);
    let dave = keys_from_byte(30);
    let bob_pk = bob.public_key().to_hex();
    let dave_pk = dave.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice.public_key().to_hex());

    let key = "test.feed.active-follows.account-switch";
    let _handle = app_ref
        .open_feed(&active_follows_params(key), &compiler)
        .expect("active-follows feed opens");

    let (bob_note_id, bob_note_json) = signed_note(&bob, "alice follow", 110);
    let (dave_note_id, dave_note_json) = signed_note(&dave, "carol follow", 120);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &dave_note_id, &dave_note_json);

    let (alice_contacts_id, alice_contacts_json) =
        signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 130);
    inject_event(app, &rx, app_ref, &alice_contacts_id, &alice_contacts_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let (carol_contacts_id, carol_contacts_json) =
        signed_contact_list(&carol, std::slice::from_ref(&dave_pk), 140);
    inject_event(app, &rx, app_ref, &carol_contacts_id, &carol_contacts_json);

    sign_in(app, &carol);
    wait_for(
        &rx,
        "account switch replays cached active follow source",
        || {
            app_ref.active_account_handle().lock().unwrap().as_deref() == Some(carol_pk.as_str())
                && flat_feed_ids(app_ref, key) == std::slice::from_ref(&dave_note_id)
        },
    );

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn list_members_cache_first_open_derives_members_and_replays_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(1);
    let bob = keys_from_byte(2);
    let stranger = keys_from_byte(3);
    let bob_pk = bob.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice.public_key().to_hex());

    let (list_id, list_json) = signed_people_list(&alice, "team", &[bob_pk], 100);
    let (bob_note_id, bob_note_json) = signed_note(&bob, "cached member note", 110);
    let (stranger_note_id, stranger_note_json) = signed_note(&stranger, "outside list", 120);
    inject_event(app, &rx, app_ref, &list_id, &list_json);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &stranger_note_id, &stranger_note_json);

    let key = "test.feed.list.cache";
    let _handle = app_ref
        .open_feed(&list_params(key), &compiler)
        .expect("list-members feed opens");

    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn active_mute_list_source_reuses_reduced_source_for_non_follow_members() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(11);
    let bob = keys_from_byte(12);
    let carol = keys_from_byte(13);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice.public_key().to_hex());

    let key = "test.feed.mute-source.replace";
    let _handle = app_ref
        .open_feed(&mute_source_params(key), &compiler)
        .expect("mute-list source feed opens");

    let (bob_note_id, bob_note_json) = signed_note(&bob, "muted author first", 110);
    let (carol_note_id, carol_note_json) = signed_note(&carol, "muted author replacement", 120);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &carol_note_id, &carol_note_json);

    let (mute_id, mute_json) = signed_mute_list(&alice, &[bob_pk], 130);
    inject_event(app, &rx, app_ref, &mute_id, &mute_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let (replacement_id, replacement_json) = signed_mute_list(&alice, &[carol_pk], 140);
    inject_event(app, &rx, app_ref, &replacement_id, &replacement_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&carol_note_id));

    let (clear_id, clear_json) = signed_mute_list(&alice, &[], 150);
    inject_event(app, &rx, app_ref, &clear_id, &clear_json);
    wait_feed_ids(&rx, app_ref, key, &[]);

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn active_mute_list_source_account_switch_replays_new_account_source() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(14);
    let bob = keys_from_byte(15);
    let carol = keys_from_byte(16);
    let dave = keys_from_byte(17);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    let dave_pk = dave.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice.public_key().to_hex());

    let key = "test.feed.mute-source.account-switch";
    let _handle = app_ref
        .open_feed(&mute_source_params(key), &compiler)
        .expect("mute-list source feed opens");

    let (bob_note_id, bob_note_json) = signed_note(&bob, "alice muted author", 110);
    let (dave_note_id, dave_note_json) = signed_note(&dave, "carol muted author", 120);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &dave_note_id, &dave_note_json);

    let (alice_mute_id, alice_mute_json) =
        signed_mute_list(&alice, std::slice::from_ref(&bob_pk), 130);
    inject_event(app, &rx, app_ref, &alice_mute_id, &alice_mute_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let (carol_mute_id, carol_mute_json) =
        signed_mute_list(&carol, std::slice::from_ref(&dave_pk), 140);
    inject_event(app, &rx, app_ref, &carol_mute_id, &carol_mute_json);

    sign_in(app, &carol);
    wait_for(
        &rx,
        "account switch replays cached active mute-list source",
        || {
            app_ref.active_account_handle().lock().unwrap().as_deref() == Some(carol_pk.as_str())
                && flat_feed_ids(app_ref, key) == std::slice::from_ref(&dave_note_id)
        },
    );

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn list_members_replacement_and_clear_recompile_acquisition_and_rows() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(4);
    let bob = keys_from_byte(5);
    let carol = keys_from_byte(6);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice.public_key().to_hex());

    let key = "test.feed.list.replace";
    let _handle = app_ref
        .open_feed(&list_params(key), &compiler)
        .expect("list-members feed opens");

    let (bob_note_id, bob_note_json) = signed_note(&bob, "first member", 110);
    let (carol_note_id, carol_note_json) = signed_note(&carol, "replacement member", 120);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    inject_event(app, &rx, app_ref, &carol_note_id, &carol_note_json);

    let (list_id, list_json) = signed_people_list(&alice, "team", &[bob_pk], 130);
    inject_event(app, &rx, app_ref, &list_id, &list_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let (replacement_id, replacement_json) = signed_people_list(&alice, "team", &[carol_pk], 140);
    inject_event(app, &rx, app_ref, &replacement_id, &replacement_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&carol_note_id));

    let (clear_id, clear_json) = signed_people_list(&alice, "team", &[], 150);
    inject_event(app, &rx, app_ref, &clear_id, &clear_json);
    wait_feed_ids(&rx, app_ref, key, &[]);

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn list_members_account_switch_withdraws_old_source_and_reacquires_new_account() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    let app_ref = unsafe { &*app };
    nmp_app_start(app, 256, 8);

    let alice = keys_from_byte(7);
    let bob = keys_from_byte(8);
    let carol = keys_from_byte(9);
    let dave = keys_from_byte(10);
    let bob_pk = bob.public_key().to_hex();
    let dave_pk = dave.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.feed.list.account-switch";
    let _handle = app_ref
        .open_feed(&list_params(key), &compiler)
        .expect("list-members feed opens");

    let (bob_note_id, bob_note_json) = signed_note(&bob, "alice list member", 110);
    inject_event(app, &rx, app_ref, &bob_note_id, &bob_note_json);
    let (alice_list_id, alice_list_json) = signed_people_list(&alice, "team", &[bob_pk], 120);
    inject_event(app, &rx, app_ref, &alice_list_id, &alice_list_json);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let (dave_note_id, dave_note_json) = signed_note(&dave, "carol list member", 130);
    inject_event(app, &rx, app_ref, &dave_note_id, &dave_note_json);
    let (carol_list_id, carol_list_json) = signed_people_list(&carol, "team", &[dave_pk], 140);
    inject_event(app, &rx, app_ref, &carol_list_id, &carol_list_json);

    sign_in(app, &carol);
    wait_for(
        &rx,
        "account switch replays cached list-members source",
        || {
            app_ref.active_account_handle().lock().unwrap().as_deref() == Some(carol_pk.as_str())
                && flat_feed_ids(app_ref, key) == std::slice::from_ref(&dave_note_id)
        },
    );

    nmp_app_free(app);
    uninstall_update_signal();
}
