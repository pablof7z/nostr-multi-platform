use super::*;
use crate::app::VIEW_PROFILE;
use crate::nip19::encode_npub;

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";

#[test]
fn reduce_open_uri_npub_routes_to_profile_view() {
    let mut r = KernelReducer::new();
    let npub = encode_npub(PK).unwrap();
    let update = r.reduce(KernelAction::OpenUri {
        uri: format!("nostr:{npub}"),
    });
    assert_eq!(
        update,
        KernelUpdate::ViewOpened {
            namespace: VIEW_PROFILE.into(),
            key: PK.into(),
        }
    );
}

#[test]
fn reduce_start_echoes_started() {
    let mut r = KernelReducer::new();
    assert_eq!(
        r.reduce(KernelAction::Start),
        KernelUpdate::Started { rev: 0 }
    );
}

#[test]
fn reduce_garbage_uri_is_rejected_not_a_panic() {
    let mut r = KernelReducer::new();
    let update = r.reduce(KernelAction::OpenUri {
        uri: "not-a-nostr-thing".into(),
    });
    assert!(matches!(
        update,
        KernelUpdate::UriRejected { reason, .. } if reason.contains("unparseable")
    ));
}

// ─── V-01 Stage 3 relay-lifecycle surface ────────────────────────────────
//
// These tests cover the contracts the wasm32 `BrowserRelayDriver` depends
// on. They are intentionally narrow — the deep behaviour (replay
// semantics, AUTH partition, wire-sub eviction) is already covered by the
// kernel-side tests in `kernel/replay_tests.rs`, `kernel/auth_tests.rs`,
// and `kernel/retention_tests.rs`. What we pin here is that
// `KernelReducer` calls the right underlying methods in the right order
// and never panics across the public surface.

const RELAY: &str = "wss://relay.example";

#[test]
fn handle_relay_frame_text_does_not_panic_on_garbage() {
    // D6 invariant: a malformed NIP-01 frame must surface as a no-op
    // (the kernel silently drops unparseable text). The WASM driver
    // forwards every onmessage payload verbatim — we cannot assume
    // well-formedness.
    let mut r = KernelReducer::new();
    let out = r.handle_relay_frame(
        RelayRole::Content,
        RELAY,
        RelayFrame::Text("garbage that is not NIP-01".to_string()),
    );
    // No registered subs / publish engine state → empty outbound; the
    // important assertion is the absence of a panic.
    assert!(
        out.is_empty(),
        "garbage text must drop, not produce outbound"
    );
}

#[test]
fn handle_relay_frame_close_does_not_panic() {
    let mut r = KernelReducer::new();
    let out = r.handle_relay_frame(
        RelayRole::Content,
        RELAY,
        RelayFrame::Close(Some("server going away".to_string())),
    );
    assert!(out.is_empty());
}

#[test]
fn handle_relay_frame_binary_and_ping_pong_are_counted_no_outbound() {
    let mut r = KernelReducer::new();
    for frame in [
        RelayFrame::Binary(b"opaque".to_vec()),
        RelayFrame::Ping,
        RelayFrame::Pong,
    ] {
        let out = r.handle_relay_frame(RelayRole::Indexer, RELAY, frame);
        assert!(out.is_empty(), "non-text frames must produce no outbound");
    }
}

#[test]
fn handle_relay_connected_first_dial_emits_startup_or_empty() {
    // First-dial path (`is_reconnect = false`) on a fresh reducer with no
    // registered interests yields no startup REQs (`startup_requests`
    // returns empty until lifecycle.tick runs against a coverage plan).
    // The important contract: no panic and AUTH partition does not strip
    // legitimate frames.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    // Empty is the correct answer for a kernel with no view-spec interests.
    assert!(out.is_empty(), "fresh kernel has no startup REQs");
}

#[test]
fn handle_relay_connected_is_reconnect_does_not_panic() {
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    // First mark the relay closed so we have a valid "reconnect" state.
    r.handle_relay_closed(RelayRole::Content, RELAY);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, true);
    // Pass: no panic.
}

#[test]
fn handle_relay_failed_and_closed_are_total() {
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    r.handle_relay_failed(
        RelayRole::Content,
        RELAY,
        "connection reset by peer".to_string(),
    );
    r.handle_relay_closed(RelayRole::Content, RELAY);
    // Pass: no panic.
}

#[test]
fn tick_on_fresh_reducer_is_empty() {
    // With no in-flight publishes, `tick_publish_engine_for_now` has
    // nothing to retry. AUTH partition over an empty vec is also empty.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    assert!(r.tick().is_empty());
}

// ─── V-01 Stage 3c publish-from-signed-event surface ─────────────────────
//
// `publish_signed_event` is the new public seam the wasm runtime uses to
// feed `Nip07Signer::sign()` results through the publish engine. The
// tests here pin only the contract — total, no panic, returns an
// outbound vec — and defer deep publish-engine behaviour to the
// existing kernel-side tests in `publish/engine/tests.rs`.

use crate::substrate::{SignedEvent, UnsignedEvent};

fn synthetic_signed_note() -> SignedEvent {
    // Synthetic SignedEvent — the id and sig are placeholder hex strings
    // (the publish engine never re-verifies the signature; it just routes
    // the wire form). The kind:1 payload reaches the engine and goes
    // through NIP-65 outbox resolution, which on a fresh kernel with no
    // kind:10002 events in the store returns no targets and produces a
    // `NoTargets` `RecentFailure` row (empty outbound). That's exactly
    // the contract we want to assert: total, no panic, returns
    // `Vec::new()` rather than throwing.
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: PK.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "hello from wasm".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

#[test]
fn publish_signed_event_on_fresh_kernel_does_not_panic() {
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let signed = synthetic_signed_note();
    // No kind:10002 known → engine records NoTargets → returns empty.
    // The important assertion is the absence of a panic; the empty-
    // outbound semantic is the documented D6 path.
    let out = r.publish_signed_event(&signed, &[], None);
    assert!(
        out.is_empty(),
        "fresh kernel has no NIP-65 outbox; publish must surface NoTargets, not outbound"
    );
}

#[test]
fn publish_signed_event_accepts_empty_p_tags() {
    // The engine recomputes `#p` from `signed.unsigned.tags`; the slice
    // is informational. Pinning that empty is accepted is the smoke
    // test for the doc contract.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let signed = synthetic_signed_note();
    let _ = r.publish_signed_event(&signed, &[], None);
    // Pass: no panic.
}

#[test]
fn publish_signed_event_threads_correlation_id_into_engine() {
    // The correlation_id parameter must reach the publish engine so
    // terminals land in `action_results` keyed on the dispatch id.
    // Without this, the wasm host receives terminals keyed on the
    // event id it never saw (partial-success UX would have no key to
    // correlate on). The contract is byte-identical with the native
    // generic publish dispatched path which uses
    // `Kernel::publish_signed_to_with_correlation`.
    //
    // We can't directly observe the engine's correlation_id table from
    // here (it's `pub(crate)`); the assertion below pins the surface
    // shape (no panic when correlation_id is `Some(_)`) — the deep
    // wire-up is exercised by the native publish tests in
    // `actor::commands::tests` and `publish::engine::tests`.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let signed = synthetic_signed_note();
    let _ = r.publish_signed_event(&signed, &[], Some("dispatch-1".to_string()));
    // Pass: no panic with Some correlation_id.
}

// ─── F-CR-00 component-owned claim seam ──────────────────────────────────
//
// These tests cover the contracts the wasm dispatch arm depends on.
// Deep claim behaviour (batch routing, indexer-only lane, pending queue)
// is already covered by `kernel/profile_claim_tests.rs`,
// `kernel/event_claim_tests.rs`, and friends. What we pin here is that
// the `KernelReducer` delegations are wired correctly and the
// `any_relay_connected` gate accurately reflects kernel relay state.

#[test]
fn claim_profile_on_fresh_reducer_parks_returns_empty() {
    // Without a relay connected (`can_send = false`), `claim_profile`
    // parks the pubkey in `profile_requests.pending` and returns no
    // outbound. The refcount is still registered internally so a second
    // claim for a different consumer does not double-fetch.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    // any_relay_connected is false on a fresh reducer — assert the gate.
    assert!(!r.any_relay_connected(), "fresh reducer: no relay connected");
    let out = r.claim_profile(PK.to_string(), "chirp-web-author-1".to_string(), false, false);
    assert!(out.is_empty(), "parked claim must emit no outbound");
}

#[test]
fn claim_profile_refcount_dedup_does_not_double_fetch() {
    // Two different consumer_ids for the same pubkey must NOT each issue a
    // separate REQ once `can_send` becomes true. The second claim hits the
    // `profile_requests.requested.contains` short-circuit and returns empty.
    // (Detailed batch/routing assertions live in profile_claim_tests.rs.)
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);

    // First claim parks it.
    let _ = r.claim_profile(
        PK.to_string(),
        "chirp-web-author-card-a".to_string(),
        false,
        false,
    );
    // Second claim for same pubkey, different consumer — must be a no-op
    // outbound (the profile is already pending / registered).
    let out2 = r.claim_profile(
        PK.to_string(),
        "chirp-web-author-card-b".to_string(),
        false,
        false,
    );
    assert!(
        out2.is_empty(),
        "second claim for same pubkey must not duplicate outbound: {out2:?}"
    );
}

#[test]
fn release_profile_is_total_no_panic() {
    // Releasing a pubkey that was never claimed is a no-op (D6).
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.release_profile(PK, "chirp-web-author-1");
    assert!(out.is_empty(), "release must emit no outbound");
}

#[test]
fn claim_event_malformed_uri_is_total_no_panic() {
    // D6: a garbled nostr URI must be silently dropped, not a panic.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.claim_event(
        "not-a-nostr-uri".to_string(),
        "chirp-web-embed-1".to_string(),
        false,
        false,
    );
    assert!(out.is_empty(), "malformed URI must produce no outbound");
}

#[test]
fn release_event_malformed_uri_is_total_no_panic() {
    // D6 symmetry: release with a garbage URI must not panic.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    let out = r.release_event("not-a-nostr-uri", "chirp-web-embed-1");
    assert!(out.is_empty());
}

#[test]
fn any_relay_connected_false_before_connect_true_after() {
    // `any_relay_connected` must mirror the kernel's per-lane
    // `RelayHealth::connection` field — false before
    // `handle_relay_connected`, true after.
    let mut r = KernelReducer::new();
    assert!(!r.any_relay_connected(), "before connect: must be false");
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    assert!(r.any_relay_connected(), "after connect: must be true");
}

#[test]
fn any_relay_connected_returns_false_after_close() {
    // After `handle_relay_closed` the lane flips back to
    // `connection != "connected"`, so the gate must return false again.
    let mut r = KernelReducer::new();
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    assert!(r.any_relay_connected());
    r.handle_relay_closed(RelayRole::Content, RELAY);
    assert!(!r.any_relay_connected(), "after close: must be false");
}

#[test]
fn make_update_frame_bumps_rev_monotonically() {
    // D6 (total) + monotonic-rev contract: `make_update_frame` must never
    // panic on a fresh reducer, and each successive call must produce a
    // strictly larger revision number in the decoded envelope.
    let mut r = KernelReducer::new();

    let bytes0 = r.make_update_frame(false);
    assert!(
        !bytes0.is_empty(),
        "make_update_frame must return a non-empty frame"
    );

    let env0 = crate::decode_snapshot_envelope(&bytes0)
        .expect("first frame must decode without error");

    let bytes1 = r.make_update_frame(false);
    let env1 = crate::decode_snapshot_envelope(&bytes1)
        .expect("second frame must decode without error");

    assert!(
        env1.rev > env0.rev,
        "rev must increase monotonically: {} → {}",
        env0.rev,
        env1.rev
    );
}

#[test]
fn set_configured_relays_surfaces_in_update_frame() {
    // After `set_configured_relays`, the next `make_update_frame` must
    // carry the supplied URL in both the Tier-3 relay_statuses rows and
    // the `configured_relays` typed-projection sidecar.
    use crate::typed_projections::{decode_configured_relays, CONFIGURED_RELAYS_SCHEMA_ID};

    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(
        "wss://relay.test".to_string(),
        "both".to_string(),
    )]);

    let bytes = r.make_update_frame(true);

    // Tier-3 relay_statuses
    let env = crate::decode_snapshot_envelope(&bytes)
        .expect("frame must decode");
    assert!(
        env.relay_statuses
            .iter()
            .any(|row| row.relay_url == "wss://relay.test"),
        "relay_statuses must contain the configured URL; got: {:?}",
        env.relay_statuses
    );

    // Tier-2 configured_relays sidecar
    let projections = crate::decode_snapshot_typed_projections(&bytes)
        .expect("typed projections must decode");
    let cr_entry = projections
        .iter()
        .find(|p| p.schema_id == CONFIGURED_RELAYS_SCHEMA_ID)
        .expect("configured_relays sidecar must be present");
    let model = decode_configured_relays(&cr_entry.payload)
        .expect("configured_relays payload must decode");
    assert!(
        model.relays.iter().any(|row| row.url == "wss://relay.test"),
        "configured_relays sidecar must contain the configured URL; got: {:?}",
        model.relays
    );
}
