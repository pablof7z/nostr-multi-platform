//! Perspective and suppression integration tests for the Chirp web feed.

use nmp_app_chirp_web::composition::setup_chirp_web_feeds;
use nmp_core::substrate::KernelEvent;
use nmp_feed::FeedRequest;
use nmp_wasm::WasmRuntime;

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const OP_ID2: &str = "0000000000000000000000000000000000000000000000000000000000000003";
const REPLY_ID: &str = "0000000000000000000000000000000000000000000000000000000000000002";

fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

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

fn make_kind3(id: &str, author: &str, follows: &[&str]) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 3,
        created_at: 1_700_000_002,
        tags: follows.iter().map(|follow| p_tag(follow)).collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn make_mute_list(id: &str, author: &str, muted: &[&str]) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 10_000,
        created_at: 1_700_000_003,
        tags: muted.iter().map(|pk| p_tag(pk)).collect(),
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn make_delete(id: &str, author: &str, target_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 5,
        created_at: 1_700_000_004,
        tags: vec![vec!["e".to_string(), target_id.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

#[test]
fn wired_path_mute_replacement_resets_and_suppresses_feed() {
    let mut runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&mut runtime);
    let reducer = runtime.reducer_handle();

    reducer.borrow_mut().set_active_account(ALICE.to_string());
    setup.notify_account_changed();

    reducer.borrow().fire_event_observers_for_test(&make_kind3(
        "3333000000000000000000000000000000000000000000000000000000000004",
        ALICE,
        &[BOB],
    ));
    reducer
        .borrow()
        .fire_event_observers_for_test(&make_kind1(OP_ID, BOB, "visible before mute"));

    assert_eq!(
        setup.engine.snapshot(&FeedRequest::default()).cards.len(),
        1,
        "BOB's note must be visible after ALICE follows BOB"
    );

    reducer
        .borrow()
        .fire_event_observers_for_test(&make_mute_list(
            "1000000000000000000000000000000000000000000000000000000000000004",
            ALICE,
            &[BOB],
        ));
    assert!(
        setup
            .engine
            .snapshot(&FeedRequest::default())
            .cards
            .is_empty(),
        "active-account mute replacement must reset the web OP feed immediately"
    );

    reducer.borrow().fire_event_observers_for_test(&make_kind1(
        OP_ID2,
        BOB,
        "suppressed after mute",
    ));
    assert!(
        setup
            .engine
            .snapshot(&FeedRequest::default())
            .cards
            .is_empty(),
        "future events from a muted author must not be admitted by the web OP-feed observer"
    );
}

#[test]
fn wired_path_kind5_delete_removes_only_author_owned_root() {
    let mut runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&mut runtime);
    let reducer = runtime.reducer_handle();

    reducer.borrow_mut().set_active_account(ALICE.to_string());
    setup.notify_account_changed();
    reducer.borrow().fire_event_observers_for_test(&make_kind3(
        "3333000000000000000000000000000000000000000000000000000000000005",
        ALICE,
        &[BOB],
    ));
    reducer
        .borrow()
        .fire_event_observers_for_test(&make_kind1(OP_ID, BOB, "deletable root"));

    reducer
        .borrow()
        .fire_event_observers_for_test(&make_delete(REPLY_ID, ALICE, OP_ID));
    assert_eq!(
        setup.engine.snapshot(&FeedRequest::default()).cards.len(),
        1,
        "foreign kind:5 must not delete BOB's visible root"
    );

    reducer
        .borrow()
        .fire_event_observers_for_test(&make_delete(OP_ID2, BOB, OP_ID));
    assert!(
        setup
            .engine
            .snapshot(&FeedRequest::default())
            .cards
            .is_empty(),
        "the root author's kind:5 must remove the visible web feed row"
    );
}
