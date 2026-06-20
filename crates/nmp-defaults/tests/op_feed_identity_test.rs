//! OP-feed identity-change integration tests.
//!
//! These drive the production `NmpApp` update listener seam: register the OP
//! feed before start, let the actor mutate the active-account slot through real
//! sign-in / remove-account commands, and wait on update callback ticks until
//! the follow predicate and feed snapshot reflect the identity change.

use std::ffi::{c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use nmp_ffi::{
    nmp_app_free, nmp_app_inject_signed_event_json, nmp_app_new, nmp_app_remove_account,
    nmp_app_set_update_callback, nmp_app_signin_nsec, nmp_app_start,
};
use nostr::prelude::*;
use nostr::{EventBuilder, Kind, Tag, Timestamp};

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

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

fn keys_from_nsec(nsec: &str) -> Keys {
    let sk = SecretKey::parse(nsec).expect("valid nsec");
    Keys::new(sk)
}

fn second_nsec() -> String {
    let sk = SecretKey::from_slice(&[2u8; 32]).expect("valid secret");
    Keys::new(sk).secret_key().to_bech32().expect("nsec bech32")
}

fn sign_in(app: *mut nmp_ffi::NmpApp, nsec: &str) {
    let secret = CString::new(nsec).expect("nsec has no nul");
    nmp_app_signin_nsec(app, secret.as_ptr(), 1);
}

fn inject_signed_json(app: *mut nmp_ffi::NmpApp, json: &str) {
    let event = CString::new(json).expect("event json has no nul");
    assert!(
        nmp_app_inject_signed_event_json(app, event.as_ptr()),
        "signed event must verify and inject"
    );
}

fn signed_kind3(keys: &Keys, follows: &[String], created_at: u64) -> String {
    let tags = follows
        .iter()
        .map(|pk| Tag::parse(["p", pk.as_str()]).expect("valid p tag"))
        .collect::<Vec<_>>();
    EventBuilder::new(Kind::from(3u16), "")
        .tags(tags)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign kind:3")
        .as_json()
}

fn signed_note(keys: &Keys, content: &str, created_at: u64) -> String {
    EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("sign note")
        .as_json()
}

#[test]
fn logout_before_kind3_clears_op_feed_identity_state() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    // SAFETY: app is a live pointer from nmp_app_new.
    let app_ref = unsafe { &*app };
    let defaults = nmp_defaults::register_op_feed_defaults(app_ref, String::new(), vec![1, 6]);
    nmp_app_start(app, 256, 4);

    let alice = keys_from_nsec(TEST_NSEC);
    let alice_pk = alice.public_key().to_hex();
    sign_in(app, TEST_NSEC);
    wait_for(&rx, "active account self-seed", || {
        defaults.follow_set.predicate()(&alice_pk)
    });

    inject_signed_json(app, &signed_note(&alice, "self root before kind3", 100));
    wait_for(&rx, "OP snapshot before logout", || {
        !defaults
            .engine
            .snapshot(&nmp_feed::FeedRequest::default())
            .cards
            .is_empty()
    });

    let identity = CString::new(alice_pk.clone()).unwrap();
    nmp_app_remove_account(app, identity.as_ptr());
    wait_for(&rx, "logout clears OP feed", || {
        app_ref.active_account_handle().lock().unwrap().is_none()
            && defaults.follow_set.follows().is_empty()
            && defaults
                .engine
                .snapshot(&nmp_feed::FeedRequest::default())
                .cards
                .is_empty()
    });

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn switch_after_kind3_clears_predicate_snapshot_then_new_kind3_repopulates() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    // SAFETY: app is a live pointer from nmp_app_new.
    let app_ref = unsafe { &*app };
    let defaults = nmp_defaults::register_op_feed_defaults(app_ref, String::new(), vec![1, 6]);
    nmp_app_start(app, 256, 4);

    let alice = keys_from_nsec(TEST_NSEC);
    let alice_pk = alice.public_key().to_hex();
    let bob = Keys::generate();
    let bob_pk = bob.public_key().to_hex();
    let bob_pk_for_kind3 = bob_pk.clone();
    let bob_pk_for_switch_check = bob_pk.clone();

    sign_in(app, TEST_NSEC);
    wait_for(&rx, "alice self-seed", || {
        defaults.follow_set.predicate()(&alice_pk)
    });

    inject_signed_json(app, &signed_kind3(&alice, &[bob_pk_for_kind3], 110));
    wait_for(&rx, "alice kind3 follow set", || {
        defaults.follow_set.predicate()(&bob_pk)
    });

    inject_signed_json(app, &signed_note(&bob, "followed root before switch", 120));
    wait_for(&rx, "OP snapshot before switch", || {
        !defaults
            .engine
            .snapshot(&nmp_feed::FeedRequest::default())
            .cards
            .is_empty()
    });

    let nsec_b = second_nsec();
    let carol = keys_from_nsec(&nsec_b);
    let carol_pk = carol.public_key().to_hex();
    sign_in(app, &nsec_b);
    wait_for(&rx, "switch clears prior OP identity", || {
        app_ref.active_account_handle().lock().unwrap().as_deref() == Some(carol_pk.as_str())
            && defaults.follow_set.predicate()(&carol_pk)
            && !defaults.follow_set.predicate()(&bob_pk_for_switch_check)
            && defaults
                .engine
                .snapshot(&nmp_feed::FeedRequest::default())
                .cards
                .is_empty()
    });

    inject_signed_json(
        app,
        &signed_kind3(&carol, std::slice::from_ref(&alice_pk), 130),
    );
    wait_for(&rx, "new account kind3 repopulates", || {
        defaults.follow_set.predicate()(&alice_pk)
            && !defaults.follow_set.predicate()(&bob_pk_for_switch_check)
            && defaults
                .engine
                .snapshot(&nmp_feed::FeedRequest::default())
                .cards
                .is_empty()
    });

    nmp_app_free(app);
    uninstall_update_signal();
}
