//! M5+M2+M8 integration tests — NIP-42 AUTH wiring in the kernel.
//!
//! These tests drive `kernel::handle_text` with synthetic relay frames (the
//! same I/O surface a real WebSocket worker would produce). No live socket;
//! `MockRelay` would be redundant here because the handshake is deterministic
//! — feed frames in order, observe state + outbound. See task #57.
//!
//! Signer injection uses an inline closure adapter; in production the actor
//! wires `nmp_signers::AccountManager::signer_active()` to the same shape
//! (cross-crate cycle prevented by the callback indirection in
//! `kernel::auth::AuthSignerFn`).

use super::auth_test_helpers::*;
use super::*;
use crate::relay::{RelayRoleTestExt, DEFAULT_VISIBLE_LIMIT};
use crate::subs::RelayAuthState;

// ───────────────────────────────────────────────────────────────────────────
// Test 1 — nip42_kernel_auth_required_for_read
// ───────────────────────────────────────────────────────────────────────────
//
// Pins: relay sends AUTH → kernel transitions ChallengeReceived →
// Authenticating; kernel emits the `["AUTH", <signed_event>]` wire frame;
// any concurrent REQ to the same relay is held in the deferred queue.

#[test]
fn nip42_kernel_auth_required_for_read() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Inbound AUTH challenge from the content relay.
    let outbound = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );

    assert_eq!(*calls.lock().unwrap(), 1, "signer invoked exactly once");
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating
    );

    // Exactly one outbound frame: the signed kind:22242 AUTH event.
    let auth_msgs: Vec<_> = outbound
        .iter()
        .filter(|m| m.role == RelayRole::Content && m.text.starts_with("[\"AUTH\""))
        .collect();
    assert_eq!(auth_msgs.len(), 1, "exactly one AUTH wire frame emitted");
    assert!(auth_msgs[0].text.contains("\"kind\":22242"));
    assert!(auth_msgs[0].text.contains("\"challenge\""));
    assert!(auth_msgs[0].text.contains("ch1"));
    assert!(auth_msgs[0].text.contains(AUTH_EVENT_ID));

    // While Authenticating, any REQ targeting Content is held — the prior
    // call to req_for_relay() succeeds (caller still gets the OutboundMessage)
    // but the partition routine pulls it back into the deferred queue.
    // V-04 Stage 4: migrated from the retired `Kernel::req` helper; the test
    // exercises a single Content URL so a direct `req_for_relay` call is
    // equivalent to the prior loop-over-bootstrap-URLs shape.
    let _ = kernel.req_for_relay(
        RelayRole::Content,
        RelayRole::Content.url().to_string(),
        "test-sub",
        "test-summary",
        serde_json::json!({"kinds":[1],"limit":1}),
    );
    let outbound_after = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Content,
        relay_url: RelayRole::Content.url().to_string(),
        text: "[\"REQ\",\"test-sub\",{}]".to_string(),
    }]);
    assert!(
        outbound_after.is_empty(),
        "REQ to AUTH-paused relay must be deferred, not emitted"
    );
    assert!(
        kernel
            .deferred_outbound
            .iter()
            .any(|m| m.text.contains("test-sub")),
        "deferred queue holds the AUTH-paused REQ"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Test 2 — nip42_kernel_auth_failed_surfaces_relay_status
// ───────────────────────────────────────────────────────────────────────────
//
// Pins: relay rejects the AUTH event (`OK <id> false <reason>`) → driver
// transitions to Failed; RelayStatus.auth becomes "failed" and last_error
// carries the rejection reason.

#[test]
fn nip42_kernel_auth_failed_surfaces_relay_status() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating
    );

    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID, false, "restricted: subscribers only"),
    );

    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Failed
    );
    let status = kernel.relay_status_for(RelayRole::Content);
    assert_eq!(status.auth, "failed");
    assert!(
        status
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("restricted"),
        "rejection reason surfaced: {:?}",
        status.last_error
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Typed FFI error contract — a NIP-42 auth failure must stamp the machine-
// readable `error_category: Some("auth_required")` on the relay snapshot so
// iOS can branch on the error *class* without substring-matching English
// prose. Mirrors `nip42_kernel_auth_failed_surfaces_relay_status` above.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nip42_auth_failure_stamps_error_category_on_snapshot() {
    use crate::kernel::closed_reason::ERR_AUTH_REQUIRED;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Drive the NIP-42 handshake into Authenticating.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );

    // Before any failure the lane carries no category.
    assert_eq!(
        kernel.relay_status_for(RelayRole::Content).error_category,
        None,
        "no error_category before the AUTH event is rejected"
    );

    // Relay rejects the AUTH event — driver transitions to Failed.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID, false, "restricted: subscribers only"),
    );

    // The snapshot carries the typed category, not just the English prose.
    let status = kernel.relay_status_for(RelayRole::Content);
    assert_eq!(
        status.error_category.as_deref(),
        Some(ERR_AUTH_REQUIRED),
        "a NIP-42 auth failure must classify as auth_required; got {:?}",
        status.error_category
    );

    // It also survives into the full kernel snapshot's relay_statuses.
    let snapshot_status = kernel
        .relay_statuses()
        .into_iter()
        .find(|s| s.role == "content")
        .expect("content relay row present in snapshot");
    assert_eq!(
        snapshot_status.error_category.as_deref(),
        Some(ERR_AUTH_REQUIRED),
        "error_category must project into the snapshot relay_statuses row"
    );

    // A subsequent successful reconnect clears the stale category — iOS must
    // not keep showing the auth prompt after the lane recovers.
    kernel.relay_connected(RelayRole::Content);
    assert_eq!(
        kernel.relay_status_for(RelayRole::Content).error_category,
        None,
        "a fresh socket clears the prior auth_required category"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Typed FFI error contract — the kernel-level `last_error_category` must
// project into the emitted JSON snapshot as a snake_case key, and the legacy
// uncategorized `set_last_error_toast` path must clear a stale category so a
// newer toast never carries a misleading class.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn last_error_category_projects_into_snapshot_and_clears_on_legacy_toast() {
    use crate::kernel::closed_reason::ERR_PERMANENT;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // A categorized toast surfaces both fields in the snapshot JSON.
    kernel.set_error_toast_with_category("publish store error".to_string(), ERR_PERMANENT);
    let snap: serde_json::Value = serde_json::from_str(&kernel.make_update_json_for_test(true))
        .expect("snapshot is valid JSON");
    assert_eq!(
        snap["last_error_toast"].as_str(),
        Some("publish store error"),
        "categorized toast text surfaces in the snapshot"
    );
    assert_eq!(
        snap["last_error_category"].as_str(),
        Some(ERR_PERMANENT),
        "last_error_category projects into the snapshot as a snake_case key"
    );

    // A subsequent legacy (uncategorized) toast must clear the stale category
    // so iOS never branches on a class that no longer matches the toast.
    kernel.set_last_error_toast(Some("something else went wrong".to_string()));
    let snap: serde_json::Value = serde_json::from_str(&kernel.make_update_json_for_test(true))
        .expect("snapshot is valid JSON");
    assert_eq!(
        snap["last_error_toast"].as_str(),
        Some("something else went wrong")
    );
    assert!(
        snap["last_error_category"].is_null(),
        "legacy set_last_error_toast clears the stale category: {:?}",
        snap["last_error_category"]
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Test 3 — nip42_kernel_replays_pending_reqs_on_auth
// ───────────────────────────────────────────────────────────────────────────
//
// Pins: REQ issued while ChallengeReceived → deferred. OK accepted=true
// (Authenticated) → next `pending_view_requests` tick drains the deferred
// REQ back to outbound.

#[test]
fn nip42_kernel_replays_pending_reqs_on_auth() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Drive into ChallengeReceived → Authenticating.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    assert!(kernel.relay_auth_paused(RelayRole::Content));

    // Caller dispatches a REQ; the partition routine pulls it into deferred.
    let req_msg = OutboundMessage {
        role: RelayRole::Content,
        relay_url: RelayRole::Content.url().to_string(),
        text: "[\"REQ\",\"timeline-1\",{\"kinds\":[1]}]".to_string(),
    };
    let pass = kernel.partition_auth_paused(vec![req_msg]);
    assert!(pass.is_empty());
    assert_eq!(kernel.deferred_outbound.len(), 1);

    // Relay accepts AUTH; driver transitions to Authenticated. The OK frame
    // by itself does not flush the deferred queue (M5+M2+M8: lifecycle
    // owns the flush trigger; the actor's next tick reads
    // `pending_view_requests` which drains).
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID, true, ""),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticated
    );
    assert!(!kernel.relay_auth_paused(RelayRole::Content));

    // Next tick: deferred queue drains; the REQ flows through.
    let drained = kernel.pending_view_requests();
    assert!(
        drained.iter().any(|m| m.text.contains("timeline-1")),
        "deferred REQ replayed on Authenticated tick: {drained:?}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Test 4 — nip42_kernel_publish_retry_on_auth_required
// ───────────────────────────────────────────────────────────────────────────
//
// Pins (spec-named test from task #57): after a Failed AUTH the relay
// re-issues a fresh challenge — the kernel-side analogue of a publish
// AUTH-REQUIRED retry cycle, except the trigger is relay re-prompt rather
// than publish-engine policy. A second signer invocation cycle drives
// back to Authenticated.
//
// The publish engine in `crates/nmp-core/src/publish/` handles an
// outbound `auth-required` ack by PARKING the publish (demoting the relay
// to durable Pending via the availability gate) until the socket reaches
// `Authenticated`, at which point this very handler re-opens the gate via
// `mark_publish_relay_available` — pinned independently by
// `crates/nmp-core/src/publish/engine/auth_park_tests.rs` and the
// `t117_auth_required_on_one_relay_parks_until_authenticated_*` kernel
// test. Both code paths are intentional per `docs/perf/m5/nip42.md`
// "coordination notes" — this test exercises the kernel AUTH FSM side;
// the publish tests exercise the publish-engine park/re-dispatch side.

#[test]
fn nip42_kernel_publish_retry_on_auth_required() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // First challenge → AUTH sent → relay rejects.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID, false, "auth-required"),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Failed
    );
    assert_eq!(*calls.lock().unwrap(), 1);

    // Caller queues a REQ during the Failed window. T76 / ADR-0019:
    // Failed is FAIL-CLOSED — the REQ is dropped, never emitted to an
    // unauthenticated relay and never deferred (bounded-buffer).
    let pass = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Content,
        relay_url: RelayRole::Content.url().to_string(),
        text: "[\"REQ\",\"thread-1\",{\"kinds\":[1]}]".to_string(),
    }]);
    assert!(
        pass.is_empty(),
        "Failed state is fail-closed: REQ must not reach the relay"
    );
    assert!(
        !kernel
            .deferred_outbound
            .iter()
            .any(|m| m.text.contains("thread-1")),
        "fail-closed REQ must be dropped, not deferred"
    );

    // Rebind the signer with a fresh event-id so the second handshake can
    // be correlated independently (a real signer would naturally produce a
    // distinct id for a distinct created_at + challenge).
    let (signer2, calls2) = make_signer(AUTH_EVENT_ID_2);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer2);

    // Relay re-prompts (publish-side AUTH-REQUIRED retry equivalent).
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch2"),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating
    );
    assert_eq!(*calls2.lock().unwrap(), 1, "signer re-invoked on re-AUTH");

    // Accept the second handshake.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID_2, true, ""),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticated
    );

    // After re-auth completion no REQ should remain auth-held — the relay
    // is live.
    assert!(!kernel.relay_auth_paused(RelayRole::Content));
}

// ───────────────────────────────────────────────────────────────────────────
// Test 5 — nip42_kernel_auth_does_not_bump_view_rev (D8 invariant)
// ───────────────────────────────────────────────────────────────────────────
//
// Pins: AUTH-state transitions DO NOT directly bump `kernel.rev`. The
// `changed_since_emit` flag IS set so the diagnostic surface re-emits on
// the next actor tick (required by `docs/plan/m5-nip42.md` §19 — Failed
// AUTH must be visible), but the rev counter advances only via
// `make_update` which the actor schedules at ≤60 Hz/view (D8).
//
// The narrower invariant pinned here: AUTH-paused REQ re-defers (the
// `pending_view_requests` drain → still-paused re-defer loop) do NOT bump
// `changed_since_emit` — otherwise the actor would emit every tick for
// the entire AUTH-pause window. This is the test most likely to regress
// silently if a future agent moves the auth-pause defer onto the noisy
// `defer_outbound` instead of the silent variant.

#[test]
fn nip42_kernel_auth_does_not_bump_view_rev() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let rev_before = kernel.rev;
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID, true, ""),
    );
    assert_eq!(
        kernel.rev, rev_before,
        "AUTH transitions must not directly bump kernel.rev (only make_update does)"
    );

    // Auth-pause re-defer invariant: simulate ChallengeReceived → 10 ticks
    // of `pending_view_requests` (each drains + re-defers the held REQ).
    // The dirty flag must NOT keep getting set or the actor will busy-emit.
    let _ = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch-idx"),
    );
    let _ = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Indexer,
        relay_url: RelayRole::Indexer.url().to_string(),
        text: "[\"REQ\",\"x\",{}]".to_string(),
    }]);
    kernel.changed_since_emit = false; // post-emit baseline
    for _ in 0..10 {
        let _ = kernel.pending_view_requests();
    }
    assert!(
        !kernel.changed_since_emit,
        "10 ticks of auth-paused REQ re-defer must NOT bump changed_since_emit"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Bonus regression: AUTH with no signer bound stays in ChallengeReceived
// (the iOS-not-yet-authenticated case). Documents the no-signer path so
// future agents don't accidentally make it a panic.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nip42_kernel_auth_without_signer_holds_in_challenge_received() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let outbound = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    assert!(outbound.is_empty(), "no signer = no wire frame emitted");
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::ChallengeReceived
    );
    assert!(kernel.relay_auth_paused(RelayRole::Content));
}

// ───────────────────────────────────────────────────────────────────────────
// Bonus regression: actor-flow integration — claim REQs are partitioned
// at the single `send_all_outbound` choke point. This test mirrors what the
// actor does for ActorCommand::ClaimProfile: it calls `kernel.claim_profile()`
// (which emits a kind:0 REQ to the Indexer) and feeds the output through
// `partition_auth_paused` (the routine `send_all_outbound` calls). Without
// the relay_mgmt.rs choke-point change, this test would fail — the claim REQs
// would bypass the AUTH gate.
//
// V-112 (ADR-0042): original test used `kernel.open_author()` (deleted).
// Updated to use `kernel.claim_profile()` which also emits an Indexer REQ.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nip42_kernel_claim_reqs_routed_through_auth_gate() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Drive Indexer into ChallengeReceived → Authenticating.
    let _ = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch1"),
    );
    assert!(kernel.relay_auth_paused(RelayRole::Indexer));

    // Claim a profile. claim_profile() emits a kind:0 REQ to the Indexer;
    // the Indexer-bound REQs should be deferred because the Indexer relay is
    // AUTH-paused.
    let outbound = kernel.claim_profile(
        "1234567812345678123456781234567812345678123456781234567812345678".to_string(),
        "auth-gate-test".to_string(),
        true,
        false,
    );
    let post_partition = kernel.partition_auth_paused(outbound);

    // No Indexer-targeted frames make it through.
    assert!(
        !post_partition
            .iter()
            .any(|m| m.role == RelayRole::Indexer && m.text.starts_with("[\"REQ\"")),
        "Indexer REQs must be diverted while AUTH-paused: {post_partition:?}"
    );
    // Indexer REQs are now in the defer queue.
    assert!(
        kernel
            .deferred_outbound
            .iter()
            .any(|m| m.role == RelayRole::Indexer),
        "deferred queue holds the AUTH-paused Indexer REQ"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// T125 — kind:22242 `relay` tag and outbound routing key must reference the
//        DELIVERING relay's URL, not the role's bootstrap URL.
// ───────────────────────────────────────────────────────────────────────────
//
// NIP-42 binds the AUTH event to the URL of the relay that issued the
// challenge: the `["relay", <url>]` tag is the canonical anti-replay
// surface, and relays validate it against the socket the AUTH arrived on.
// Pre-T125 the kernel stamped `role.url()` (i.e. the lane's bootstrap host
// — the shared content bootstrap URL for the Content lane) regardless of which
// resolved relay sent the CHALLENGE. After T105's URL-keyed transport pool
// (`fada22b`), that ALSO routed the AUTH response to the wrong socket.
//
// Two distinct relays issue distinct challenges; each AUTH response must
// carry the matching delivering URL on BOTH the `relay` tag AND the
// outbound `relay_url` routing field. We parse the wire frames rather than
// substring-match, because the bootstrap URL and
// our test URLs ("wss://a.example", "wss://b.example") would otherwise
// require fragile contains/disjointness checks.
#[test]
fn nip42_kind_22242_tags_delivering_relay_url_not_bootstrap() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Two signer instances with distinct fixed event ids so each AUTH
    // dispatch has an independently correlatable id (the advisor's note —
    // a single signer would have the second `record_dispatch` overwrite
    // the first's pending_event_id, but the bug under test is the OUTBOUND
    // frame contents, not the driver state, so we never drive OK here).
    let url_a = "wss://a.example";
    let url_b = "wss://b.example";

    // First relay's challenge → first signed AUTH.
    let (signer_a, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer_a);
    let outbound_a = kernel.handle_text(RelayRole::Content, url_a, &auth_frame("ch-a"));

    let auth_frame_a = outbound_a
        .iter()
        .find(|m| m.text.starts_with("[\"AUTH\""))
        .expect("AUTH outbound from relay A");
    assert_eq!(
        auth_frame_a.relay_url, url_a,
        "T125: OutboundMessage.relay_url must equal the delivering URL so \
         the URL-keyed transport (fada22b) dials the right socket; \
         pre-T125 this stamped role.url() = bootstrap"
    );
    let relay_tag_a = extract_relay_tag(&auth_frame_a.text);
    assert_eq!(
        relay_tag_a,
        url_a,
        "T125: kind:22242 `relay` tag must equal the delivering URL per \
         NIP-42 (anti-replay binds AUTH to the issuing relay); pre-T125 \
         this stamped role.url() = bootstrap ({:?})",
        RelayRole::Content.url()
    );

    // Second relay's challenge — rebind a signer with a distinct event id
    // so the driver's pending_event_id correlation is unambiguous, then
    // confirm the second AUTH carries url_b on both fields.
    let (signer_b, _) = make_signer(AUTH_EVENT_ID_2);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer_b);
    let outbound_b = kernel.handle_text(RelayRole::Content, url_b, &auth_frame("ch-b"));

    let auth_frame_b = outbound_b
        .iter()
        .find(|m| m.text.starts_with("[\"AUTH\""))
        .expect("AUTH outbound from relay B");
    assert_eq!(
        auth_frame_b.relay_url, url_b,
        "T125: second AUTH must route to delivering URL B"
    );
    let relay_tag_b = extract_relay_tag(&auth_frame_b.text);
    assert_eq!(
        relay_tag_b, url_b,
        "T125: second AUTH `relay` tag must equal delivering URL B"
    );

    // Crucial cross-check: the two AUTH frames carry DISTINCT relay tags.
    // Pre-T125 they'd both be `RelayRole::Content.url()` (bootstrap) and
    // this would fail with a clear A=B mismatch.
    assert_ne!(
        relay_tag_a, relay_tag_b,
        "T125: distinct delivering URLs must produce distinct AUTH `relay` tags"
    );
}

/// Parse `["AUTH", {<event>}]` text and return the value of the `relay` tag.
/// Panics with a helpful message if the frame shape is unexpected — these
/// are tests, so loud failure is the right policy.
fn extract_relay_tag(wire: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(wire).expect("AUTH frame is valid JSON");
    let arr = v.as_array().expect("AUTH frame is a JSON array");
    assert_eq!(arr.first().and_then(|s| s.as_str()), Some("AUTH"));
    let event = arr.get(1).expect("AUTH frame has event payload");
    let tags = event
        .get("tags")
        .and_then(|t| t.as_array())
        .expect("event has tags array");
    for tag in tags {
        let parts = tag.as_array().expect("tag is array");
        if parts.first().and_then(|s| s.as_str()) == Some("relay") {
            return parts
                .get(1)
                .and_then(|s| s.as_str())
                .expect("relay tag has url")
                .to_string();
        }
    }
    panic!("no `relay` tag found in AUTH event: {wire}")
}

// ───────────────────────────────────────────────────────────────────────────
// Per-role signer isolation — `set_relay_auth_signer` binds ONE role.
//
// The handshake tests above use `bind_auth_signer`, which binds Content AND
// Indexer with one call, so the per-role `auth_signers` map's isolation is
// never exercised. These tests pin the `RelayRole`-keyed lookup in
// `handle_auth_challenge`: a signer bound to one role must not answer a
// challenge delivered on another. A regression here would silently sign an
// AUTH event for the wrong lane's relay (D0: AUTH state is per-transport).
// ───────────────────────────────────────────────────────────────────────────

/// A signer bound to `Content` only does NOT answer an `Indexer` challenge:
/// the Indexer driver stays in `ChallengeReceived` and no AUTH frame is sent.
#[test]
fn nip42_per_role_signer_does_not_answer_other_role_challenge() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    // Bind ONLY the Content role (per-role primitive, not the compat wrapper).
    kernel.set_relay_auth_signer(RelayRole::Content, SIGNER_PUBKEY.to_string(), signer);

    // Challenge arrives on the Indexer lane, which has no signer.
    let outbound = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch-idx"),
    );

    assert_eq!(
        *calls.lock().unwrap(),
        0,
        "a Content-bound signer must not be invoked for an Indexer challenge",
    );
    assert!(
        outbound.is_empty(),
        "no signer for the Indexer role => no AUTH wire frame emitted",
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Indexer),
        RelayAuthState::ChallengeReceived,
        "the Indexer driver records the challenge but stays unanswered",
    );
}

/// With a signer bound to `Content` only, a `Content` challenge IS answered
/// while a concurrent `Indexer` challenge is NOT — the two lanes resolve
/// independently against the per-role `auth_signers` map.
#[test]
fn nip42_signer_answers_bound_role_only_across_concurrent_challenges() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    kernel.set_relay_auth_signer(RelayRole::Content, SIGNER_PUBKEY.to_string(), signer);

    // Indexer challenge first — unanswered (no signer for that role).
    let idx_out = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch-idx"),
    );
    assert!(idx_out.is_empty(), "Indexer has no signer => no AUTH frame");

    // Content challenge — answered by the bound signer.
    let content_out = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch-content"),
    );
    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "the Content-bound signer fires exactly once, for the Content challenge",
    );
    assert!(
        content_out
            .iter()
            .any(|m| m.role == RelayRole::Content && m.text.starts_with("[\"AUTH\"")),
        "the Content challenge is answered with a kind:22242 AUTH frame",
    );

    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating,
        "the Content lane advances to Authenticating",
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Indexer),
        RelayAuthState::ChallengeReceived,
        "the Indexer lane stays in ChallengeReceived — its signer is unbound",
    );
}

/// `handle_auth_challenge` stores the verbatim challenge string on the
/// per-role NIP-42 driver. The handshake tests assert state transitions but
/// never the stored challenge value directly; this pins that the kernel
/// retains the exact challenge it must echo back in the kind:22242 tag.
#[test]
fn nip42_auth_challenge_is_stored_verbatim_on_driver() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No signer bound — isolates the challenge-capture step from signing.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("challenge-xyz-123"),
    );

    let driver = kernel
        .auth_drivers
        .get(&RelayRole::Content)
        .expect("an AUTH challenge must create the per-role driver entry");
    assert_eq!(
        driver.pending_challenge.as_deref(),
        Some("challenge-xyz-123"),
        "the kernel stores the relay's challenge string verbatim",
    );
    assert_eq!(
        driver.state,
        RelayAuthState::ChallengeReceived,
        "the driver transitions to ChallengeReceived on the AUTH frame",
    );
}
