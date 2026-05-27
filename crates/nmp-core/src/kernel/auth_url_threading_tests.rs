//! T148 — per-URL keying of the lifecycle `AuthGate`.
//!
//! T125 fixed the AUTH-tagging side (NIP-42 kind:22242 events now carry the
//! delivering relay's URL in the `["relay", ...]` tag). T148 fixes the
//! lifecycle AuthGate side: `kernel::ingest::auth_handlers::handle_auth_*`
//! were still passing `role.url()` (the lane's bootstrap host) into
//! `SubscriptionLifecycle::handle_auth_state_change`, which keys an
//! internal per-URL `BTreeMap<RelayUrl, _>`. The pre-T148 effect: if the
//! AUTH challenge arrived on a non-bootstrap relay, the lifecycle's
//! pending-REQ buffer for THAT URL was never paused; instead a phantom
//! pause was recorded for the bootstrap URL, which had no in-flight subs.
//!
//! These tests pin the invariant: the URL that ENTERS the lifecycle is the
//! URL that DELIVERED the AUTH frame — not a lane bootstrap label.
//!
//! ## What this DOES NOT test
//!
//! The kernel-side `auth_drivers: HashMap<RelayRole, _>` continues to key
//! by role, not URL — splitting that into per-URL drivers is a separate,
//! larger change (one socket per URL is already invariant per T126; the
//! per-role driver collapses correctly today because each lane has at most
//! one connected URL in production). T148 only fixes the lifecycle side.

use super::auth_test_helpers::*;
use super::*;
use crate::relay::{RelayRoleTestExt, DEFAULT_VISIBLE_LIMIT};
use crate::subs::RelayAuthState;

const NON_BOOTSTRAP_URL: &str = "wss://nip65-resolved.example/";

#[test]
fn lifecycle_auth_gate_keys_on_delivering_url_not_bootstrap_for_challenge() {
    // Pre-T148: a challenge from URL_B mis-routed into the AuthGate as the
    // bootstrap URL of `role`, so URL_B's pending buffer was never armed.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _calls) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // AUTH challenge arrives on a non-bootstrap URL (a resolved write-relay,
    // not the cold-start seed).
    let _ = kernel.handle_text(
        RelayRole::Content,
        NON_BOOTSTRAP_URL,
        &auth_frame("ch-from-b"),
    );

    // The lifecycle's per-URL gate must now mark THIS URL paused…
    assert!(
        kernel.lifecycle.is_auth_paused_for_url(NON_BOOTSTRAP_URL),
        "delivering URL {NON_BOOTSTRAP_URL} must be paused after its AUTH challenge"
    );
    // …and the bootstrap URL must NOT be paused (no challenge arrived there).
    let bootstrap = RelayRole::Content.url();
    assert!(
        !kernel.lifecycle.is_auth_paused_for_url(bootstrap),
        "bootstrap URL {bootstrap} must NOT be paused — no challenge arrived there"
    );
}

#[test]
fn lifecycle_auth_gate_keys_on_delivering_url_on_authenticated() {
    // OK accepted=true arrives on URL_B. The lifecycle must clear URL_B's
    // pause, not the bootstrap URL's. We exercise this by first arming the
    // bootstrap URL's pause (record a state transition for it) and then
    // ensuring it stays paused after URL_B's Authenticated arrives.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _calls) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Drive the challenge on URL_B.
    let _ = kernel.handle_text(
        RelayRole::Content,
        NON_BOOTSTRAP_URL,
        &auth_frame("ch-from-b"),
    );
    assert!(kernel.lifecycle.is_auth_paused_for_url(NON_BOOTSTRAP_URL));

    // Independently arm the bootstrap URL's pause via the lifecycle directly
    // (simulates an earlier challenge that arrived on the seed socket).
    let bootstrap = RelayRole::Content.url().to_string();
    let _ = kernel
        .lifecycle
        .handle_auth_state_change(bootstrap.clone(), RelayAuthState::ChallengeReceived);
    assert!(kernel.lifecycle.is_auth_paused_for_url(&bootstrap));

    // Now URL_B authenticates. The lifecycle must un-pause URL_B and leave
    // the bootstrap URL untouched.
    let _ = kernel.handle_text(
        RelayRole::Content,
        NON_BOOTSTRAP_URL,
        &ok_frame(AUTH_EVENT_ID, true, ""),
    );
    assert!(
        !kernel.lifecycle.is_auth_paused_for_url(NON_BOOTSTRAP_URL),
        "URL_B must un-pause after its OK accepted=true"
    );
    assert!(
        kernel.lifecycle.is_auth_paused_for_url(&bootstrap),
        "bootstrap URL must remain paused — its OK never arrived"
    );
}

#[test]
fn lifecycle_auth_gate_keys_on_delivering_url_on_failed() {
    // Failed transition follows the same rule: it must record against the
    // delivering URL, not the bootstrap.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _calls) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    let _ = kernel.handle_text(
        RelayRole::Content,
        NON_BOOTSTRAP_URL,
        &auth_frame("ch-from-b"),
    );

    // Relay rejects.
    let _ = kernel.handle_text(
        RelayRole::Content,
        NON_BOOTSTRAP_URL,
        &ok_frame(
            AUTH_EVENT_ID,
            false,
            "auth-required: signer not on allowlist",
        ),
    );

    // URL_B is failed → still considered paused (fail-closed).
    assert!(
        kernel.lifecycle.is_auth_paused_for_url(NON_BOOTSTRAP_URL),
        "URL_B Failed → still paused (fail-closed per ADR-0019)"
    );
    // Bootstrap is untouched.
    assert!(
        !kernel
            .lifecycle
            .is_auth_paused_for_url(RelayRole::Content.url()),
        "bootstrap URL must be unaffected by URL_B's Failed transition"
    );
}

#[test]
fn closed_auth_required_pauses_delivering_url_not_bootstrap() {
    // ingest/closed.rs::on_closed_auth_required keys the same way: an
    // `auth-required:` CLOSED frame arriving on URL_B must pause URL_B in
    // the lifecycle's gate, not the bootstrap URL.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Synthesize a CLOSED frame with reason `auth-required: <text>`.
    let closed_frame =
        serde_json::json!(["CLOSED", "test-sub", "auth-required: signer required"]).to_string();
    let _ = kernel.handle_text(RelayRole::Content, NON_BOOTSTRAP_URL, &closed_frame);

    assert!(
        kernel.lifecycle.is_auth_paused_for_url(NON_BOOTSTRAP_URL),
        "delivering URL must be paused after `auth-required:` CLOSED frame"
    );
    assert!(
        !kernel
            .lifecycle
            .is_auth_paused_for_url(RelayRole::Content.url()),
        "bootstrap URL must not be falsely paused"
    );
}
