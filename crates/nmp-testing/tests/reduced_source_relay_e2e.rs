//! #2092 M6 -- ReducedSource acquisition over real relay frames.
//!
//! These tests drive a real `NmpApp` plus the owner-local feed compiler through a
//! local WebSocket relay. The relay records actual `REQ`/`CLOSE` frames and
//! sends signed Nostr `EVENT`/`EOSE` frames back through the kernel ingest path.

#[path = "reduced_source_relay_e2e/support.rs"]
mod support;

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use nmp_core::substrate::{ReqFrameContext, ReqFrameInterceptor};
use nmp_core::{Kernel, OutboundMessage};
use serde_json::Value;

use support::*;

#[derive(Default)]
struct CapturingReqInterceptor {
    seen: Mutex<Vec<ReqFrameContext>>,
    wake: Condvar,
}

impl CapturingReqInterceptor {
    fn wait_for(&self, label: &str, pred: impl Fn(&ReqFrameContext) -> bool) -> ReqFrameContext {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(ctx) = seen.iter().find(|ctx| pred(ctx)).cloned() {
                return ctx;
            }
            let now = Instant::now();
            assert!(
                now < deadline,
                "timed out waiting for {label}; observed req contexts = {:?}",
                *seen
            );
            let remaining = deadline.saturating_duration_since(now);
            let (next, result) = self
                .wake
                .wait_timeout(seen, remaining)
                .expect("req capture condvar");
            seen = next;
            if result.timed_out() {
                assert!(
                    seen.iter().any(&pred),
                    "timed out waiting for {label}; observed req contexts = {:?}",
                    *seen
                );
            }
        }
    }
}

impl ReqFrameInterceptor for CapturingReqInterceptor {
    fn intercept_req(
        &self,
        _kernel: &mut Kernel,
        ctx: &ReqFrameContext,
    ) -> Option<Vec<OutboundMessage>> {
        self.seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(ctx.clone());
        self.wake.notify_all();
        None
    }
}

fn captured_req_matches(ctx: &ReqFrameContext, relay_url: &str, author: &str, kind: u64) -> bool {
    if ctx.relay_url != relay_url {
        return false;
    }
    let Ok(filter) = serde_json::from_str::<Value>(&ctx.filter_json) else {
        return false;
    };
    has_kind(&filter, kind) && has_author(&filter, author)
}

#[test]
fn active_follows_relay_replaces_source_and_closes_stale_author_sub() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(41);
    let bob = keys_from_byte(42);
    let carol = keys_from_byte(43);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();

    let initial_contacts = signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let replacement_contacts = signed_contact_list(&alice, std::slice::from_ref(&carol_pk), 130);
    let bob_note = signed_note(&bob, "relay bob", 110);
    let carol_note = signed_note(&carol, "relay carol", 120);
    let bob_note_id = bob_note.id.to_hex();
    let carol_note_id = carol_note.id.to_hex();

    let mut relay = RecordingRelay::spawn(vec![
        initial_contacts.clone(),
        bob_note.clone(),
        carol_note.clone(),
    ]);
    let app = new_started_reduced_source_app();
    add_relay(app, relay.url());
    sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.relay.active-follows.replace";
    let _handle = app_ref
        .open_feed(&active_follows_params(key), &compiler)
        .expect("active follows opens");

    relay.wait_req("active account kind:3 source", |filter| {
        has_kind(filter, 3) && has_author(filter, &alice_pk)
    });
    let bob_req = relay.wait_req("derived Bob author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk) && !has_author(filter, &carol_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    relay.push_event(replacement_contacts);
    relay.wait_close("withdrawn Bob author sub", &bob_req.sub_id);
    relay.wait_req("derived Carol author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &carol_pk) && !has_author(filter, &bob_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&carol_note_id));

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

#[test]
fn active_follows_cache_first_open_still_replays_live_relay_reqs() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(44);
    let bob = keys_from_byte(45);
    let bob_pk = bob.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();
    let contacts = signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let bob_note = signed_note(&bob, "cached before relay", 110);
    let bob_note_id = bob_note.id.to_hex();

    let app = new_started_reduced_source_app();
    let app_ref = unsafe { &*app };
    inject_event(app, &rx, app_ref, &contacts);
    inject_event(app, &rx, app_ref, &bob_note);

    let key = "test.relay.active-follows.cache-first";
    let _handle = app_ref
        .open_feed(&active_follows_params(key), &compiler)
        .expect("active follows opens from cache");
    assert_eq!(
        flat_feed_ids(app_ref, key),
        Vec::<String>::new(),
        "pre-login active follows must fail closed even with cached source events"
    );

    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice_pk);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    let mut relay = RecordingRelay::spawn(vec![contacts, bob_note]);
    add_relay(app, relay.url());
    relay.wait_req("live source REQ after cache-first open", |filter| {
        has_kind(filter, 3) && has_author(filter, &alice_pk)
    });
    relay.wait_req("live derived author REQ after cache-first open", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk)
    });
    assert_eq!(
        flat_feed_ids(app_ref, key),
        vec![bob_note_id],
        "relay refinement must not disturb the cache-first row"
    );

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

#[test]
fn flat_active_follows_cache_first_open_uses_source_effects() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(55);
    let bob = keys_from_byte(56);
    let bob_pk = bob.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();
    let contacts = signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let bob_note = signed_note(&bob, "flat cached before login", 110);
    let bob_note_id = bob_note.id.to_hex();

    let app = new_started_reduced_source_app();
    let app_ref = unsafe { &*app };
    inject_event(app, &rx, app_ref, &contacts);
    inject_event(app, &rx, app_ref, &bob_note);

    let key = "test.relay.flat-active-follows.cache-first";
    let _handle = app_ref
        .open_feed(&flat_active_follows_params(key), &compiler)
        .expect("flat active follows opens from cache");
    assert_eq!(
        flat_feed_ids(app_ref, key),
        Vec::<String>::new(),
        "pre-login flat active follows must fail closed even with cached source events"
    );

    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice_pk);
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

#[test]
fn list_members_nip65_arrival_emits_target_author_req() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(53);
    let bob = keys_from_byte(54);
    let bob_pk = bob.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();
    let nip65_target = "wss://bob-nip65-reroute.invalid";

    let mute_list = signed_mute_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let bob_relay_list = signed_relay_list(&bob, &[nip65_target], 105);

    let mut relay = RecordingRelay::spawn(vec![mute_list, bob_relay_list]);
    let capture = Arc::new(CapturingReqInterceptor::default());
    let app = new_reduced_source_app_before_start();
    let app_ref = unsafe { &*app };
    assert_eq!(
        app_ref.set_req_frame_interceptor(capture.clone()),
        nmp_native_runtime::NmpConfigStatus::Ok
    );
    start_app(app);
    add_relay(app, relay.url());
    sign_in(app, &alice);
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.relay.list-members.nip65-reroute";
    let _handle = app_ref
        .open_feed(&mute_source_params(key), &compiler)
        .expect("active mute source opens");

    relay.wait_req("active mute-list source", |filter| {
        has_kind(filter, 10_000) && has_author(filter, &alice_pk)
    });
    let fallback_req = relay.wait_req("pre-NIP65 Bob author fallback sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk) && !has_author(filter, &alice_pk)
    });
    relay.wait_req("Bob kind:10002 mailbox probe", |filter| {
        has_kind(filter, 10_002) && has_author(filter, &bob_pk)
    });
    assert!(
        !fallback_req.sub_id.is_empty(),
        "pre-NIP-65 app-relay author sub must be a real wire subscription"
    );
    capture.wait_for("NIP-65 target Bob author REQ", |ctx| {
        captured_req_matches(ctx, nip65_target, &bob_pk, 1)
    });

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

#[test]
fn active_follows_account_switch_withdraws_old_relay_source() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(49);
    let bob = keys_from_byte(50);
    let carol = keys_from_byte(51);
    let dave = keys_from_byte(52);
    let alice_pk = alice.public_key().to_hex();
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    let dave_pk = dave.public_key().to_hex();

    let alice_contacts = signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let carol_contacts = signed_contact_list(&carol, std::slice::from_ref(&dave_pk), 130);
    let bob_note = signed_note(&bob, "alice follow over relay", 110);
    let dave_note = signed_note(&dave, "carol follow over relay", 120);
    let bob_note_id = bob_note.id.to_hex();
    let dave_note_id = dave_note.id.to_hex();

    let mut relay =
        RecordingRelay::spawn(vec![alice_contacts, carol_contacts, bob_note, dave_note]);
    let app = new_started_reduced_source_app();
    add_relay(app, relay.url());
    sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.relay.active-follows.account-switch";
    let _handle = app_ref
        .open_feed(&active_follows_params(key), &compiler)
        .expect("active follows opens");

    relay.wait_req("Alice active-account kind:3 source", |filter| {
        has_kind(filter, 3) && has_author(filter, &alice_pk)
    });
    let bob_req = relay.wait_req("Alice derived Bob author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk) && !has_author(filter, &dave_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    sign_in(app, &carol);
    wait_active(&rx, app_ref, &carol_pk);
    relay.wait_close("account switch closes old Bob author sub", &bob_req.sub_id);
    relay.wait_req("Carol active-account kind:3 source", |filter| {
        has_kind(filter, 3) && has_author(filter, &carol_pk)
    });
    relay.wait_req("Carol derived Dave author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &dave_pk) && !has_author(filter, &bob_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&dave_note_id));

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}

#[test]
fn active_mute_list_relay_uses_same_reduced_source_lifecycle() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();

    let alice = keys_from_byte(46);
    let bob = keys_from_byte(47);
    let carol = keys_from_byte(48);
    let bob_pk = bob.public_key().to_hex();
    let carol_pk = carol.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();

    let initial_mute = signed_mute_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let replacement_mute = signed_mute_list(&alice, std::slice::from_ref(&carol_pk), 130);
    let bob_note = signed_note(&bob, "relay muted bob", 110);
    let carol_note = signed_note(&carol, "relay muted carol", 120);
    let bob_note_id = bob_note.id.to_hex();
    let carol_note_id = carol_note.id.to_hex();

    let mut relay = RecordingRelay::spawn(vec![initial_mute.clone(), bob_note, carol_note]);
    let app = new_started_reduced_source_app();
    add_relay(app, relay.url());
    sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    wait_active(&rx, app_ref, &alice_pk);

    let key = "test.relay.active-mute.replace";
    let _handle = app_ref
        .open_feed(&mute_source_params(key), &compiler)
        .expect("active mute source opens");

    relay.wait_req("active mute-list source", |filter| {
        has_kind(filter, 10_000) && has_author(filter, &alice_pk)
    });
    let bob_req = relay.wait_req("derived mute Bob author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &bob_pk) && !has_author(filter, &carol_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&bob_note_id));

    relay.push_event(replacement_mute);
    relay.wait_close("withdrawn mute Bob author sub", &bob_req.sub_id);
    relay.wait_req("derived mute Carol author sub", |filter| {
        has_kind(filter, 1) && has_author(filter, &carol_pk) && !has_author(filter, &bob_pk)
    });
    wait_feed_ids(&rx, app_ref, key, std::slice::from_ref(&carol_note_id));

    unsafe { drop(Box::from_raw(app)) };
    uninstall_update_signal();
}
