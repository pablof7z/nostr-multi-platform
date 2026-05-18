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
use crate::relay::DEFAULT_VISIBLE_LIMIT;
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
    let paused_after_pause = kernel.lifecycle.handle_auth_state_change(
        role.url().to_string(),
        RelayAuthState::ChallengeReceived,
    );
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
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-4", "blocked: spam"),
    );

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
    assert!(relay.denied, "shadowbanned routes to denied (same as blocked)");
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
    let _ = kernel.handle_text(
        role,
        role.url(),
        &closed_frame("sub-7", "error: internal"),
    );

    let relay = kernel.relay(role);
    assert_eq!(relay.last_close_reason.as_deref(), Some("error"));
    assert!(!relay.denied, "generic error never marks denied");
    assert_eq!(relay.auth, "not_required", "generic error leaves AUTH alone");
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
