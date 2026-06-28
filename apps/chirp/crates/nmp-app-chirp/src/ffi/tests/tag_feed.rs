//! #1740 step 6 — the migrated hashtag feed produces a real feed snapshot
//! through the typed `open_feed` session path.
//!
//! Before the migration `nmp_app_chirp_open_tag_feed` opened a bare kernel `#t`
//! interest with NO render. After the migration it routes through
//! `NmpApp::open_feed(FeedParams{Tag})` → `compile_feed_params`, which opens the
//! SAME `#t` acquisition interest AND registers an event-aware `#t` render engine
//! + typed sidecar under `nmp.feed.tag.<tag>`. These tests prove:
//!
//!   1. a `#t`-tagged note injected then opened appears in the migrated tag
//!      feed's typed sidecar (the feed produces a real snapshot — proof the
//!      session render path is wired, not just the interest);
//!   2. a note WITHOUT the tag does not leak into the feed (the event-aware `#t`
//!      admission gates correctly);
//!   3. `close_tag_feed` tears the session down (the sidecar stops emitting).
//!
//! Harness mirrors `ffi/interest_feed/tests.rs` (the ADR-0062 actor harness):
//! start the actor, inject signed events, block until the kernel read-cache has
//! them, open the feed (replay delivers the cached events), then block until the
//! typed sidecar carries the expected ids.

use std::ffi::{c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use nmp_ffi::{
    nmp_app_free, nmp_app_inject_signed_event_json, nmp_app_new, nmp_app_set_update_callback,
    nmp_app_start, NmpApp,
};
use nostr::prelude::JsonUtil;
use nostr::{EventBuilder, Keys, Tag, Timestamp};

use super::super::{nmp_app_chirp_close_tag_feed, nmp_app_chirp_open_tag_feed};

extern "C" fn update_signal(ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if ctx.is_null() {
        return;
    }
    let tx: &Sender<()> = unsafe { &*(ctx as *const Sender<()>) };
    let _ = tx.send(());
}

fn start_app() -> (*mut NmpApp, Receiver<()>, Box<Sender<()>>) {
    let app = nmp_app_new();
    assert!(!app.is_null(), "nmp_app_new must succeed");
    let (tx, rx) = channel::<()>();
    let tx_box = Box::new(tx);
    let ctx = tx_box.as_ref() as *const Sender<()> as *mut c_void;
    nmp_app_set_update_callback(app, ctx, Some(update_signal));
    nmp_app_start(app, 80, 4);
    (app, rx, tx_box)
}

fn inject_and_wait(app: *mut NmpApp, json: &str, id: &str, rx: &Receiver<()>) {
    let json_c = CString::new(json).expect("event JSON");
    let ok = nmp_app_inject_signed_event_json(app, json_c.as_ptr());
    assert!(ok, "inject must succeed for: {json}");
    let app_ref: &NmpApp = unsafe { &*app };
    if app_ref.event_by_id(id).is_some() {
        return;
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) if app_ref.event_by_id(id).is_some() => return,
            Ok(()) => {}
            Err(_) => panic!(
                "actor timed out making event {} readable",
                &id[..16.min(id.len())]
            ),
        }
    }
}

/// Decode the op-feed card ids carried by the typed sidecar for `key`.
fn read_tag_card_ids(app: *mut NmpApp, key: &str) -> Vec<String> {
    let app_ref: &NmpApp = unsafe { &*app };
    let projections = app_ref.run_typed_snapshot_projections();
    let Some(entry) = projections
        .iter()
        .find(|p| p.key == key && !p.payload.is_empty())
    else {
        return Vec::new();
    };
    let Ok(snapshot) = nmp_nip01::op_feed::decode_op_feed_snapshot(&entry.payload) else {
        return Vec::new();
    };
    snapshot.cards.iter().map(|c| c.card.id.clone()).collect()
}

/// Block until the tag sidecar for `key` carries `expected` cards; return ids.
fn wait_for_tag_cards(
    app: *mut NmpApp,
    key: &str,
    expected: usize,
    rx: &Receiver<()>,
) -> Vec<String> {
    let ids = read_tag_card_ids(app, key);
    if ids.len() >= expected {
        return ids;
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {
                let ids = read_tag_card_ids(app, key);
                if ids.len() >= expected {
                    return ids;
                }
            }
            Err(_) => {
                let ids = read_tag_card_ids(app, key);
                panic!(
                    "timed out waiting for {expected} cards in {key} (got {})",
                    ids.len()
                );
            }
        }
    }
}

/// Sign a kind:1 note carrying the given `#t` hashtags.
fn tagged_note(keys: &Keys, body: &str, hashtags: &[&str], ts: u64) -> nostr::Event {
    let mut builder = EventBuilder::text_note(body).custom_created_at(Timestamp::from(ts));
    for t in hashtags {
        builder = builder.tag(Tag::hashtag(*t));
    }
    builder.sign_with_keys(keys).expect("sign note")
}

#[test]
fn open_tag_feed_renders_matching_notes_via_typed_session() {
    let (app, rx, _tx_box) = start_app();
    let keys = Keys::generate();

    // The session engine anchors on the active viewer (the perspective owner);
    // with no active account `open_feed` fails closed (`scope-no-active-account`).
    // Sign in a viewer so the Tag session opens.
    {
        let app_ref: &NmpApp = unsafe { &*app };
        *app_ref.active_account_handle().lock().expect("active slot") =
            Some(keys.public_key().to_hex());
    }

    // Open via the migrated typed path FIRST (fire-and-forget; no raw
    // open_interest). The session registers its event-aware `#t` ingest observer
    // + render engine; live-injected matching notes flow straight into the feed.
    let tag_c = CString::new("nostr").unwrap();
    nmp_app_chirp_open_tag_feed(app, tag_c.as_ptr());
    let key = "nmp.feed.tag.nostr";

    // Two notes carry the `#nostr` hashtag; both must enter the tag feed.
    let n1 = tagged_note(&keys, "older nostr post", &["nostr"], 1_000);
    let n2 = tagged_note(&keys, "newer nostr post", &["nostr"], 2_000);
    // A note WITHOUT the tag must NOT leak into the feed (event-aware `#t`
    // admission, not blanket admit-any).
    let off_topic = tagged_note(&keys, "bitcoin only", &["bitcoin"], 3_000);

    inject_and_wait(app, &n1.as_json(), &n1.id.to_hex(), &rx);
    inject_and_wait(app, &n2.as_json(), &n2.id.to_hex(), &rx);
    inject_and_wait(app, &off_topic.as_json(), &off_topic.id.to_hex(), &rx);

    let app_ref: &NmpApp = unsafe { &*app };
    let ids = wait_for_tag_cards(app, key, 2, &rx);

    assert!(ids.contains(&n1.id.to_hex()), "tagged note n1 must render");
    assert!(ids.contains(&n2.id.to_hex()), "tagged note n2 must render");
    assert!(
        !ids.contains(&off_topic.id.to_hex()),
        "an untagged note must NOT leak into the #nostr feed (event-aware admission)"
    );

    // Close via the typed handle (re-derived from the tag key map) tears the
    // session down — the sidecar stops carrying a live payload.
    let close_c = CString::new("nostr").unwrap();
    nmp_app_chirp_close_tag_feed(app, close_c.as_ptr());

    use nmp_core::WireProjectionState;
    let post_close = app_ref.run_typed_snapshot_projections();
    let live_after = post_close
        .iter()
        .any(|p| p.key == key && p.state != WireProjectionState::Cleared && !p.payload.is_empty());
    assert!(!live_after, "no live tag sidecar payload after close");

    nmp_app_free(app);
}

/// Late-joiner back-fill now WORKS for `#t` hashtag feeds: the general
/// single-letter-tag store query (`StoreQuery::Tags`, #2100) replaced the old
/// `#e`/`#p`-only special cases, so a `#t`-only shape is store-queryable instead
/// of `UnsupportedInterestShape`. A note cached BEFORE `open_tag_feed` is now
/// surfaced from the store on open — not only LIVE matching events.
///
/// This is the "green flip" the prior known-limitation test predicted: the
/// assertion below now expects the pre-cached note to be back-filled.
#[test]
fn pre_cached_tag_notes_are_pull_backfilled() {
    let (app, rx, _tx_box) = start_app();
    let keys = Keys::generate();
    let app_ref: &NmpApp = unsafe { &*app };
    *app_ref.active_account_handle().lock().expect("active slot") =
        Some(keys.public_key().to_hex());

    let tag = "latejoiner-probe";
    let key = format!("nmp.feed.tag.{tag}");

    // Inject + cache-confirm a matching note BEFORE the feed is opened.
    let cached = tagged_note(&keys, "cached before open", &[tag], 1_000);
    inject_and_wait(app, &cached.as_json(), &cached.id.to_hex(), &rx);

    // Open AFTER the note is already cached, then drive the pull repeatedly.
    let tag_c = CString::new(tag).unwrap();
    nmp_app_chirp_open_tag_feed(app, tag_c.as_ptr());
    for _ in 0..8 {
        let _ = app_ref.load_older_feed(&key);
        let _ = rx.recv_timeout(Duration::from_millis(100));
    }

    // The `#t` shape is now covered by `StoreQuery::Tags` (#2100), so the
    // pre-cached note IS back-filled from the store on open.
    let ids = read_tag_card_ids(app, &key);
    assert!(
        ids.contains(&cached.id.to_hex()),
        "pre-cached #t note IS pull-backfilled now that StoreQuery::Tags covers #t (#2100)"
    );

    let close_c = CString::new(tag).unwrap();
    nmp_app_chirp_close_tag_feed(app, close_c.as_ptr());
    nmp_app_free(app);
}

/// **Session-engine structural pairing (#1740 — the open_feed author-migration
/// unblocker)**: a Tag-scope feed opened through `open_feed` →
/// `compile_feed_params` → `build_scope_session` MUST register its feed-author
/// auto-resolve provider STRUCTURALLY PAIRED with its typed sidecar (ADR-0063 D7,
/// #1671 Lane H) — exactly like the FlatFeed author/thread path. Before this fix
/// `build_scope_session` installed ONLY the typed sidecar (bare
/// `register_typed_snapshot_projection`, NO provider), so EVERY session-engine
/// scope (Authors/Tag/List/Wot) would render with blank avatars. Migrating a Chirp
/// feed onto the session engine would have REGRESSED avatars.
///
/// This test is LOAD-BEARING: it asserts the provider is present while the session
/// feed is open, surfaces the visible card author for auto-resolve, and is RELEASED
/// on close (no leak). If `build_scope_session` reverted to the bare typed-only
/// registration, the provider-key assertion would fail.
#[test]
fn session_engine_tag_feed_registers_author_provider_structurally_and_releases_on_close() {
    let (app, rx, _tx_box) = start_app();
    let keys = Keys::generate();
    let author = keys.public_key().to_hex();
    let app_ref: &NmpApp = unsafe { &*app };
    // The session engine anchors on the active viewer; sign one in so the Tag
    // session opens (no active account fails closed).
    *app_ref.active_account_handle().lock().expect("active slot") = Some(author.clone());

    let tag = "structural-provider-probe";
    let key = format!("nmp.feed.tag.{tag}");
    let tag_c = CString::new(tag).unwrap();
    nmp_app_chirp_open_tag_feed(app, tag_c.as_ptr());

    // A `#t`-tagged note so the session's visible window carries a real author.
    let note = tagged_note(&keys, "tagged for provider", &[tag], 1_000);
    inject_and_wait(app, &note.as_json(), &note.id.to_hex(), &rx);
    let _ = wait_for_tag_cards(app, &key, 1, &rx);

    // STRUCTURAL: the session feed registered BOTH lanes under the SAME key.
    assert!(
        app_ref.registered_typed_projection_keys().contains(&key),
        "session-engine tag feed typed sidecar must be registered"
    );
    assert!(
        app_ref
            .registered_feed_author_provider_keys()
            .contains(&key),
        "session-engine tag feed MUST have a structurally-paired author provider \
         (else open_feed migration regresses avatars — the #1740 gap this fixes)"
    );

    // The provider surfaces the feed's visible card author → it auto-resolves.
    assert!(
        app_ref
            .run_feed_author_provider_for_test(&key)
            .contains(&author),
        "the session author provider returns the visible author (auto-resolved via resolve_ref)"
    );

    // Close → BOTH lanes gone (no provider leak).
    let close_c = CString::new(tag).unwrap();
    nmp_app_chirp_close_tag_feed(app, close_c.as_ptr());
    assert!(
        !app_ref
            .registered_feed_author_provider_keys()
            .contains(&key),
        "session author provider released after close (no leak — paired teardown)"
    );

    nmp_app_free(app);
}

#[test]
fn close_unopened_tag_feed_is_a_no_op() {
    let (app, _rx, _tx_box) = start_app();
    // Closing a tag that was never opened must be a harmless no-op (D6).
    let tag_c = CString::new("never-opened").unwrap();
    nmp_app_chirp_close_tag_feed(app, tag_c.as_ptr());
    nmp_app_free(app);
}

/// Re-opening the SAME tag before closing must (a) NOT leak the prior session
/// and (b) NOT clobber the replacement. `FeedSessionRegistry::open` mints a fresh
/// session id each open, and both sessions share the `nmp.feed.tag.<tag>`
/// projection key — so the prior session MUST be torn down BEFORE the replacement
/// registers (closing it after would unregister the new feed's controller). This
/// proves both: `live_feed_session_count` never exceeds 1 and the REOPENED feed
/// still renders a live note (its controller/projection survived the re-open),
/// and one close returns the count to 0.
#[test]
fn reopen_same_tag_keeps_one_live_session_and_replacement_renders() {
    let (app, rx, _tx_box) = start_app();
    let keys = Keys::generate();
    let app_ref: &NmpApp = unsafe { &*app };
    *app_ref.active_account_handle().lock().expect("active slot") =
        Some(keys.public_key().to_hex());

    // A tag unique to this test: `OPEN_TAGS` is a process-global static, so a
    // shared tag could race with the render test under parallel execution.
    let tag = "reopen-leak-probe";
    let key = format!("nmp.feed.tag.{tag}");
    let tag_c = CString::new(tag).unwrap();
    nmp_app_chirp_open_tag_feed(app, tag_c.as_ptr());
    assert_eq!(
        app_ref.live_feed_session_count(),
        1,
        "first open mints one session"
    );

    // Re-open WITHOUT closing — the prior session is torn down FIRST, leaving
    // exactly one live session (not two), and the replacement is the survivor.
    let tag_c2 = CString::new(tag).unwrap();
    nmp_app_chirp_open_tag_feed(app, tag_c2.as_ptr());
    assert_eq!(
        app_ref.live_feed_session_count(),
        1,
        "re-open replaces the prior session — no leak, no double session"
    );

    // The REPLACEMENT must still be wired: a live matching note renders through
    // its controller/projection (proving the prior session's key-based teardown
    // did NOT remove the new feed's registrations).
    let note = tagged_note(&keys, "post after reopen", &[tag], 5_000);
    inject_and_wait(app, &note.as_json(), &note.id.to_hex(), &rx);
    let ids = wait_for_tag_cards(app, &key, 1, &rx);
    assert!(
        ids.contains(&note.id.to_hex()),
        "the reopened tag feed still renders matching notes (replacement not clobbered)"
    );

    // A single close returns to zero — there is no leaked second session left.
    let close_c = CString::new(tag).unwrap();
    nmp_app_chirp_close_tag_feed(app, close_c.as_ptr());
    assert_eq!(
        app_ref.live_feed_session_count(),
        0,
        "one close tears the single live session down — nothing leaked"
    );

    nmp_app_free(app);
}

/// Parked tag handles are keyed by the OWNING app, so two live apps' tag
/// sessions are disjoint: opening the same tag on app A and app B yields two
/// independent sessions, and closing the tag on A leaves B's session live (a
/// `FeedHandle.session_id` is app-scoped — a tag-only key would let A's close
/// clobber B's same-id session). Proven by per-app `live_feed_session_count`.
#[test]
fn two_apps_same_tag_are_disjoint_sessions() {
    let (app_a, _rx_a, _tx_a) = start_app();
    let (app_b, _rx_b, _tx_b) = start_app();
    let a_ref: &NmpApp = unsafe { &*app_a };
    let b_ref: &NmpApp = unsafe { &*app_b };
    *a_ref.active_account_handle().lock().expect("a slot") =
        Some(Keys::generate().public_key().to_hex());
    *b_ref.active_account_handle().lock().expect("b slot") =
        Some(Keys::generate().public_key().to_hex());

    let tag = "two-apps-probe";
    let tag_c_a = CString::new(tag).unwrap();
    let tag_c_b = CString::new(tag).unwrap();
    nmp_app_chirp_open_tag_feed(app_a, tag_c_a.as_ptr());
    nmp_app_chirp_open_tag_feed(app_b, tag_c_b.as_ptr());
    assert_eq!(
        a_ref.live_feed_session_count(),
        1,
        "app A has its own session"
    );
    assert_eq!(
        b_ref.live_feed_session_count(),
        1,
        "app B has its own session"
    );

    // Close the tag on A only — B's session must survive (no cross-app clobber).
    let close_a = CString::new(tag).unwrap();
    nmp_app_chirp_close_tag_feed(app_a, close_a.as_ptr());
    assert_eq!(
        a_ref.live_feed_session_count(),
        0,
        "app A's session torn down"
    );
    assert_eq!(
        b_ref.live_feed_session_count(),
        1,
        "app B's session is untouched by closing the same tag on app A"
    );

    let close_b = CString::new(tag).unwrap();
    nmp_app_chirp_close_tag_feed(app_b, close_b.as_ptr());
    assert_eq!(
        b_ref.live_feed_session_count(),
        0,
        "app B's session torn down"
    );

    nmp_app_free(app_a);
    nmp_app_free(app_b);
}
