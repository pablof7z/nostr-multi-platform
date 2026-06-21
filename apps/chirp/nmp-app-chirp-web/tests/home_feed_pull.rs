//! ADR-0058 seq-ordered PULL scrolling — Chirp WEB home-feed pager engagement.
//!
//! The web twin of `apps/chirp/nmp-app-chirp/src/ffi/tests/home_feed_pull.rs`
//! and `crates/nmp-defaults/tests/pull_feed_seq1_e2e.rs`, driven through the
//! REAL wasm surfaces:
//!
//!   * the `PullFeedController` registered by `setup_chirp_web_feeds` under
//!     `nmp.feed.home` (proving the composition wires the pager, not just the
//!     typed projection), and
//!   * the `WorkerRequest::LoadOlderFeed` handler on `WasmRuntime` (proving the
//!     command round-trips and reaches the controller).

use nmp_nip01::op_feed::OP_FEED_SNAPSHOT_KEY;
use nmp_wasm::{WasmRuntime, WorkerEvent, WorkerRequest};

use nmp_app_chirp_web::composition::setup_chirp_web_feeds;

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";

/// The WEB composition's `nmp.feed.home` engages the seq-ordered PULL pager on
/// a `LoadOlderFeed` request, and a late-old event surfaces.
///
/// Events are inserted DIRECTLY into the kernel event store (the ingest log)
/// WITHOUT firing observers, so the engine learns of them ONLY via the pull
/// drain — exactly the ADR-0058 §1 scenario. A `created_at` cursor would skip
/// the late-old event; the seq cursor does not.
#[test]
fn load_older_feed_engages_home_pull_pager_and_surfaces_late_old_event() {
    use nmp_core::store::{RawEvent, VerifiedEvent};

    let mut runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);

    // ALICE active → the live, fail-closed shape resolves authors = {ALICE}
    // (self-inclusion) and kinds = {1,6}, so the pull covers ALICE's notes.
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());
    setup.notify_account_changed();

    let store = runtime.reducer_handle().borrow().event_store_handle();
    let relay = "wss://test.relay/".to_string();
    let insert = |id: &str, created_at: u64| {
        store
            .insert(
                VerifiedEvent::from_raw_unchecked(RawEvent {
                    id: id.to_string(),
                    pubkey: ALICE.to_string(),
                    created_at,
                    kind: 1,
                    tags: vec![],
                    content: format!("note {id}"),
                    sig: "a".repeat(128),
                }),
                &relay,
                1_000,
            )
            .expect("store insert must succeed");
    };

    // Two recent events — store-only, the engine has NOT observed them.
    insert(&"a".repeat(64), 1_000);
    insert(&"b".repeat(64), 1_100);

    let before = setup.engine.snapshot_current_window();
    assert_eq!(
        before.cards.len(),
        0,
        "engine must be empty before load_older (events are store-only, never pushed)"
    );

    // Tail reached → LoadOlderFeed (the wasm twin of nmp_app_load_older_feed).
    let events = runtime
        .handle(WorkerRequest::LoadOlderFeed {
            feed_key: OP_FEED_SNAPSHOT_KEY.to_string(),
            correlation_id: "load-older-1".to_string(),
        })
        .expect("LoadOlderFeed must be accepted");
    assert!(
        events.iter().any(|e| matches!(
            e,
            WorkerEvent::ActionAccepted { action_type, .. } if action_type == "nmp.feed.load_older"
        )),
        "LoadOlderFeed must round-trip an ActionAccepted; got {events:?}"
    );

    // The pager engaged: both ALICE notes now surface in the grown window.
    let after = setup.engine.snapshot_current_window();
    let ids: Vec<String> = after.cards.iter().map(|c| c.card.id.clone()).collect();
    assert_eq!(
        ids.len(),
        2,
        "home pull pager must surface both ALICE notes after load_older; got {ids:?}"
    );

    // §1 completeness — a LATE-OLD event (low created_at, HIGHER seq) inserted
    // after the first drain. The seq cursor sees it even though its timestamp
    // falls below the already-displayed window.
    insert(&"c".repeat(64), 10);
    runtime
        .handle(WorkerRequest::LoadOlderFeed {
            feed_key: OP_FEED_SNAPSHOT_KEY.to_string(),
            correlation_id: "load-older-2".to_string(),
        })
        .expect("second LoadOlderFeed must be accepted");

    let grown = setup.engine.snapshot_current_window();
    let grown_ids: Vec<String> = grown.cards.iter().map(|c| c.card.id.clone()).collect();
    assert_eq!(
        grown_ids.len(),
        3,
        "the late-old event must surface via the seq cursor; got {grown_ids:?}"
    );
    assert!(
        grown_ids.contains(&"c".repeat(64)),
        "late-old event id must be present after the second load_older; got {grown_ids:?}"
    );
}
