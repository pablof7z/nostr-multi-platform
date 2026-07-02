use crate::support::*;

use crate::common::recording_relay::{has_author, has_kind, RecordingRelay};

#[test]
fn replacement_updates_open_feed_via_source_effects() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(57);
    let bob = keys_from_byte(58);
    let carol = keys_from_byte(59);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();
    let list_id = "team";

    let initial_list = signed_people_list(&alice, list_id, std::slice::from_ref(&bob_pk), 100);
    let replacement_list =
        signed_people_list(&alice, list_id, std::slice::from_ref(&carol_pk), 130);
    let bob_note = signed_note(&bob, "people-list bob", 110);
    let carol_note = signed_note(&carol, "people-list carol", 120);
    let bob_note_id = bob_note.id.to_hex();
    let carol_note_id = carol_note.id.to_hex();

    let mut relay = RecordingRelay::spawn(vec![initial_list.clone(), bob_note, carol_note]);
    let app = new_started_reduced_source_app();
    add_relay(app, relay.url());
    sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.relay.list-members.replace";
    let _handle = app_ref
        .open_feed(&list_members_params(key, list_id))
        .expect("ListMembers source opens");

    relay.wait_req("Alice kind:30000 list source", |filter| {
        has_kind(filter, 30_000) && has_author(filter, &alice_pk)
    });
    let bob_req = relay.wait_req("derived people-list Bob author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk) && !has_author(filter, &carol_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    relay.push_event(replacement_list);
    relay.wait_close("withdrawn people-list Bob author sub", &bob_req.sub_id);
    relay.wait_req("derived people-list Carol author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &carol_pk) && !has_author(filter, &bob_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&carol_note_id));

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

#[test]
fn account_switch_replays_open_feed_via_source_effects() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(60);
    let carol = keys_from_byte(61);
    let bob = keys_from_byte(62);
    let dave = keys_from_byte(63);
    let alice_pk = alice.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let dave_pk = dave.public_key().to_hex();
    let list_id = "team";

    let alice_list = signed_people_list(&alice, list_id, std::slice::from_ref(&bob_pk), 100);
    let carol_list = signed_people_list(&carol, list_id, std::slice::from_ref(&dave_pk), 130);
    let bob_note = signed_note(&bob, "alice list member", 110);
    let dave_note = signed_note(&dave, "carol list member", 120);
    let bob_note_id = bob_note.id.to_hex();
    let dave_note_id = dave_note.id.to_hex();

    let mut relay = RecordingRelay::spawn(vec![alice_list, carol_list, bob_note, dave_note]);
    let app = new_started_reduced_source_app();
    add_relay(app, relay.url());
    sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.relay.list-members.account-switch";
    let _handle = app_ref
        .open_feed(&list_members_params(key, list_id))
        .expect("ListMembers source opens");

    relay.wait_req("Alice kind:30000 list source", |filter| {
        has_kind(filter, 30_000) && has_author(filter, &alice_pk)
    });
    let bob_req = relay.wait_req("Alice derived Bob author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk) && !has_author(filter, &dave_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    sign_in(app, &carol);
    wait_active(&rx, app_ref, &carol_pk);
    relay.wait_close("account switch closes old Bob author sub", &bob_req.sub_id);
    relay.wait_req("Carol kind:30000 list source", |filter| {
        has_kind(filter, 30_000) && has_author(filter, &carol_pk)
    });
    relay.wait_req("Carol derived Dave author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &dave_pk) && !has_author(filter, &bob_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&dave_note_id));

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}
