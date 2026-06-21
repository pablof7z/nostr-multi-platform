//! Regression tests for the Chirp web substrate wiring.

use nmp_app_chirp_web::composition::setup_chirp_web_feeds;
use nmp_core::typed_projections::decode_resolved_profiles;
use nmp_core::{decode_snapshot_typed_projections, ProfileLiveness};
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

    let _ = reducer.borrow_mut().claim_profile(
        BOB.to_string(),
        "chirp-web-substrate-test".to_string(),
        true,
        false,
        ProfileLiveness::CacheOk,
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

#[test]
fn kind0_parser_updates_resolved_profiles_snapshot() {
    let mut runtime = WasmRuntime::new();
    let _setup = setup_chirp_web_feeds(&runtime);
    let reducer = runtime.reducer_handle();

    let _ = reducer.borrow_mut().claim_profile(
        BOB.to_string(),
        "chirp-web-profile-test".to_string(),
        false,
        false,
        ProfileLiveness::CacheOk,
    );
    reducer.borrow_mut().project_raw_event_for_test(
        "00000000000000000000000000000000000000000000000000000000000000b0",
        BOB,
        1_700_000_010,
        0,
        vec![],
        &kind0_content("Bob Web"),
    );

    let frame = runtime.snapshot_bytes_for_test();
    let projections = decode_snapshot_typed_projections(&frame)
        .expect("snapshot frame must decode as typed projections");
    let resolved = projections
        .iter()
        .find(|projection| projection.key == "resolved_profiles")
        .expect("snapshot must include resolved_profiles");
    let decoded =
        decode_resolved_profiles(&resolved.payload).expect("resolved_profiles payload must decode");
    let (_, profile) = decoded
        .entries
        .iter()
        .find(|(pubkey, _)| pubkey == BOB)
        .expect("resolved_profiles must include the claimed BOB profile");

    assert_eq!(
        profile.display_name.as_deref(),
        Some("Bob Web"),
        "kind:0 parser must write the cache read by resolved_profiles"
    );
}
