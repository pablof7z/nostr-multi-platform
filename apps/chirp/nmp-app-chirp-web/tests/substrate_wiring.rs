//! Regression tests for the Chirp web substrate wiring.

use nmp_app_chirp_web::composition::setup_chirp_web_feeds;
use nmp_core::{decode_snapshot_typed_projections, ProfileShape, RefLiveness, RefNamespace, RefShape};
use nmp_wasm::WasmRuntime;

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";

fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

fn relay_tag(url: &str) -> Vec<String> {
    vec!["r".to_string(), url.to_string()]
}

fn kind0_content(display_name: &str) -> String {
    serde_json::json!({ "display_name": display_name }).to_string()
}

#[test]
fn kind3_parser_updates_kernel_follow_feed_authors() {
    let runtime = WasmRuntime::new();
    let setup = setup_chirp_web_feeds(&runtime);

    runtime
        .reducer_handle()
        .borrow_mut()
        .set_active_account(ALICE.to_string());
    setup.notify_account_changed();

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

#[test]
fn kind10002_parser_updates_profile_claim_routing_cache() {
    const BOB_RELAY: &str = "wss://bob.relay.example";

    let runtime = WasmRuntime::new();
    let _setup = setup_chirp_web_feeds(&runtime);
    let reducer = runtime.reducer_handle();

    reducer.borrow_mut().project_raw_event_for_test(
        "100020000000000000000000000000000000000000000000000000000000002",
        BOB,
        1_700_000_002,
        10_002,
        vec![relay_tag(BOB_RELAY)],
        "",
    );

    // ADR-0063 Lane H: claim_profile → resolve_ref (KernelReducer 5-arg form:
    // namespace, key, consumer_id, shape, liveness; force=false + hints=[] hardcoded).
    let _ = reducer.borrow_mut().resolve_ref(
        RefNamespace::Profile,
        BOB.to_string(),
        "chirp-web-substrate-test".to_string(),
        RefShape::Profile(ProfileShape::Card),
        RefLiveness::CacheOk.into(),
    );
    let outbound = reducer.borrow_mut().tick();
    let profile_req_relays: Vec<_> = outbound
        .iter()
        .filter(|msg| msg.text().starts_with("[\"REQ\""))
        .filter_map(|msg| {
            let frame: serde_json::Value = serde_json::from_str(msg.text()).ok()?;
            let filter = frame.as_array()?.get(2)?;
            let kinds = filter.get("kinds")?.as_array()?;
            let authors = filter.get("authors")?.as_array()?;
            let is_kind0 = kinds.len() == 1 && kinds[0].as_u64() == Some(0);
            let has_bob = authors.iter().any(|author| author.as_str() == Some(BOB));
            (is_kind0 && has_bob).then(|| msg.relay_url().to_string())
        })
        .collect();

    assert_eq!(
        profile_req_relays,
        vec![BOB_RELAY.to_string()],
        "kind:10002 parser must write the cache read by the web router; outbound={outbound:?}"
    );
}

// ADR-0063 Lane H: `kind0_parser_updates_resolved_profiles_snapshot` deleted.
//
// That test exercised the old `resolved_profiles` JSON snapshot projection
// (KRPR / `decode_resolved_profiles`), which is removed by ADR-0063 Lane H.
// Profile delivery is now via the `refs.profile` KPRF NRRD row-delta sidecar;
// the equivalent assertion lives in the refs integration suite.
