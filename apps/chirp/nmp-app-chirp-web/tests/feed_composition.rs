//! Integration tests for the Chirp web feed composition.

use std::sync::Arc;

use nmp_app_chirp_web::composition::setup_chirp_web_feeds;
use nmp_core::{substrate::KernelEvent, KernelEventObserver};
use nmp_feed::FeedRequest;
use nmp_nip01::op_feed::{register_op_feed, OP_FEED_SNAPSHOT_KEY};
use nmp_wasm::{ActionDispatch, WasmRuntime, WorkerRequest};

// ── Test constants ───────────────────────────────────────────────────────────

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const OP_ID2: &str = "0000000000000000000000000000000000000000000000000000000000000003";
const REPLY_ID: &str = "0000000000000000000000000000000000000000000000000000000000000002";

fn make_kind1(id: &str, author: &str, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at: 1_700_000_000,
        tags: vec![],
        content: content.to_string(),
        relay_provenance: Vec::new(),
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
        relay_provenance: Vec::new(),
    }
}

fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
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
    let follow_set: std::collections::HashSet<String> = [ALICE.to_string()].into_iter().collect();
    let follow_predicate: nmp_feed::FollowPredicate =
        Arc::new(move |pk: &str| follow_set.contains(pk));
    let event_lookup: nmp_feed::EventLookup = Arc::new(move |_id: &String| None);

    let engine = register_op_feed(ALICE.to_string(), follow_predicate, event_lookup);

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
fn setup_chirp_web_feeds_wires_snapshot_key() {
    // Verify the typed projection is registered under "nmp.feed.home" by
    // checking the snapshot key in the projection output. We can't call
    // make_update_frame here (needs signed events), so we just verify the
    // engine key constant is correct.
    assert_eq!(OP_FEED_SNAPSHOT_KEY, "nmp.feed.home");
}

// ── Wired-path tests (Blocking 6) ────────────────────────────────────────────
//
// These tests go through the REAL observer fan-out path: events are fired via
// `KernelReducer::fire_event_observers_for_test`, which invokes the same
// `notify_observers` slot that `Kernel::notify_event_observers` calls on
// production ingest. The engine and follow set receive events as registered
// observers, not through direct `on_kernel_event` calls.

#[test]
fn wired_path_follow_feed_populates_snapshot() {
    // End-to-end wired path proof:
    //   1. build KernelReducer + setup_chirp_web_feeds
    //   2. set active account (ALICE) → notify_account_changed seeds follow set
    //   3. deliver kind:1 from ALICE through the kernel observer slot
    //   4. assert the engine's snapshot contains one root card
    //
    // This is the load-bearing proof that the observer registration in
    // `setup_chirp_web_feeds` actually wires the engine into the kernel's
    // fan-out. The existing tests above drive the engine directly; this test
    // drives it through the registered slot.
    let runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);

    // Set ALICE as the active account. This writes to the `ActiveAccountSlot`
    // that `ActiveFollowSet` reads from.
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());

    // Notify the follow set of the account change. This seeds ALICE (self-
    // inclusion) into the follow set so the engine's follow predicate returns
    // `true` for ALICE's events and resets the feed perspective.
    setup.notify_account_changed();

    // Deliver ALICE's kind:1 through the wired observer slot — NOT via
    // engine.on_kernel_event directly. The engine must receive it via the
    // registered KernelEventObserver slot.
    let note = make_kind1(OP_ID, ALICE, "hello from the wired path");
    runtime
        .reducer_handle()
        .borrow()
        .fire_event_observers_for_test(&note);

    let snapshot = setup.engine.snapshot(&FeedRequest::default());
    assert_eq!(
        snapshot.cards.len(),
        1,
        "engine registered via setup_chirp_web_feeds must receive ALICE's kind:1          through the observer slot and surface it as a root card; got {}",
        snapshot.cards.len()
    );
    assert_eq!(
        snapshot.cards[0].card.id, OP_ID,
        "the surfaced card must carry ALICE's note id"
    );
}

#[test]
fn wired_kind3_parser_updates_kernel_follow_feed_authors() {
    // Regression guard for the wasm composition substrate wiring:
    //   1. web installs the same ContactsCache as kernel reader + kind:3 parser
    //   2. host opens the contact-feed interest for kinds 1/6
    //   3. the active account's kind:3 arrives through the projection chokepoint
    //   4. the kernel's follow-feed author set expands from self-only to include
    //      BOB, so the wasm relay pool can subscribe to BOB's notes.
    let mut runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);
    let reducer = runtime.reducer_handle();

    let _ = reducer.borrow_mut().set_active_account(ALICE.to_string());
    setup.notify_account_changed();

    runtime
        .handle(WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.kernel.open_contact_feed".to_string(),
            payload: serde_json::json!({ "kinds": [1] }),
            correlation_id: "open-contact-feed".to_string(),
        }))
        .expect("open_contact_feed dispatch must be accepted");

    reducer.borrow_mut().project_raw_event_for_test(
        "00000000000000000000000000000000000000000000000000000000000000f3",
        ALICE,
        1_700_000_010,
        3,
        vec![p_tag(BOB)],
        "",
    );

    let active_follow_set = setup.follow_set.follows();
    assert!(
        active_follow_set.contains(&BOB.to_string()),
        "ActiveFollowSet observer must see BOB in ALICE's kind:3; got {active_follow_set:?}",
    );

    let authors = reducer.borrow().active_timeline_authors();
    assert!(
        authors.contains(&BOB.to_string()),
        "kernel follow-feed authors must include BOB after ALICE's kind:3; got {authors:?}",
    );
    assert!(
        authors.contains(&ALICE.to_string()),
        "kernel follow-feed authors must retain self-inclusion; got {authors:?}",
    );
}

#[test]
fn wired_path_attribution_surfaces_when_missing_root_arrives() {
    // ADR-0035 product semantics on the wired path:
    //   A followed-user reply to a non-followed root surfaces that root with
    //   attribution after the root arrives, going through the registered
    //   observer slot. The feed does not claim or fetch the missing root.
    //
    // Sequence:
    //   1. setup + set ALICE as viewer → ALICE is a follow (self-inclusion)
    //   2. ALICE (follow) replies to BOB's OP (not yet seen) via observer
    //   3. assert no card yet — root absent
    //   4. BOB's root arrives via observer
    //   5. assert 1 card with ALICE's attribution
    let runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);

    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());
    setup.notify_account_changed();

    // Step 2: ALICE's reply to BOB's not-yet-seen root, via the wired slot.
    let reply = make_reply(REPLY_ID, ALICE, OP_ID);
    runtime
        .reducer_handle()
        .borrow()
        .fire_event_observers_for_test(&reply);

    // Step 3: root absent → no card.
    let before = setup.engine.snapshot(&FeedRequest::default());
    assert!(
        before.cards.is_empty(),
        "reply to absent root must not surface before the root arrives;          got {} cards",
        before.cards.len()
    );

    // Step 4: BOB's root arrives via the wired slot.
    let reducer = runtime.reducer_handle();
    let root = make_kind1(OP_ID, BOB, "BOB's root note");
    reducer.borrow().fire_event_observers_for_test(&root);

    // Step 5: root now present → card surfaces with ALICE's attribution.
    let after = setup.engine.snapshot(&FeedRequest::default());
    assert_eq!(
        after.cards.len(),
        1,
        "BOB's root must surface once it arrives; got {} cards",
        after.cards.len()
    );
    assert_eq!(
        after.cards[0].card.id, OP_ID,
        "the surfaced card must be BOB's root"
    );
    assert_eq!(
        after.cards[0].attribution.len(),
        1,
        "ALICE's reply must attach one attribution to BOB's root; got {}",
        after.cards[0].attribution.len()
    );
    assert_eq!(
        after.cards[0].attribution[0].author_pubkey, ALICE,
        "attribution must carry ALICE's pubkey"
    );
}

/// PR-F1 acceptance test: the `nmp.feed.home` typed projection is PRODUCED.
///
/// After `setup_chirp_web_feeds`, every snapshot frame must carry a
/// `TypedProjectionData` entry keyed `"nmp.feed.home"` with
/// `schema_id = "nmp.nip01.opfeed"`. This is the load-bearing proof that
/// wiring the composition root into the build target causes the browser
/// snapshot to contain the feed projection — not just register the function.
///
/// Runs native (not wasm-pack) to keep the CI gate fast and free of browser
/// infrastructure. The projection closure (`register_typed_snapshot_projection`
/// step 9 in `composition.rs`) is exercised on the same `Kernel` code path
/// regardless of host, so native is an honest proof.
#[test]
fn setup_chirp_web_feeds_projection_appears_in_snapshot() {
    use nmp_core::decode_snapshot_typed_projections;
    use nmp_nip01::op_feed::{OP_FEED_SCHEMA_ID, OP_FEED_SNAPSHOT_KEY};

    let mut runtime = WasmRuntime::new();
    let _setup = setup_chirp_web_feeds(&runtime);

    // `snapshot_bytes_for_test` builds the FlatBuffers update frame the same
    // way the wasm32 relay-pool sink does (via `make_update_frame`), which
    // runs the registered typed-projection closures.
    let frame = runtime.snapshot_bytes_for_test();
    let projections = decode_snapshot_typed_projections(&frame)
        .expect("snapshot frame must decode as valid typed projections");

    let feed_proj = projections
        .iter()
        .find(|p| p.key == OP_FEED_SNAPSHOT_KEY)
        .expect(
            "typed projections must contain an entry keyed \"nmp.feed.home\" \
             after setup_chirp_web_feeds; the projection was not registered",
        );

    assert_eq!(
        feed_proj.schema_id, OP_FEED_SCHEMA_ID,
        "nmp.feed.home projection must carry schema_id \"nmp.nip01.opfeed\""
    );
}

#[test]
fn notify_account_changed_resets_engine_on_switch() {
    // Blocking-3 regression guard: switching accounts clears prior roots.
    //
    // Sequence:
    //   1. ALICE is active → engine populated with ALICE's root
    //   2. Switch to BOB → notify_account_changed detects pubkey change
    //   3. engine is reset → snapshot empty
    //   4. BOB's root arrives → snapshot has BOB's root
    let runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);

    // Step 1: set ALICE, seed follow set, deliver ALICE's root.
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());
    setup.notify_account_changed();

    let note_alice = make_kind1(OP_ID, ALICE, "alice note");
    runtime
        .reducer_handle()
        .borrow()
        .fire_event_observers_for_test(&note_alice);

    let before_switch = setup.engine.snapshot(&FeedRequest::default());
    assert_eq!(
        before_switch.cards.len(),
        1,
        "ALICE's root must be present before the switch"
    );

    // Step 2: switch to BOB — update the slot THEN call notify_account_changed.
    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(BOB.to_string());
    setup.notify_account_changed();

    // Step 3: engine is reset → prior roots cleared.
    let after_switch = setup.engine.snapshot(&FeedRequest::default());
    assert_eq!(
        after_switch.cards.len(),
        0,
        "engine must be empty after account switch; prior account's roots must be cleared;          got {} cards",
        after_switch.cards.len()
    );

    // Step 4: BOB's own root arrives → engine accepts it (BOB is a follow via
    // self-inclusion after the switch).
    let note_bob = make_kind1(OP_ID2, BOB, "bob note");
    runtime
        .reducer_handle()
        .borrow()
        .fire_event_observers_for_test(&note_bob);

    let after_bob_note = setup.engine.snapshot(&FeedRequest::default());
    assert_eq!(
        after_bob_note.cards.len(),
        1,
        "BOB's root must appear after the switch; got {} cards",
        after_bob_note.cards.len()
    );
    assert_eq!(
        after_bob_note.cards[0].card.id, OP_ID2,
        "the card must be BOB's note"
    );
}
