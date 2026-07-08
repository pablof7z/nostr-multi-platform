//! NMP#2944 regression: the app-owned OP-feed projection (`chirp.timeline.home`)
//! must emit a `projection_rev` that ADVANCES when its content changes.
//!
//! Rev-aware host apply caches (the generated iOS `ProjectionCache.generated.swift`
//! and the Android `ProjectionCache.kt`) skip a `Changed` row when
//! `incomingRev <= cached.rev`. Before the fix, app-owned keys (absent from the
//! kernel's built-in rev manifest) emitted `projection_rev == 0` on EVERY tick,
//! so a host committed the first (empty) home-feed frame and then skipped every
//! later card-bearing frame — the home feed rendered empty forever while the
//! frame-level envelope rev kept climbing ("decodedRev advanced but no card").
//!
//! These tests assert on the REAL emitted frame bytes (captured off the update
//! listener and decoded exactly as a host does), not the out-of-band
//! `run_typed_snapshot_projections` recompute accessor.

#[path = "common/mod.rs"]
mod common;
#[path = "reduced_source_relay_e2e/support.rs"]
mod support;

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use common::recording_relay::{has_author, has_kind, RecordingRelay};
use nmp_native_runtime::NmpApp;
use support::*;

/// One emitted frame's view of the home-feed typed projection.
#[derive(Clone, Debug)]
struct HomeFrame {
    projection_rev: u64,
    card_ids: Vec<String>,
}

static FRAMES: OnceLock<Mutex<Vec<HomeFrame>>> = OnceLock::new();
static HOME_KEY: OnceLock<String> = OnceLock::new();

fn frames() -> &'static Mutex<Vec<HomeFrame>> {
    FRAMES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Decode the real emitted frame exactly as a host does: lift the typed
/// projection sidecar and the OP-feed payload for the home-feed key.
fn capture_frame(bytes: &[u8]) {
    let Some(key) = HOME_KEY.get() else { return };
    let Ok(projections) = nmp_core::decode_snapshot_typed_projections(bytes) else {
        return;
    };
    let Some(entry) = projections.iter().find(|e| e.key == key.as_str()) else {
        return;
    };
    let card_ids = nmp_feed::typed_wire::decode_feed_row_snapshot(&entry.payload)
        .map(|snap| snap.cards.into_iter().map(|c| c.card.canonical_row_id).collect::<Vec<_>>())
        .unwrap_or_default();
    if let Ok(mut g) = frames().lock() {
        g.push(HomeFrame {
            projection_rev: entry.projection_rev,
            card_ids,
        });
    }
}

fn new_capturing_app() -> *mut NmpApp {
    let app = Box::into_raw(Box::new(nmp_native_runtime::new_app()));
    nmp_substrate::install(
        unsafe { &mut *app },
        nmp_substrate::SubstrateConfig::default(),
    );
    unsafe { &*app }.set_update_listener(Some(std::sync::Arc::new(|bytes: &[u8]| {
        capture_frame(bytes);
    })));
    unsafe { &*app }.start_runtime(256, 8);
    app
}

fn wait_active_poll(app: &NmpApp, pubkey: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if app
            .active_account_handle()
            .lock()
            .map(|g| g.as_deref() == Some(pubkey))
            .unwrap_or(false)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("active account never set");
}

fn wait_for_card(note_id: &str, secs: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        if frames()
            .lock()
            .map(|g| g.iter().any(|f| f.card_ids.iter().any(|id| id == note_id)))
            .unwrap_or(false)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The root-cause regression: across an empty→cards content change, the emitted
/// `projection_rev` for the app-owned home-feed key must STRICTLY ADVANCE.
#[test]
fn op_feed_projection_rev_advances_on_content_change() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    frames().lock().unwrap_or_else(|e| e.into_inner()).clear();

    let alice = keys_from_byte(197);
    let bob = keys_from_byte(198);
    let bob_pk = bob.public_key().to_hex();
    let alice_pk = alice.public_key().to_hex();

    let contacts = signed_contact_list(&alice, std::slice::from_ref(&bob_pk), 100);
    let bob_note = signed_note(&bob, "rev-advance bob", 110);
    let bob_note_id = bob_note.id.to_hex();

    let key = "op_feed_rev.home".to_string();
    let _ = HOME_KEY.set(key.clone());

    // Relay holds only the contact list — Bob's note arrives live post-EOSE, so
    // the emitted frames carry a clean empty→cards transition.
    let mut relay = RecordingRelay::spawn(vec![contacts]);
    let app = new_capturing_app();
    add_relay(app, relay.url());
    sign_in(app, &alice);
    let app_ref = unsafe { &*app };
    wait_active_poll(app_ref, &alice_pk);

    let _handle = app_ref
        .open_feed(&support::active_follows_params(&key))
        .expect("home feed opens");
    relay.wait_req("alice kind:3", |f| has_kind(f, 3) && has_author(f, &alice_pk));
    relay.wait_req("bob kind:1", |f| has_kind(f, 1) && has_author(f, &bob_pk));
    std::thread::sleep(Duration::from_millis(300));
    relay.push_event(bob_note);

    assert!(wait_for_card(&bob_note_id, 10), "the card frame is emitted");
    let g = frames().lock().unwrap_or_else(|e| e.into_inner());
    let empty_rev = g
        .iter()
        .filter(|f| f.card_ids.is_empty())
        .map(|f| f.projection_rev)
        .max();
    let card_rev = g
        .iter()
        .find(|f| f.card_ids.iter().any(|id| id == &bob_note_id))
        .map(|f| f.projection_rev);
    assert!(
        matches!((empty_rev, card_rev), (Some(e), Some(c)) if c > e),
        "app-owned op-feed projection_rev must advance across empty→cards \
         (empty={empty_rev:?}, card={card_rev:?}); a frozen rev freezes rev-aware host caches"
    );

    drop(g);
    unsafe { drop(Box::from_raw(app)) };
}
