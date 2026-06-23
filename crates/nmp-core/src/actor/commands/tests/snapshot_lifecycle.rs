//! Tests for snapshot JSON shape and follows-feed lifecycle
//! (`declare_active_follows_feed`, `clear_active_follows_feed`).

use super::*;

#[test]
fn snapshot_json_carries_new_projections() {
    let (mut id, mut kernel) = fresh();
    sign_in_with_nip65(&mut id, &mut kernel);
    let unsigned = crate::substrate::UnsignedEvent {
        pubkey: String::new(),
        kind: 1,
        tags: Vec::new(),
        content: "json shape check".to_string(),
        created_at: 0,
    };
    publish_unsigned_event(
        &id,
        &mut kernel,
        unsigned,
        None,
        None,
        &mut crate::actor::pending_sign::ParkedSignerOps::new(),
    );
    add_relay(&mut kernel, "wss://relay.damus.io", "both");
    let json = kernel.make_update_json_for_test(true);
    assert!(json.contains("\"accounts\""));
    assert!(json.contains("\"active_account\""));
    assert!(json.contains("\"last_error_toast\""));
    // D0: the publish cluster (`publish_queue`, `publish_outbox`,
    // `configured_relays`) is no longer a set of typed `KernelSnapshot` fields —
    // all three are kernel-owned built-in entries in the host-extensible
    // `projections` map. They are always present (kernel-owned data, no host
    // registration step), unlike the host-registered `"bunker_handshake"`
    // projection. Decode the map and assert the keys nest under it.
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("snapshot must be valid JSON");
    let projections = parsed
        .get("projections")
        .expect("snapshot must carry the projections map once the publish cluster is populated");
    assert!(projections.get("publish_queue").is_some());
    assert!(projections.get("publish_outbox").is_some());
    assert!(projections.get("outbox_summary").is_some());
    assert!(projections.get("configured_relays").is_some());
    let role_options = projections["relay_role_options"]
        .as_array()
        .expect("relay_role_options must be a projection array");
    assert_eq!(role_options[0]["value"].as_str(), Some("both,indexer"));
    // `label` removed from the wire (#1678, D7) — shells map value→label.
    assert!(
        role_options[0].get("label").is_none(),
        "label must not appear on the wire"
    );
    assert_eq!(role_options[0]["tint"].as_str(), Some("accent"));
    assert_eq!(role_options[1]["value"].as_str(), Some("both"));
    assert_eq!(role_options[1]["is_default"].as_bool(), Some(true));
    let relay_rows = projections["configured_relays"]
        .as_array()
        .expect("configured_relays must be a projection array");
    assert!(
        !relay_rows.is_empty(),
        "configured_relays projection must have entries"
    );
    // D0: the views cluster (`profile`) is a kernel-owned built-in entry in
    // the `projections` map. `profile` is always present.
    // V-112 (ADR-0042): `author_view` / `thread_view` deleted from projections.
    // #1610: `timeline`, `inserted`, `updated`, `removed` removed from the
    // codegen registry (JSON-era vestigials; typed feed ships via `nmp.feed.home`).
    // These asserts confirm the kernel never emits those legacy keys.
    assert!(projections.get("profile").is_some());
    // Kernel never emits the JSON-era timeline/delta keys (#1610).
    assert!(
        projections.get("timeline").is_none(),
        "#1610: timeline must never appear in projections (removed JSON-era key)"
    );
    assert!(
        projections.get("inserted").is_none(),
        "#1610: inserted must never appear in projections (removed JSON-era key)"
    );
    assert!(
        projections.get("updated").is_none(),
        "#1610: updated must never appear in projections (removed JSON-era key)"
    );
    assert!(
        projections.get("removed").is_none(),
        "#1610: removed must never appear in projections (removed JSON-era key)"
    );
    // V-112 (ADR-0042): `author_view` / `thread_view` deleted from snapshot.
    // `retarget_timeline` no longer calls `kernel.open_author()`.
    assert!(
        projections.get("author_view").is_none(),
        "V-112: author_view must be absent — deleted in ADR-0042 M2 migration"
    );
    assert!(
        projections.get("thread_view").is_none(),
        "V-112: thread_view must be absent — deleted in ADR-0042 M2 migration"
    );
    // The typed `KernelSnapshot` fields must be gone — a shell that still
    // reads them would silently get `null`.
    assert!(parsed.get("profile").is_none());
    assert!(parsed.get("items").is_none());
    assert!(parsed.get("author_view").is_none());
    assert!(parsed.get("thread_view").is_none());
    // D0: NIP-46 bunker handshake is no longer a typed `KernelSnapshot` field
    // — it is surfaced through the built-in `"bunker_handshake"` snapshot
    // projection registered in `nmp_app_new`. A bare `make_update` (no
    // projection registered) therefore does NOT carry the key; the projection
    // path is covered by `snapshot_carries_bunker_handshake_value` in
    // `remote_signer_tests.rs`.
}

// ─── T140 — open_timeline must register M2 interests, not open_author ────────

/// T140 RED test: the `open_timeline()` actor command must register M2
/// `LogicalInterest`s in the lifecycle registry (for the active account's
/// follow set) so that `drain_lifecycle_tick()` emits follow-feed REQ frames.
///
/// Pre-T140: `declare_active_follows_feed` → `open_author` → no follow-feed interests in
/// registry → `drain_lifecycle_tick` returns `Vec::new()`. FAILS.
///
/// Post-T140: `declare_active_follows_feed` pushes per-follow `LogicalInterest`s → the
/// M2 planner compiles them → `drain_lifecycle_tick` returns REQ frame(s) for
/// the followed author's NIP-65 write relay. PASSES.
#[test]
fn t140_declare_active_follows_feed_registers_m2_interests_drain_emits_req() {
    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let (mut id, mut kernel) = fresh();

    // Sign in so `open_timeline` has an active pubkey.
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    // ALICE has a resolved write relay (via kind:10002 test support helper).
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-t140.relay/"]);

    // Inject kind:3 for the active account listing ALICE as a follow.
    // This populates the capability-owned contacts cache via the registered
    // kind:3 parser, and the active-account contacts-transition signal drives
    // the kernel-owned follow-feed effects (ADR-0057 PR 3).
    let follow_tags = vec![vec!["p".to_string(), ALICE.to_string()]];
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        &active_pk,
        2_000,
        3,
        follow_tags,
        "wss://seed.relay/",
        2_000_000,
    );

    // Force the lifecycle selection budget so the compiler routes freely.
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // Call the actor command under test. `declare_active_follows_feed` receives the
    // compiled acquisition kinds {1, 6}, which re-register the active account's
    // M2 follow-feed interests.
    let _outbound = declare_active_follows_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );

    // Drain the M2 planner — must emit follow-feed REQs after T140.
    let frames = kernel.drain_lifecycle_tick();
    let req_urls: Vec<String> = frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();

    assert!(
        !req_urls.is_empty(),
        "T140: declare_active_follows_feed must register follow-feed M2 interests so \
         drain_lifecycle_tick emits REQ frames (got {} total frames, 0 REQs)",
        frames.len(),
    );
    assert!(
        req_urls.iter().any(|u| u == "wss://alice-t140.relay/"),
        "T140: declare_active_follows_feed REQ must target ALICE's resolved write relay \
         wss://alice-t140.relay/; got urls: {req_urls:?}"
    );
}

// ── declare_active_follows_feed / clear_active_follows_feed (RED tests — Step 1 of TDD) ──────

/// After `declare_active_follows_feed({1,6})` compiled acquisition kinds are supplied,
/// the follow-feed interests are registered;
/// after `clear_active_follows_feed()` they are withdrawn, a CLOSE frame is emitted,
/// `follow_feed_interest_ids` is empty, and `timeline_authors` is empty.
///
/// Verifies the full symmetric lifecycle required by the design: D5 cluster is
/// present after open, absent after close.
#[test]
fn clear_active_follows_feed_withdraws_follow_interests_and_emits_close() {
    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let (mut id, mut kernel) = fresh();

    // Sign in so `declare_active_follows_feed` has an active pubkey.
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    // ALICE has a resolved write relay.
    kernel.seed_kind10002_for_test(ALICE, &["wss://alice-close-test.relay/"]);

    // Inject kind:3 for the active account listing ALICE as a follow.
    let follow_tags = vec![vec!["p".to_string(), ALICE.to_string()]];
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        &active_pk,
        2_000,
        3,
        follow_tags,
        "wss://seed.relay/",
        2_000_000,
    );

    // Force the lifecycle selection budget so the compiler routes freely.
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // Open with compiled acquisition kinds {1, 6}: interests should be registered.
    let _outbound = declare_active_follows_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );

    // Drain — must emit REQ frames for ALICE after open.
    let open_frames = kernel.drain_lifecycle_tick();
    let req_count_after_open = open_frames
        .iter()
        .filter(|f| matches!(f, crate::subs::WireFrame::Req { .. }))
        .count();
    assert!(
        req_count_after_open > 0,
        "declare_active_follows_feed must register follow-feed interests (got 0 REQs after open)"
    );

    // Close: interests should be withdrawn, CLOSE frames emitted.
    let _close_out = clear_active_follows_feed(&id, &mut kernel);

    let close_frames = kernel.drain_lifecycle_tick();
    let close_count = close_frames
        .iter()
        .filter(|f| matches!(f, crate::subs::WireFrame::Close { .. }))
        .count();
    assert!(
        close_count > 0,
        "clear_active_follows_feed must emit CLOSE frames (got 0 CLOSEs after close)"
    );

    // After close the follow-feed interest registry must be empty.
    assert!(
        kernel.follow_feed_interest_ids.is_empty(),
        "clear_active_follows_feed must clear follow_feed_interest_ids"
    );

    // timeline_authors must be cleared as well (the kernel CLEAR branch).
    assert!(
        kernel.timeline_authors.is_empty(),
        "clear_active_follows_feed must clear timeline_authors"
    );

    // Confirm that the kernel never emits the JSON-era timeline/delta keys
    // (#1610 removed them from the registry; they were never produced post-V-112).
    let post_close_json = kernel.make_update_json_for_test(true);
    let post_close: serde_json::Value =
        serde_json::from_str(&post_close_json).expect("post-close snapshot must be valid JSON");
    let post_projections = post_close
        .get("projections")
        .expect("snapshot must carry the projections map");
    assert!(
        post_projections.get("timeline").is_none(),
        "#1610: timeline must never appear in projections (removed JSON-era key)"
    );
    assert!(
        post_projections.get("inserted").is_none(),
        "#1610: inserted must never appear in projections (removed JSON-era key)"
    );
    assert!(
        post_projections.get("updated").is_none(),
        "#1610: updated must never appear in projections (removed JSON-era key)"
    );
    assert!(
        post_projections.get("removed").is_none(),
        "#1610: removed must never appear in projections (removed JSON-era key)"
    );
}

/// `declare_active_follows_feed` with an empty kinds set acts as a clear (same as close):
/// any previously registered follow-feed interests are withdrawn.
#[test]
fn declare_active_follows_feed_empty_kinds_is_clear() {
    const BOB: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let (mut id, mut kernel) = fresh();
    sign_in_nsec(&mut id, &mut kernel, TEST_NSEC, false);
    let active_pk = id.active_pubkey().expect("active account after sign_in");

    kernel.seed_kind10002_for_test(BOB, &["wss://bob-empty-test.relay/"]);

    let follow_tags = vec![vec!["p".to_string(), BOB.to_string()]];
    kernel.inject_replaceable_event(
        "0000000000000000000000000000000000000000000000000000000000000002",
        &active_pk,
        2_000,
        3,
        follow_tags,
        "wss://seed.relay/",
        2_000_000,
    );

    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);

    // First open with non-empty kinds so there is something to clear.
    let _ = declare_active_follows_feed(
        &id,
        &mut kernel,
        std::collections::BTreeSet::from([1u32, 6u32]),
    );
    let _ = kernel.drain_lifecycle_tick();

    // Now open with empty kinds set — behaves as clear.
    let _ = declare_active_follows_feed(&id, &mut kernel, std::collections::BTreeSet::new());
    let _ = kernel.drain_lifecycle_tick();

    assert!(
        kernel.follow_feed_interest_ids.is_empty(),
        "declare_active_follows_feed with empty kinds must clear follow_feed_interest_ids"
    );
}
