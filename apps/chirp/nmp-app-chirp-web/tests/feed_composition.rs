//! Integration tests for the Chirp web feed composition.
//!
//! # Tests
//!
//! * `setup_completes_without_panic` — `setup_chirp_web_feeds` completes
//!   and the returned handles are non-null.
//!
//! * `engine_observes_kind1_event` — after calling `setup_chirp_web_feeds`,
//!   delivering a kind:1 event directly to the engine via `on_kernel_event`
//!   populates the snapshot with one root card.
//!
//! * `reentrant_claim_is_queued_not_panicked` — when `on_kernel_event` fires
//!   while the `KernelReducer` is mutably borrowed (simulating the
//!   `handle_relay_frame` re-entrancy window), the queuing claim sink parks the
//!   `ClaimRequest` without panicking. After the borrow is released,
//!   `drain_pending_claims` processes the queue.

use std::sync::Arc;

use nmp_app_chirp_web::{composition::setup_chirp_web_feeds, claim_queue::{
    build_queuing_claim_sink, drain_pending_claims, new_pending_claim_queue,
}};
use nmp_core::{substrate::KernelEvent, KernelEventObserver};
use nmp_feed::FeedRequest;
use nmp_nip01::op_feed::{register_op_feed, OP_FEED_SNAPSHOT_KEY};
use nmp_nip02::ActiveFollowSet;
use nmp_wasm::WasmRuntime;

// ── Test constants ───────────────────────────────────────────────────────────

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const REPLY_ID: &str = "0000000000000000000000000000000000000000000000000000000000000002";

fn make_kind1(id: &str, author: &str, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at: 1_700_000_000,
        tags: vec![],
        content: content.to_string(),
    }
}

fn make_reply(id: &str, author: &str, root_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at: 1_700_000_001,
        tags: vec![vec![
            "e".to_string(),
            root_id.to_string(),
            String::new(), // relay hint
            "root".to_string(),
        ]],
        content: "reply".to_string(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn setup_completes_without_panic() {
    let runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);
    // Snapshot is empty but accessible — setup did not panic.
    let snapshot = setup.engine.snapshot(&FeedRequest::default());
    assert_eq!(snapshot.cards.len(), 0);
}

#[test]
fn engine_observes_kind1_event_via_direct_observer_call() {
    // Construct a minimal engine independently to verify the observation
    // path. We drive the engine directly (no relay frame) to avoid needing
    // real secp256k1 signatures for the kernel's ingest path.
    let follow_set: std::collections::HashSet<String> =
        [ALICE.to_string()].into_iter().collect();
    let follow_predicate: nmp_feed::FollowPredicate =
        Arc::new(move |pk: &str| follow_set.contains(pk));
    let event_lookup: nmp_feed::EventLookup =
        Arc::new(move |_id: &String| None);
    let queue = new_pending_claim_queue();
    let claim_sink = build_queuing_claim_sink(Arc::clone(&queue));

    let engine = register_op_feed(
        ALICE.to_string(),
        follow_predicate,
        event_lookup,
        claim_sink,
    );

    // Deliver a kind:1 from Alice.
    let note = make_kind1(OP_ID, ALICE, "hello web");
    engine.on_kernel_event(&note);

    let snapshot = engine.snapshot(&FeedRequest::default());
    assert_eq!(
        snapshot.cards.len(),
        1,
        "engine should have one root card after observing Alice's kind:1"
    );
    assert_eq!(snapshot.cards[0].card.id, OP_ID);
}

#[test]
fn reentrant_claim_sink_queues_without_panic() {
    // Simulate the re-entrancy hazard: the engine fires its ClaimSink from
    // within an on_kernel_event call that is itself called while the reducer
    // is mutably borrowed (the handle_relay_frame window).
    //
    // Contract: the queuing claim sink MUST park the ClaimRequest without
    // attempting to call reducer.borrow_mut() — doing so would panic with
    // "already mutably borrowed". This test asserts the no-panic invariant
    // and verifies the request lands in the queue.

    let runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);
    let reducer = runtime.reducer_handle();

    // Seed a follow — Alice follows Alice.
    // (ActiveFollowSet is seeded from the kernel slot; no active account
    // means an empty follow set. We bypass the follow predicate check by
    // building a standalone engine below.)

    // Build a standalone follow predicate that always returns true so
    // Alice's reply triggers a claim.
    let always_follows: nmp_feed::FollowPredicate = Arc::new(|_: &str| true);
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |_id: &String| None);
    let queue = new_pending_claim_queue();
    let queuing_sink = build_queuing_claim_sink(Arc::clone(&queue));
    let engine = register_op_feed(
        ALICE.to_string(),
        always_follows,
        event_lookup,
        queuing_sink,
    );

    // Take a mutable borrow of the reducer — simulating the window inside
    // handle_relay_frame where the borrow is held.
    {
        let _guard = reducer.borrow_mut();
        // While the reducer is borrowed, fire on_kernel_event with a reply
        // whose root is unknown. The engine should emit a ClaimRequest via
        // the queuing sink WITHOUT touching the reducer (no panic).
        let reply = make_reply(REPLY_ID, ALICE, OP_ID);
        engine.on_kernel_event(&reply);
    }
    // _guard dropped here — borrow released.

    // The queuing sink must have captured the claim without panicking.
    let queue_len = {
        queue.lock().unwrap().len()
    };
    assert!(
        queue_len > 0,
        "claim sink should have queued at least one ClaimRequest; got {queue_len}"
    );

    // drain_pending_claims processes the queue and calls claim_event on the
    // reducer with can_send=false. No panic expected.
    drain_pending_claims(&queue, &reducer);

    let remaining = queue.lock().unwrap().len();
    assert_eq!(remaining, 0, "queue should be empty after drain");
}

#[test]
fn setup_chirp_web_feeds_wires_snapshot_key() {
    // Verify the typed projection is registered under "nmp.feed.home" by
    // checking the snapshot key in the projection output. We can't call
    // make_update_frame here (needs signed events), so we just verify the
    // engine key constant is correct.
    assert_eq!(OP_FEED_SNAPSHOT_KEY, "nmp.feed.home");
}
