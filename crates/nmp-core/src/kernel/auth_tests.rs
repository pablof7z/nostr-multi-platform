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

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::subs::RelayAuthState;
use std::sync::{Arc, Mutex};

/// Test pubkey hex — 32 bytes / 64 hex chars / arbitrary.
const SIGNER_PUBKEY: &str = "abababababababababababababababababababababababababababababababab";
const AUTH_EVENT_ID: &str = "1234567812345678123456781234567812345678123456781234567812345678";
const AUTH_EVENT_ID_2: &str = "9876987698769876987698769876987698769876987698769876987698769876";

/// Build a "passing" signer that returns a `SignedEvent` whose id is
/// `AUTH_EVENT_ID` (or the supplied override). Tracks invocation count so
/// tests can assert re-AUTH cycles.
fn make_signer(fixed_id: &'static str) -> (crate::kernel::auth::AuthSignerFn, Arc<Mutex<usize>>) {
    let count = Arc::new(Mutex::new(0_usize));
    let count_clone = Arc::clone(&count);
    let signer: crate::kernel::auth::AuthSignerFn = Arc::new(move |unsigned| {
        *count_clone.lock().unwrap() += 1;
        Ok(crate::substrate::SignedEvent {
            id: fixed_id.to_string(),
            sig: "f".repeat(128),
            unsigned: unsigned.clone(),
        })
    });
    (signer, count)
}

fn auth_frame(challenge: &str) -> String {
    serde_json::json!(["AUTH", challenge]).to_string()
}

fn ok_frame(event_id: &str, accepted: bool, reason: &str) -> String {
    serde_json::json!(["OK", event_id, accepted, reason]).to_string()
}

fn auth_state_of(kernel: &Kernel, role: RelayRole) -> RelayAuthState {
    kernel
        .nip42_drivers
        .get(&role)
        .map(|d| d.state.clone())
        .unwrap_or(RelayAuthState::NotRequired)
}

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
    let outbound = kernel.handle_text(RelayRole::Content, &auth_frame("ch1"));

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
    // call to req() succeeds (caller still gets the OutboundMessage) but the
    // partition routine pulls it back into the deferred queue.
    let _ = kernel.req(
        RelayRole::Content,
        "test-sub",
        "test-summary",
        serde_json::json!({"kinds":[1],"limit":1}),
    );
    let outbound_after = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Content,
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

    let _ = kernel.handle_text(RelayRole::Content, &auth_frame("ch1"));
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating
    );

    let _ = kernel.handle_text(
        RelayRole::Content,
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
    let _ = kernel.handle_text(RelayRole::Content, &auth_frame("ch1"));
    assert!(kernel.relay_auth_paused(RelayRole::Content));

    // Caller dispatches a REQ; the partition routine pulls it into deferred.
    let req_msg = OutboundMessage {
        role: RelayRole::Content,
        text: "[\"REQ\",\"timeline-1\",{\"kinds\":[1]}]".to_string(),
    };
    let pass = kernel.partition_auth_paused(vec![req_msg]);
    assert!(pass.is_empty());
    assert_eq!(kernel.deferred_outbound.len(), 1);

    // Relay accepts AUTH; driver transitions to Authenticated. The OK frame
    // by itself does not flush the deferred queue (M5+M2+M8: lifecycle
    // owns the flush trigger; the actor's next tick reads
    // `pending_view_requests` which drains).
    let _ = kernel.handle_text(RelayRole::Content, &ok_frame(AUTH_EVENT_ID, true, ""));
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
// The publish engine in `crates/nmp-core/src/publish/` carries its OWN
// `AckClass::AuthRequired` retry policy for outbound publishes — pinned
// independently by `crates/nmp-core/src/publish/tests.rs` (per-relay
// state machine tests). Both code paths are intentional per
// `docs/perf/m5/nip42.md` "coordination notes" — this test exercises
// the kernel side; publish/tests.rs exercises the publish-engine side.

#[test]
fn nip42_kernel_publish_retry_on_auth_required() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // First challenge → AUTH sent → relay rejects.
    let _ = kernel.handle_text(RelayRole::Content, &auth_frame("ch1"));
    let _ = kernel.handle_text(
        RelayRole::Content,
        &ok_frame(AUTH_EVENT_ID, false, "auth-required"),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Failed
    );
    assert_eq!(*calls.lock().unwrap(), 1);

    // Caller queues a REQ during the Failed window. Failed is pass-through
    // per AuthGate semantics (D7: the operator owns resolution path; the
    // buffer would grow without bound otherwise) — so the REQ is emitted,
    // not held.
    let pass = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Content,
        text: "[\"REQ\",\"thread-1\",{\"kinds\":[1]}]".to_string(),
    }]);
    assert_eq!(pass.len(), 1, "Failed state is pass-through (D7)");

    // Rebind the signer with a fresh event-id so the second handshake can
    // be correlated independently (a real signer would naturally produce a
    // distinct id for a distinct created_at + challenge).
    let (signer2, calls2) = make_signer(AUTH_EVENT_ID_2);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer2);

    // Relay re-prompts (publish-side AUTH-REQUIRED retry equivalent).
    let _ = kernel.handle_text(RelayRole::Content, &auth_frame("ch2"));
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating
    );
    assert_eq!(*calls2.lock().unwrap(), 1, "signer re-invoked on re-AUTH");

    // Accept the second handshake.
    let _ = kernel.handle_text(RelayRole::Content, &ok_frame(AUTH_EVENT_ID_2, true, ""));
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

    let _ = kernel.handle_text(RelayRole::Content, &auth_frame("ch1"));
    let _ = kernel.handle_text(RelayRole::Content, &ok_frame(AUTH_EVENT_ID, true, ""));
    assert_eq!(
        kernel.rev, rev_before,
        "AUTH transitions must not directly bump kernel.rev (only make_update does)"
    );

    // Auth-pause re-defer invariant: simulate ChallengeReceived → 10 ticks
    // of `pending_view_requests` (each drains + re-defers the held REQ).
    // The dirty flag must NOT keep getting set or the actor will busy-emit.
    let _ = kernel.handle_text(RelayRole::Indexer, &auth_frame("ch-idx"));
    let _ = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Indexer,
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
    let outbound = kernel.handle_text(RelayRole::Content, &auth_frame("ch1"));
    assert!(outbound.is_empty(), "no signer = no wire frame emitted");
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::ChallengeReceived
    );
    assert!(kernel.relay_auth_paused(RelayRole::Content));
}

// ───────────────────────────────────────────────────────────────────────────
// Bonus regression: actor-flow integration — view-open REQs are partitioned
// at the single `send_all_outbound` choke point. This test mirrors what the
// actor does for ActorCommand::OpenAuthor: it calls `kernel.open_author()`
// (which historically returned REQs straight to the wire) and feeds the
// output through `partition_auth_paused` (the routine `send_all_outbound`
// calls). Without the relay_mgmt.rs choke-point change, this test would
// fail — the view-open REQs would bypass the AUTH gate.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn nip42_kernel_view_open_reqs_routed_through_auth_gate() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Drive Indexer into ChallengeReceived → Authenticating.
    let _ = kernel.handle_text(RelayRole::Indexer, &auth_frame("ch1"));
    assert!(kernel.relay_auth_paused(RelayRole::Indexer));

    // Open an author view. open_author() emits REQs across Content +
    // Indexer; the Indexer-bound REQs should be deferred because the
    // Indexer relay is AUTH-paused (open_author dispatches relay-list and
    // profile REQs to the Indexer).
    let outbound = kernel.open_author(
        "1234567812345678123456781234567812345678123456781234567812345678".to_string(),
        true,
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
