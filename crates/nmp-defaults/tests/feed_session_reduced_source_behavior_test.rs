//! Reduced-source feed-session behavior (#2092 M3).
//!
//! These tests drive the real `NmpApp::open_feed` path for
//! `FeedScope::ListMembers`: a kind:30000 source event reduces to a pubkey set,
//! which recompiles the session-owned dependent acquisition set and cache-serves
//! member timelines. The app never observes the concrete author expansion.

use std::ffi::{c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nmp_core::WireProjectionState;
use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ListId,
    ProjectionKey,
};
use nmp_ffi::{
    nmp_app_free, nmp_app_inject_signed_event_json, nmp_app_new, nmp_app_set_update_callback,
    nmp_app_signin_nsec, nmp_app_start, nmp_app_wait_barrier, FeedOpenError, NmpApp,
};
use nostr::prelude::*;
use nostr::{EventBuilder, Kind, Tag, Timestamp};

static SERIAL: Mutex<()> = Mutex::new(());
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

fn install_update_signal() -> Receiver<()> {
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

fn wait_for(rx: &Receiver<()>, label: &str, pred: impl Fn() -> bool) {
    if pred() {
        return;
    }
    loop {
        rx.recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|_| panic!("timed out waiting for {label}"));
        if pred() {
            return;
        }
    }
}

fn keys_from_byte(byte: u8) -> Keys {
    let sk = SecretKey::from_slice(&[byte; 32]).expect("valid secret");
    Keys::new(sk)
}

fn sign_in(app: *mut NmpApp, keys: &Keys) {
    let nsec = keys.secret_key().to_bech32().expect("nsec bech32");
    let secret = CString::new(nsec).expect("nsec has no nul");
    nmp_app_signin_nsec(app, secret.as_ptr(), 1);
}

fn wait_active(rx: &Receiver<()>, app: &NmpApp, pubkey: &str) {
    wait_for(rx, "active account", || {
        app.active_account_handle().lock().unwrap().as_deref() == Some(pubkey)
    });
}

fn inject_event(app: *mut NmpApp, rx: &Receiver<()>, app_ref: &NmpApp, id: &str, json: &str) {
    let event = CString::new(json).expect("event json has no nul");
    assert!(
        nmp_app_inject_signed_event_json(app, event.as_ptr()),
        "signed event must verify and inject"
    );
    assert!(
        nmp_app_wait_barrier(app, 5_000),
        "actor must process injected event before the test continues"
    );
    wait_for(rx, "event readable", || app_ref.event_by_id(id).is_some());
}

fn signed_people_list(
    keys: &Keys,
    list_id: &str,
    members: &[String],
    created_at: u64,
) -> (String, String) {
    let mut tags = vec![Tag::parse(["d", list_id]).expect("valid d tag")];
    tags.extend(
        members
            .iter()
            .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag")),
    );
    let event = EventBuilder::new(Kind::from(30_000u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:30000");
    (event.id.to_hex(), event.as_json())
}

fn signed_note(keys: &Keys, content: &str, created_at: u64) -> (String, String) {
    let event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note");
    (event.id.to_hex(), event.as_json())
}

fn list_params(projection: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::ListMembers {
            list: ListId("team".to_string()),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: FeedWindow { initial_limit: 80 },
        projection: ProjectionKey(projection.into()),
    }
}

fn compiler(
    app: &NmpApp,
    params: &FeedParams,
    kinds: &std::collections::BTreeSet<u32>,
) -> Result<nmp_feed::FeedSessionBuild, FeedOpenError> {
    nmp_defaults::compile_feed_params(app, params, kinds)
}

fn flat_feed_ids(app: &NmpApp, key: &str) -> Vec<String> {
    let Some(row) = app
        .run_typed_snapshot_projections()
        .into_iter()
        .find(|row| row.key == key && row.state != WireProjectionState::Cleared)
    else {
        return Vec::new();
    };
    nmp_nip01::op_feed::decode_op_feed_snapshot(&row.payload)
        .expect("NOFS payload decodes")
        .cards
        .into_iter()
        .map(|card| card.card.id)
        .collect()
}

fn wait_feed_ids(rx: &Receiver<()>, app: &NmpApp, key: &str, expected: &[String]) {
    wait_for(rx, "feed ids", || flat_feed_ids(app, key) == expected);
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
    wait_for(&rx, "account switch replays cached list-members source", || {
        app_ref.active_account_handle().lock().unwrap().as_deref() == Some(carol_pk.as_str())
            && flat_feed_ids(app_ref, key) == std::slice::from_ref(&dave_note_id)
    });

    nmp_app_free(app);
    uninstall_update_signal();
}
