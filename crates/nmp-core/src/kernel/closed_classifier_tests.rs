//! T120 — NIP-01 CLOSED reason-prefix classifier integration tests.
//!
//! Pure parser tests live alongside the classifier in `closed_reason.rs`.
//! These tests drive `Kernel::handle_text` with synthetic CLOSED frames and
//! assert that the kernel-level side effects fire per the policy table in
//! `ingest/closed.rs`:
//!
//! - `auth-required:` → `RelayHealth.auth == "challenge_received"` (pauses
//!   the AuthGate; the actual signing happens when the relay sends its own
//!   `["AUTH", challenge]` frame — we do not synthesize a pseudo-challenge).
//! - `rate-limited:`  → `RelayHealth.last_close_reason == "rate-limited"`
//!   and `last_error` carries the reason.
//! - `restricted:` / `blocked:` / `shadowbanned:` → `RelayHealth.denied`.
//! - Unknown prefix → folds to error-policy (log + give up); no `denied`,
//!   no auth pause.
//!
//! These tests pin the routing table so regressions surface at the kernel
//! boundary, not at the per-call site.

use super::*;
use crate::relay::{RelayRoleTestExt, DEFAULT_VISIBLE_LIMIT};
use crate::subs::RelayAuthState;

fn closed_frame(sub_id: &str, reason: &str) -> String {
    serde_json::json!(["CLOSED", sub_id, reason]).to_string()
}

#[test]
fn closed_auth_required_triggers_auth_pause() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-1", "auth-required: please AUTH"),
    );

    let relay = kernel.relay(role);
    assert_eq!(
        relay.auth, "challenge_received",
        "auth-required CLOSED must transition the auth surface to challenge_received"
    );
    assert_eq!(
        relay.last_close_reason.as_deref(),
        Some("auth-required"),
        "diagnostic key matches NIP-01 prefix"
    );

    // The lifecycle AuthGate must also see the pause — REQs to this URL
    // partition out (mirrors the AUTH-frame ingest path).
    let paused_after_pause = kernel
        .lifecycle
        .handle_auth_state_change(role.url().to_string(), RelayAuthState::ChallengeReceived);
    assert!(
        paused_after_pause.is_empty(),
        "second pause is a no-op transition; nothing to flush"
    );
}

#[test]
fn closed_rate_limited_records_classification_no_denied() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-2", "rate-limited: slow down"),
    );

    let relay = kernel.relay(role);
    assert_eq!(relay.last_close_reason.as_deref(), Some("rate-limited"));
    assert!(
        relay
            .last_error
            .as_deref()
            .map(|s| s.contains("rate-limited"))
            .unwrap_or(false),
        "last_error must mention the rate-limited classification"
    );
    assert!(
        !relay.denied,
        "rate-limited must NOT mark the relay denied — recovery is retry-with-backoff"
    );
    assert_eq!(
        relay.auth, "not_required",
        "rate-limited must NOT touch the AUTH surface"
    );
}

#[test]
fn closed_restricted_marks_relay_denied() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-3", "restricted: paid only"),
    );

    let relay = kernel.relay(role);
    assert!(relay.denied, "restricted must mark the relay denied");
    assert_eq!(relay.last_close_reason.as_deref(), Some("restricted"));
    assert!(
        relay
            .last_error
            .as_deref()
            .map(|s| s.contains("denied"))
            .unwrap_or(false),
        "last_error must surface the denial"
    );
}

#[test]
fn closed_blocked_marks_relay_denied() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Indexer;
    let _ = kernel.handle_text(role, role.url(), &closed_frame("sub-4", "blocked: spam"));

    let relay = kernel.relay(role);
    assert!(relay.denied, "blocked must mark the relay denied");
    assert_eq!(relay.last_close_reason.as_deref(), Some("blocked"));
}

#[test]
fn closed_shadowbanned_marks_relay_denied() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-5", "shadowbanned: sorry"),
    );

    let relay = kernel.relay(role);
    assert!(
        relay.denied,
        "shadowbanned routes to denied (same as blocked)"
    );
    assert_eq!(relay.last_close_reason.as_deref(), Some("shadowbanned"));
}

#[test]
fn closed_unknown_prefix_folds_to_error_no_denied_no_auth_pause() {
    // Unknown prefix MUST behave like `error:` — record classification +
    // last_error, no `denied` flag, no AUTH-pause.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-6", "totally-made-up: oops"),
    );

    let relay = kernel.relay(role);
    assert_eq!(
        relay.last_close_reason.as_deref(),
        Some("unknown"),
        "unknown prefix records the unknown classification key"
    );
    assert!(
        !relay.denied,
        "unknown prefix must NOT mark relay denied — only restricted/blocked/shadowbanned do"
    );
    assert_eq!(
        relay.auth, "not_required",
        "unknown prefix must NOT pause AUTH — only auth-required does"
    );
}

#[test]
fn closed_error_logs_and_records_classification() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(role, role.url(), &closed_frame("sub-7", "error: internal"));

    let relay = kernel.relay(role);
    assert_eq!(relay.last_close_reason.as_deref(), Some("error"));
    assert!(!relay.denied, "generic error never marks denied");
    assert_eq!(
        relay.auth, "not_required",
        "generic error leaves AUTH alone"
    );
}

#[test]
fn closed_reconnect_clears_denied_flag() {
    // A fresh socket means policy may have changed (user paid, relay
    // operator changed mind). `relay_connected` clears `denied` so the
    // reconnect machinery does not permanently brand the relay.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-8", "restricted: paid only"),
    );
    assert!(kernel.relay(role).denied);

    kernel.relay_connected(role);
    let relay = kernel.relay(role);
    assert!(
        !relay.denied,
        "relay_connected (fresh socket) must clear the denied flag"
    );
    assert!(
        relay.last_close_reason.is_none(),
        "relay_connected resets last_close_reason"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `wire_subs` eviction — the `"CLOSED"` arm in `ingest/mod.rs` removes the
// `(relay_url, sub_id)` row before `classify_and_route_closed` runs.
//
// The classifier tests above pin the `RelayHealth` side effects; the basic
// known-sub eviction is pinned by `retention_tests::closed_frame_evicts_
// wire_sub_row`. These tests fill the remaining gaps: the unknown-sub no-op
// (no panic, no phantom row) and the relay-scoped `#170` keying invariant
// (a CLOSED on one relay must not evict a sibling's row for the same
// sub-id) — both currently uncovered for the CLOSED ingest path.
// ───────────────────────────────────────────────────────────────────────────

/// Seed a `wire_subs` row for `(relay_url, sub_id)` via the production REQ
/// path so the row carries a realistic shape (state, relay_url, …).
fn seed_wire_sub(kernel: &mut Kernel, role: RelayRole, relay_url: &str, sub_id: &str) {
    let _ = kernel.req_for_relay(
        role,
        relay_url.to_string(),
        sub_id,
        "test-summary",
        serde_json::json!({"kinds": [1], "limit": 1}),
    );
}

/// A CLOSED frame for a sub-id with no `wire_subs` row is a graceful no-op:
/// no panic, and the classifier still records the diagnostic reason on the
/// relay surface. The relay-scoped key means an unknown key is just a
/// `HashMap::remove` miss.
#[test]
fn closed_for_unknown_sub_is_graceful_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let url = role.url();

    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "precondition: no wire_subs rows exist",
    );

    // Must not panic on a CLOSED for a sub-id the kernel never opened.
    let _ = kernel.handle_text(role, url, &closed_frame("sub-never-opened", "error: gone"));

    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "a CLOSED for an unknown sub_id must not create a wire_subs row",
    );
    // The classifier still runs — the diagnostic reason lands on RelayHealth.
    assert_eq!(
        kernel.relay(role).last_close_reason.as_deref(),
        Some("error"),
        "an unknown-sub CLOSED still records its classified reason",
    );
}

/// `#170` relay-scoped keying: the same `sub_id` lives on two relays; a
/// CLOSED on relay A evicts only relay A's row and leaves relay B's row
/// alive. Pins the `(relay_url, sub_id)` key against a silent regression to
/// sub-id-only keying (which would evict the sibling).
#[test]
fn closed_eviction_is_relay_scoped_sibling_survives() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Canonical URL form (no empty-path trailing slash). `req_for_relay` and
    // the CLOSED handler both canonicalize the relay URL before keying
    // `wire_subs` (T-relay-url-normalize), so the relay-scoped-keying
    // assertions below operate on the canonical form.
    let url_a = "wss://a.closed.example";
    let url_b = "wss://b.closed.example";
    let shared = "sub-shared";

    seed_wire_sub(&mut kernel, RelayRole::Content, url_a, shared);
    seed_wire_sub(&mut kernel, RelayRole::Content, url_b, shared);
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        2,
        "precondition: both relays carry the shared sub_id under distinct keys",
    );

    // CLOSED arrives on relay A only.
    let _ = kernel.handle_text(
        RelayRole::Content,
        url_a,
        &closed_frame(shared, "error: relay A gone"),
    );

    let active = kernel.snapshot_active_wire_subs();
    assert!(
        !active.iter().any(|(sid, u)| sid == shared && u == url_a),
        "relay A's row must be evicted by its own CLOSED: {active:?}",
    );
    assert!(
        active.iter().any(|(sid, u)| sid == shared && u == url_b),
        "#170: relay B's row for the same sub_id must survive a CLOSED on \
         relay A — eviction is relay-scoped, not sub-id-only: {active:?}",
    );
}

// ─── V-58 — rate-limited CLOSED enqueues a backoff hint ─────────────────────

/// A `CLOSED ["rate-limited: …"]` frame must enqueue exactly one backoff hint
/// (URL = the delivering relay URL) so the actor can forward it to the pool
/// worker for a long reconnect delay.
///
/// This pins the kernel side of the V-58 fix: `on_closed_rate_limited` must
/// write to `pending_backoff_hints`, and `take_backoff_hints` must drain it.
#[test]
fn v58_rate_limited_closed_enqueues_backoff_hint() {
    use crate::kernel::BackoffHint;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let url = role.url();

    // Before the frame: queue must be empty.
    assert!(
        kernel.take_backoff_hints().is_empty(),
        "no hints expected before any CLOSED frame"
    );

    let _ = kernel.handle_text(role, url, &closed_frame("sub-rl", "rate-limited: slow down"));

    let hints = kernel.take_backoff_hints();
    assert_eq!(hints.len(), 1, "exactly one hint must be enqueued");
    let (hint_url, hint_class) = &hints[0];
    assert_eq!(hint_url, url, "hint must carry the delivering relay URL");
    assert_eq!(
        *hint_class,
        BackoffHint::RateLimited,
        "hint class must be RateLimited"
    );

    // Queue must be drained after take_backoff_hints.
    assert!(
        kernel.take_backoff_hints().is_empty(),
        "take_backoff_hints must drain the queue"
    );
}

/// A `CLOSED` for any reason other than `rate-limited:` must NOT enqueue
/// a backoff hint — only rate-limited causes the long-backoff signal.
#[test]
fn v58_non_rate_limited_closed_does_not_enqueue_hint() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Content;
    let url = role.url();

    for reason in [
        "error: internal",
        "restricted: paid only",
        "blocked: spam",
        "auth-required: please AUTH",
        "totally-made-up: unknown",
        "duplicate: same sub",
        "invalid: bad filter",
    ] {
        let _ = kernel.handle_text(role, url, &closed_frame("sub-x", reason));
        let hints = kernel.take_backoff_hints();
        assert!(
            hints.is_empty(),
            "reason {reason:?} must NOT enqueue a backoff hint, got: {hints:?}"
        );
    }
}

/// The per-frame CLOSED reason text is surfaced to the diagnostic snapshot:
/// `classify_and_route_closed` stamps `RelayHealth.last_error` with the raw
/// reason so the UI can show *why* the relay closed the subscription.
#[test]
fn closed_message_is_surfaced_to_relay_diagnostics() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let role = RelayRole::Indexer;
    let url = role.url();

    let _ = kernel.handle_text(
        role,
        url,
        &closed_frame("sub-msg", "error: backend connection lost"),
    );

    let status = kernel.relay_status_for(role);
    assert!(
        status
            .last_error
            .as_deref()
            .map(|s| s.contains("backend connection lost"))
            .unwrap_or(false),
        "the CLOSED reason message must be surfaced on the relay status \
         snapshot, got: {:?}",
        status.last_error,
    );
}
