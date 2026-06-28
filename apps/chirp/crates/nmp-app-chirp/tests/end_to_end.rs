//! End-to-end: register Chirp through the FFI, drive synthetic kind:1
//! events through the actor's `IngestPreVerifiedEvents` channel, decode the
//! `"nmp.feed.home"` projection, and assert the OP-centric `RootFeedSnapshot`
//! lands.
//!
//! V-80 rung 7 — the home feed is now thread-ROOTS-only, produced by the
//! `nmp-nip01` OP-feed engine (via `nmp_native_runtime::register_op_feed_defaults`
//! wired in `nmp_app_chirp_register`). Replies no longer appear as their own
//! rows; a followed author's reply attributes back to its root. These tests
//! seed the active-account slot before registration so the home feed opens a
//! declared active-follows shape. The active account self-includes, so its root
//! notes surface; replies from non-followed authors are dropped.
//!
//! Mirrors the production flow `KernelBridge.swift` takes: open an `NmpApp`,
//! register the projection, watch ingest fan-out feed the engine, read the
//! snapshot. Bypasses the relay layer by pushing pre-verified events directly
//! into the actor command channel (`IngestPreVerifiedEvents` routes through
//! `kernel.ingest_pre_verified_event`, which fans out to observers without the
//! `timeline_authors` store gate — so the engine sees every injected event).

use std::ffi::CString;
use std::sync::Mutex;
use std::time::Duration;

use nmp_app_chirp::{
    nmp_app_chirp_register, nmp_app_chirp_unregister, ChirpHandle, NmpRegisterStatus,
    OpFeedSnapshot,
};

/// Convenience wrapper: register with no viewer pubkey (the common case in
/// these tests) and panic on failure.
fn register_app(app: *mut nmp_ffi::NmpApp) -> *mut ChirpHandle {
    let mut handle: *mut ChirpHandle = std::ptr::null_mut();
    let status = nmp_app_chirp_register(app, std::ptr::null(), &mut handle);
    assert_eq!(
        status,
        NmpRegisterStatus::Ok as u32,
        "register_app: unexpected status {status}"
    );
    assert!(!handle.is_null());
    handle
}

/// Sign in `pubkey` by writing the same active-account slot the actor writes
/// on real sign-in. `register_op_feed_defaults` reads this slot during Chirp
/// registration to open the home feed's declared active-follows shape.
fn set_active_account(app: *mut nmp_ffi::NmpApp, pubkey: &str) {
    let app_ref = unsafe { &*app };
    *app_ref.active_account_handle().lock().expect("active slot") = Some(pubkey.to_string());
}
use nmp_core::actor::ActorCommand;
use nmp_core::actor::TestSupportCommand;
use nmp_ffi::{nmp_app_free, nmp_app_load_older_feed, nmp_app_new, nmp_app_start};
use nmp_nip01::DEFAULT_TIMELINE_WINDOW_LIMIT;
use nmp_store::{RawEvent, VerifiedEvent};

// Serialize tests because `NmpApp` initialisation spawns process-global
// actor threads; staggering avoids cross-test interference.
static SERIAL: Mutex<()> = Mutex::new(());

fn raw_note(id: &str, author: &str, ts: u64, tags: Vec<Vec<String>>, content: &str) -> RawEvent {
    RawEvent {
        id: id.to_string(),
        pubkey: author.to_string(),
        created_at: ts,
        kind: 1,
        tags,
        content: content.to_string(),
        sig: "a".repeat(128),
    }
}

fn feed_projection_for(handle: *mut ChirpHandle) -> OpFeedSnapshot {
    // The generic JSON lane is deleted (rule A6). Snapshot directly from the
    // ChirpHandle's OpFeedEngine — the same data the typed FlatBuffers sidecar
    // encodes and sends to Swift.
    unsafe { &*handle }.snapshot()
}

fn inject(app: *mut nmp_ffi::NmpApp, events: Vec<VerifiedEvent>) {
    // SAFETY: `app` is a valid `*mut NmpApp` for the duration of this call
    // — caller passes the same handle they got from `nmp_app_new`.
    let app_ref = unsafe { &*app };
    let tx = app_ref.actor_sender();
    tx.send(ActorCommand::TestSupport(
        TestSupportCommand::IngestPreVerifiedEvents(events),
    ))
    .expect("actor command channel open");
}

#[test]
fn root_surfaces_and_unfollowed_reply_is_dropped() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    let root_id = "1".repeat(64);
    let reply_id = "2".repeat(64);
    let author = "a".repeat(64);
    let reply_author = "d".repeat(64);
    set_active_account(app, &author);
    let handle = register_app(app);
    nmp_app_start(app, 80, 4);

    // Root + one NIP-10-marked reply pointing at the root. The root author is
    // the active account, so the declared active-follows shape covers the root.
    // The reply author is not followed, so the reply is dropped: the feed shows
    // ONLY the root, with no attribution.
    let root = VerifiedEvent::from_raw_unchecked(raw_note(&root_id, &author, 1, vec![], "root"));
    let reply = VerifiedEvent::from_raw_unchecked(raw_note(
        &reply_id,
        &reply_author,
        2,
        vec![
            vec!["e".into(), root_id.clone(), "".into(), "root".into()],
            vec!["e".into(), root_id.clone(), "".into(), "reply".into()],
        ],
        "reply",
    ));

    inject(app, vec![root, reply]);

    // Wait for the actor to drain the injection. Two events are tiny; the
    // 250ms idle tick should be plenty even on a loaded CI machine.
    std::thread::sleep(Duration::from_millis(500));

    let snap = feed_projection_for(handle);
    assert_eq!(
        snap.cards.len(),
        1,
        "feed is roots-only: the root surfaces, the reply does not get a row; got {snap:?}"
    );
    assert_eq!(
        snap.cards[0].card.id, root_id,
        "the surfaced card is the root"
    );
    assert!(
        snap.cards[0].attribution.is_empty(),
        "the reply is from a non-followed author → no attribution attaches"
    );

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn standalone_note_renders_as_root_card() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    let id = "3".repeat(64);
    let author = "b".repeat(64);
    set_active_account(app, &author);
    let handle = register_app(app);
    nmp_app_start(app, 80, 4);

    let note = VerifiedEvent::from_raw_unchecked(raw_note(&id, &author, 1, vec![], "lone note"));
    inject(app, vec![note]);
    std::thread::sleep(Duration::from_millis(500));

    let snap = feed_projection_for(handle);
    assert_eq!(snap.cards.len(), 1, "one root card");
    assert_eq!(snap.cards[0].card.id, id);
    assert!(snap.cards[0].attribution.is_empty());

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}

#[test]
fn snapshot_returns_default_window() {
    let _g = SERIAL.lock().unwrap();

    let app = nmp_app_new();
    let author = "c".repeat(64);
    set_active_account(app, &author);
    let handle = register_app(app);
    nmp_app_start(app, 80, 4);

    let total = DEFAULT_TIMELINE_WINDOW_LIMIT + 2;
    let events = (0..total)
        .map(|idx| {
            let id = format!("{:064x}", idx + 1);
            VerifiedEvent::from_raw_unchecked(raw_note(
                &id,
                &author,
                (idx + 1) as u64,
                vec![],
                "note",
            ))
        })
        .collect::<Vec<_>>();
    inject(app, events);
    std::thread::sleep(Duration::from_millis(500));

    let snap = feed_projection_for(handle);
    assert_eq!(
        snap.cards.len(),
        DEFAULT_TIMELINE_WINDOW_LIMIT,
        "the default window caps the visible root cards"
    );
    let page = snap.page.expect("window snapshot carries page metadata");
    // The engine's RootFeedSnapshot does not emit per-tick timing metrics (V-80
    // §3-G removed them from this surface); it is always `None` here.
    assert!(snap.metrics.is_none(), "engine snapshot carries no metrics");
    assert!(page.has_more);
    assert_eq!(page.total_blocks, total);

    // ADR-0058 §8 6B + B1 fail-closed: `load_older` on the home feed is no
    // longer the OP engine's `created_at` window-grow (that path was deleted —
    // the engine is no longer a `FeedController`). It is now the
    // `PullFeedController`, whose live provider proves the signed-in active
    // account FIRST. This test has no backing store, so `load_older` must NOT
    // grow the window: it returns false (no covered pull progress). Growth on a
    // covered shape is proved by `nmp-defaults/tests/pull_feed_seq1_e2e.rs`.
    let key = CString::new("nmp.feed.home").expect("static key has no nul");
    nmp_app_load_older_feed(app, key.as_ptr());
    let after = feed_projection_for(handle);
    assert_eq!(
        after.cards.len(),
        DEFAULT_TIMELINE_WINDOW_LIMIT,
        "no backing store progress ⇒ load_older leaves the window unchanged"
    );
    let after_page = after
        .page
        .expect("window snapshot carries page metadata after load-older");
    assert!(
        after_page.has_more,
        "fail-closed load_older did not reveal more roots — page still has_more"
    );
    assert_eq!(after_page.total_blocks, total);

    nmp_app_chirp_unregister(handle);
    nmp_app_free(app);
}
