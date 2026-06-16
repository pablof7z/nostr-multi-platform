//! Regression tests for the Chirp web substrate wiring.

use nmp_app_chirp_web::composition::setup_chirp_web_feeds;
use nmp_wasm::{
    protocol::{ActionDispatch, WorkerRequest},
    WasmRuntime,
};

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";

fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

#[test]
fn kind3_parser_updates_kernel_follow_feed_authors() {
    let mut runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);

    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());
    setup.notify_account_changed();

    runtime
        .handle(WorkerRequest::Dispatch(ActionDispatch {
            action_type: "nmp.kernel.open_contact_feed".to_string(),
            payload: serde_json::json!({ "kinds": [1, 6] }),
            correlation_id: "open-contact-feed".to_string(),
        }))
        .expect("open contact feed dispatch must succeed");

    runtime
        .reducer_handle()
        .borrow_mut()
        .project_raw_event_for_test(
            "3333000000000000000000000000000000000000000000000000000000000003",
            ALICE,
            1_700_000_003,
            3,
            vec![p_tag(BOB)],
            "",
        );

    let follows = setup.follow_set.follows();
    assert!(
        follows.contains(&BOB.to_string()),
        "kind:3 parser must update the web follow predicate cache; got {follows:?}"
    );

    let authors = runtime.reducer_handle().borrow().active_timeline_authors();
    assert!(
        authors.contains(&ALICE.to_string()),
        "follow-feed authors must include the active account; got {authors:?}"
    );
    assert!(
        authors.contains(&BOB.to_string()),
        "follow-feed authors must include kind:3 follows; got {authors:?}"
    );
}
