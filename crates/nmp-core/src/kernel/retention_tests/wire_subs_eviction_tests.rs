//! T133 — `wire_subs` row eviction at every terminal point: EOSE for a
//! non-keep (oneshot) sub, a relay-initiated CLOSED frame, per-URL socket
//! teardown (`relay_closed`), and the URL-scoped (not role-wide) semantics
//! that keep a sibling socket's subscriptions alive.
//!
//! V-112 (ADR-0076): `view_close_evicts_wire_subs_to_zero` deleted. That test
//! called `kernel.open_author()` / `kernel.close_author()` (both deleted).
//! T133 view-close eviction is now exercised at the FFI layer via
//! `nmp_app_open_interest` / `nmp_app_close_interest`; the oneshot-EOSE and
//! CLOSED-frame paths below remain as the primary kernel-level T133 pins.

use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_network::role::RelayRole;

/// EOSE for a non-keep sub (oneshot: profile-claim, author-profile,
/// thread-ids, …) evicts the row from `wire_subs`. This is the
/// higher-volume retention source than view-close: every claim and every
/// thread hydration ends via EOSE.
#[test]
fn eose_evicts_wire_sub_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Stage a wire-sub via the same insertion path the production code uses.
    let req = kernel.req_for_relay(
        RelayRole::Indexer,
        "wss://relay.test".to_string(),
        "profile-claim-1-abcd1234",
        "T133 eviction probe",
        serde_json::json!({"kinds":[0],"authors":["aa".repeat(32)],"limit":1}),
    );
    assert_eq!(req.text.split("\"REQ\"").count(), 2, "one REQ emitted");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "REQ inserted exactly one row"
    );

    // Simulate the relay's EOSE — the kernel must (a) emit a CLOSE outbound
    // and (b) evict the row.
    let frame = serde_json::json!(["EOSE", "profile-claim-1-abcd1234"]).to_string();
    let outbound = kernel.handle_text(RelayRole::Indexer, "wss://relay.test", &frame);
    assert!(
        outbound
            .iter()
            .any(|m| m.text.contains("CLOSE") && m.text.contains("profile-claim-1-abcd1234")),
        "EOSE for a oneshot must emit a CLOSE outbound"
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "EOSE for a oneshot must evict the wire_subs row"
    );
}

/// Relay-initiated CLOSED frame evicts the row outright (no outbound
/// CLOSE — the relay already declared the sub dead).
#[test]
fn closed_frame_evicts_wire_sub_row() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    kernel.req_for_relay(
        RelayRole::Content,
        "wss://relay.test".to_string(),
        "author-notes-7-deadbeef",
        "T133 CLOSED-frame eviction probe",
        serde_json::json!({"kinds":[1,6],"authors":["bb".repeat(32)],"limit":100}),
    );
    assert_eq!(kernel.wire_subs_len_for_test(), 1);

    let frame =
        serde_json::json!(["CLOSED", "author-notes-7-deadbeef", "rate-limited"]).to_string();
    let _ = kernel.handle_text(RelayRole::Content, "wss://relay.test", &frame);
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "CLOSED frame must evict the row"
    );
}

/// `relay_closed` (per-URL socket teardown) evicts every row for the
/// closed socket's URL; rows on a different URL are preserved. `relay_failed`
/// (transient → state="retrying") does NOT evict — the sub may resume after
/// the backoff window.
#[test]
fn relay_closed_evicts_per_url_relay_failed_preserves() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Two subs on the Indexer lane, one on Content.
    kernel.req_for_relay(
        RelayRole::Indexer,
        "wss://idx.test".to_string(),
        "profile-claim-1-aaaa",
        "ix probe a",
        serde_json::json!({"kinds":[0]}),
    );
    kernel.req_for_relay(
        RelayRole::Indexer,
        "wss://idx.test".to_string(),
        "profile-claim-2-bbbb",
        "ix probe b",
        serde_json::json!({"kinds":[0]}),
    );
    kernel.req_for_relay(
        RelayRole::Content,
        "wss://content.test".to_string(),
        "author-notes-1-cccc",
        "content probe",
        serde_json::json!({"kinds":[1]}),
    );
    assert_eq!(kernel.wire_subs_len_for_test(), 3);

    // relay_failed must NOT evict — it only marks "retrying".
    kernel.relay_failed(
        RelayRole::Indexer,
        "wss://idx.test",
        "transient error".to_string(),
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        3,
        "relay_failed is transient — rows preserved"
    );

    // relay_closed evicts every row on that URL.
    kernel.relay_closed(RelayRole::Indexer, "wss://idx.test");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "relay_closed evicts the two idx.test rows; Content row preserved"
    );

    // Content socket still healthy.
    kernel.relay_closed(RelayRole::Content, "wss://content.test");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "relay_closed on the content URL evicts the last row"
    );
}

/// T105 regression — under URL-keyed routing several sockets share one
/// `RelayRole` lane. Closing ONE socket must evict only that socket's
/// wire-subs; a sibling socket on the *same role* must keep its subscriptions
/// live. A role-wide `retain` (the pre-fix behaviour) would silently drop the
/// healthy sibling's REQs.
#[test]
fn relay_closed_does_not_evict_sibling_url_on_same_role() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Two Content-lane sockets — different URLs, same RelayRole.
    kernel.req_for_relay(
        RelayRole::Content,
        "wss://content-a.test".to_string(),
        "author-notes-1-aaaa",
        "content A",
        serde_json::json!({"kinds":[1]}),
    );
    kernel.req_for_relay(
        RelayRole::Content,
        "wss://content-b.test".to_string(),
        "author-notes-2-bbbb",
        "content B",
        serde_json::json!({"kinds":[1]}),
    );
    assert_eq!(kernel.wire_subs_len_for_test(), 2);

    // Close ONLY content-a — content-b shares the role but is a live socket.
    kernel.relay_closed(RelayRole::Content, "wss://content-a.test");
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "closing content-a must NOT evict content-b's sub (same role, live socket)"
    );
    let surviving = kernel.snapshot_active_wire_subs();
    assert!(
        surviving.iter().any(|(_id, url)| url.contains("content-b")),
        "the sibling socket's wire-sub must survive; got {surviving:?}"
    );

    // relay_failed is likewise URL-scoped: failing content-b marks only
    // content-b and never evicts (the sub may resume after backoff).
    kernel.relay_failed(
        RelayRole::Content,
        "wss://content-b.test",
        "transient".to_string(),
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        1,
        "relay_failed must not evict — content-b's row stays (now retrying)"
    );

    // The full teardown path still clears the whole lane.
    kernel.relay_closed_all(RelayRole::Content);
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "relay_closed_all evicts every row on the role lane"
    );
}
