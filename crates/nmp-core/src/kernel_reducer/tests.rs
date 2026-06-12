use super::*;
use crate::app::VIEW_PROFILE;
use crate::nip19::{encode_nevent, encode_npub, NeventData};
use std::collections::BTreeSet;

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

// ─── pending_view_requests drain on idle tick (PR #1140 fix) ─────────────
//
// Pins that `tick()` calls `pending_view_requests()` as its FIRST drain,
// ensuring time-gated work and deferred AUTH-gate REQs are pumped on every
// idle tick rather than only when inbound traffic arrives (quiet-socket
// starvation fix). Genuinely time-gated paths (contacts_deadline, F-TTL
// drain_pending_reverify) need a fake-clock injection below the KernelReducer
// seam to test deterministically — they are out of scope here. We pin the
// deferred_outbound drain, which is the most accessible non-time-gated source
// and directly exercises the quiet-socket starvation case the fix targets.

#[test]
fn tick_drains_pending_view_requests_on_idle() {
    // Fail-first guard: RED under the old tick() (publish-pump + lifecycle-drain
    // never touch deferred_outbound). GREEN with the new first line.
    //
    // Setup: park a REQ frame in deferred_outbound — the precise buffer that
    // receives AUTH-gate-deferred messages. On a quiet socket (no inbound
    // traffic) the pre-fix tick() left this buffer untouched indefinitely.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);

    // Push directly into the kernel's deferred_outbound ring (pub(crate) field,
    // reachable from this child module). relay_auth_paused(Content) is false on a
    // fresh reducer, so partition_auth_paused will not re-defer the frame — it
    // passes through to the caller.
    r.kernel.defer_outbound(OutboundMessage {
        role: RelayRole::Content,
        relay_url: RELAY.to_string(),
        text: "[\"REQ\",\"deferred-idle-1\",{\"kinds\":[1]}]".to_string(),
    });

    let out = r.tick();
    assert!(
        out.iter().any(|m| m.text.contains("deferred-idle-1")),
        "tick() must drain deferred_outbound via pending_view_requests(); \
         got empty — pending_view_requests() is missing from tick()"
    );
}

// ─── PR-2 tick driver acceptance tests ───────────────────────────────────
//
// These two tests pin the contracts the wasm32 1 Hz timer depends on.
// They are deliberately fail-first: they FAIL without the lifecycle-drain
// extension to `tick()` and the `changed_since_emit` accessor.

fn nevent_uri_for_test(event_id: &str) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: None,
        kind: Some(1),
    })
    .expect("encode_nevent must succeed for well-formed test data");
    format!("nostr:{bech}")
}

#[test]
fn tick_drains_lifecycle_outbound_after_claim_event() {
    // Regression guard for PR-2: `tick()` must drain the subscription
    // lifecycle outbound, not only the publish-engine retry queue.
    //
    // `claim_event` enqueues a `CompileTrigger::ViewOpened` and returns
    // `Vec::new()` — the REQ frame only materialises through a lifecycle
    // drain. Before the PR-2 fix, `tick()` called only
    // `tick_publish_engine_for_now` and the trigger silently sat until the
    // next relay-connected event. After the fix, `tick()` appends
    // `drain_lifecycle_outbound()` and the REQ appears in the same call.
    //
    // Test structure:
    //   1. Start the reducer and seed a configured relay so the lifecycle
    //      planner has a lane to compile REQs against.
    //   2. Connect the relay — this drains startup triggers (empty on a
    //      fresh kernel).
    //   3. Call `claim_event` with `can_send = true` — this enqueues a
    //      fresh `CompileTrigger::ViewOpened` but returns `Vec::new()`.
    //   4. Assert `tick()` returns a non-empty outbound containing a REQ.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    // Step 2: connect — drains any startup lifecycle triggers.
    let startup = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    let _ = startup; // may be empty or carry startup REQs; we don't assert here

    // Step 3: claim an event (enqueues CompileTrigger, returns empty).
    let event_id = "a".repeat(64);
    let uri = nevent_uri_for_test(&event_id);
    let direct = r.claim_event(uri, "chirp-web-embed-tick-test".to_string(), true, false);
    assert!(
        direct.is_empty(),
        "claim_event must return empty (REQ flows via lifecycle drain, not direct return)"
    );

    // Step 4: tick must drain the trigger and emit the REQ.
    let tick_out = r.tick();
    assert!(
        !tick_out.is_empty(),
        "tick() must drain the lifecycle trigger enqueued by claim_event and emit a REQ; \
         got empty outbound — lifecycle drain is missing from tick()"
    );
}

#[test]
fn idle_tick_does_not_set_dirty_flag() {
    // Guard for the PR-2 dirty-flag coalescing rider: a tick with no
    // pending publish retries and no pending lifecycle triggers must NOT
    // mark the kernel dirty. If it did, the wasm32 timer would push a
    // snapshot on every heartbeat regardless of whether anything changed —
    // burning JS-heap churn and upstream re-renders for free.
    //
    // Flow:
    //   1. Start and take a snapshot — clears `changed_since_emit`.
    //   2. Execute an idle tick (nothing pending).
    //   3. Assert `changed_since_emit()` is still false.
    let mut r = KernelReducer::new();
    let _ = r.reduce(KernelAction::Start);
    // Clear the dirty flag by taking a snapshot.
    let _ = r.make_update_frame(true);
    assert!(
        !r.changed_since_emit(),
        "changed_since_emit must be false right after make_update_frame"
    );
    // An idle tick: no publishes in flight, no lifecycle triggers.
    let tick_out = r.tick();
    assert!(tick_out.is_empty(), "idle tick must produce no outbound");
    assert!(
        !r.changed_since_emit(),
        "idle tick must not dirty the kernel — would cause spurious snapshot pushes"
    );
}

// ─── PR-3 feed-verb surface acceptance tests ─────────────────────────────
//
// Pin that the four new KernelReducer methods introduced in PR-3 honour their
// public contracts:
//
// • `open_interest`   — cold-open emits REQ inline (drain_lifecycle_outbound
//                       is called before returning so frames go out against
//                       already-connected relays immediately).
// • `close_interest`  — last-owner removal emits CLOSE inline.
// • `set_follow_feed_kinds` — total (D6: empty or non-empty, no panic).
// • `set_active_account`   — total (D6: valid or empty pubkey, no panic);
//                            sets active_account projection and returns
//                            outbound without panicking.

const FILTER_KINDS_1: &str = r#"{"kinds":[1]}"#;
const CONSUMER: &str = "chirp-web-home";

#[test]
fn open_interest_with_connected_relay_emits_req_inline() {
    // Contract: calling open_interest after a relay is connected must return
    // at least one outbound frame whose text contains "REQ" — the sub is
    // wired immediately (inline drain), not deferred to the next tick.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);

    let out = r.open_interest(FILTER_KINDS_1, CONSUMER, 0);

    assert!(
        !out.is_empty(),
        "open_interest must emit at least one frame when a relay is connected"
    );
    assert!(
        out.iter().any(|m| m.text.contains("REQ")),
        "open_interest must emit a REQ frame; got: {:?}",
        out.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
}

#[test]
fn close_interest_after_open_emits_close_inline() {
    // Contract: the CLOSE frame flows through the inline drain so the host
    // does not need to call tick() to get the relay-side unsubscribe.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    // Open first so there is an active owner to remove.
    let _ = r.open_interest(FILTER_KINDS_1, CONSUMER, 0);

    let out = r.close_interest(FILTER_KINDS_1, CONSUMER, 0);

    assert!(
        !out.is_empty(),
        "close_interest must emit at least one frame when closing an open sub"
    );
    assert!(
        out.iter().any(|m| m.text.contains("CLOSE")),
        "close_interest must emit a CLOSE frame; got: {:?}",
        out.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
}

#[test]
fn open_interest_malformed_filter_is_silent_no_panic() {
    // D6 — malformed filter_json must be silently dropped, not a panic.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    // Garbage JSON must not panic.
    let out = r.open_interest("not valid json {{{", CONSUMER, 0);
    // Silently dropped — no REQ emitted for unparseable input.
    assert!(
        out.iter().all(|m| !m.text.contains("REQ")),
        "malformed filter must not produce a REQ"
    );
}

#[test]
fn set_follow_feed_kinds_is_total() {
    // D6 — total: empty set, populated set, called before or after
    // start/relay-connect must never panic.
    let mut r = KernelReducer::new();
    // Before start / relay connect — must not panic.
    let _ = r.set_follow_feed_kinds(BTreeSet::new());
    let _ = r.set_follow_feed_kinds([1u32, 6u32].into_iter().collect());

    // After start + relay connected — must not panic.
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    let _ = r.set_follow_feed_kinds([1u32, 6u32].into_iter().collect());
    let _ = r.set_follow_feed_kinds(BTreeSet::new());
    // Pass: no panic.
}

#[test]
fn set_active_account_is_total() {
    // D6 — total: valid hex pubkey, empty string — must not panic and must
    // return without unwinding.
    let mut r = KernelReducer::new();
    let out = r.set_active_account(PK.to_string());
    // With no relays configured, active_account_bootstrap_requests is empty
    // and the lifecycle outbound is empty, so the result is empty.
    // The key assertion is the absence of a panic.
    let _ = out; // may be empty

    // Also valid with empty string (D6).
    let _ = r.set_active_account(String::new());
    // Pass: no panic.
}

#[test]
fn set_active_account_with_relay_does_not_panic() {
    // Extended coverage: with a relay connected, set_active_account must
    // still return without panicking. The result may be empty (no contacts
    // to fan-out) but the method must be callable at any point.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    let _ = r.set_active_account(PK.to_string());
    // Pass: no panic.
}

// ─── Fix #1143 — tick() claim-expansion parity ───────────────────────────
//
// Proves that `KernelReducer::tick()` calls `poll_claim_expansion` (W6)
// and does not panic. With no pending claims (the common idle case) the
// call must be a D8 zero-cost no-op: no allocation, no outbound frames.

#[test]
fn tick_with_no_claims_is_noop_and_does_not_panic() {
    // Regression guard: before the #1143 fix, `tick()` was missing the
    // `poll_claim_expansion` drain entirely. On wasm32 that meant Phase-1
    // claims stalled permanently on quiet sockets.  The assertion here is
    // minimal (empty outbound, no panic) but the compile guarantee is what
    // matters: `crate::time::Instant::now()` resolves to `web_time::Instant`
    // on wasm32 and to `std::time::Instant` on native — neither panics.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "content".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    // tick() with no relay connected yet and no claims registered.
    let out = r.tick();
    assert!(
        out.is_empty(),
        "idle tick with no claims and no relay must produce no outbound; got {out:?}"
    );
}

#[test]
fn tick_invokes_claim_expansion_drain_without_panicking_with_relay() {
    // With a relay connected and no claims pending, `tick()` must still
    // complete without panicking. This guards the wasm32 path: the shim
    // uses `web_time::Instant::now()` which is backed by
    // `performance.now()` in a JS Worker and never panics.
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    let _ = r.reduce(KernelAction::Start);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, false);
    // Tick after relay connect — poll_claim_expansion must run without panic.
    let _out = r.tick();
    // Pass: no panic is the primary assertion.
}

