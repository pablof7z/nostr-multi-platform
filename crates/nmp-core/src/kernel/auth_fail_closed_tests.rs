//! T76 — NIP-42 failed-auth fail-closed regressions (ADR-0019).
//!
//! Split from `auth_tests.rs` (handshake happy/sad paths) so each file
//! stays under the AGENTS.md LOC ceiling and the fail-closed contract has
//! one cohesive home. Shared fixtures live in `auth_test_helpers`.

use super::auth_test_helpers::*;
use super::*;
use crate::relay::{RelayRoleTestExt, DEFAULT_VISIBLE_LIMIT};
use crate::subs::RelayAuthState;

// Spec'd by T76 / ADR-0019. An AUTH-required relay that REJECTS the AUTH
// event must FAIL-CLOSED for that relay:
//   1. its AUTH-gated REQs are withheld (dropped, not emitted, not deferred),
//   2. other relays are completely unaffected,
//   3. RelayStatus reflects the failure (auth="failed" + last_error),
//   4. a prior deferred REQ for the failed relay is purged (no late leak),
//   5. recovery is reconnect-only — relay_connected resets to NotRequired.

#[test]
fn nip42_kernel_failed_auth_fails_closed() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer);

    // Content relay demands AUTH; a REQ arrives while Authenticating and is
    // deferred (transient-pause path).
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch1"),
    );
    let early = kernel.partition_auth_paused(vec![OutboundMessage {
        role: RelayRole::Content,
        relay_url: RelayRole::Content.url().to_string(),
        text: "[\"REQ\",\"gated-1\",{\"kinds\":[1]}]".to_string(),
    }]);
    assert!(early.is_empty(), "REQ deferred while Authenticating");
    assert!(
        kernel
            .deferred_outbound
            .iter()
            .any(|m| m.text.contains("gated-1")),
        "deferred buffer holds the gated REQ pre-rejection"
    );

    // Relay REJECTS the AUTH event → fail-closed.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &ok_frame(AUTH_EVENT_ID, false, "restricted: members only"),
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Failed
    );

    // (3) status reflects it.
    let status = kernel.relay_status_for(RelayRole::Content);
    assert_eq!(status.auth, "failed");
    assert!(status
        .last_error
        .as_deref()
        .unwrap_or("")
        .contains("restricted"));

    // (4) the previously-deferred gated REQ was purged on the Failed
    // transition — it must NOT leak to the now-unauthenticated relay.
    assert!(
        !kernel
            .deferred_outbound
            .iter()
            .any(|m| m.text.contains("gated-1")),
        "fail-closed: prior deferred REQ purged on Failed transition"
    );
    let drained = kernel.pending_view_requests();
    assert!(
        !drained.iter().any(|m| m.text.contains("gated-1")),
        "purged REQ must never drain to the wire"
    );

    // (1) new REQs to the failed Content relay are dropped, AND
    // (2) a concurrent REQ to a healthy relay (Indexer) passes through.
    let partitioned = kernel.partition_auth_paused(vec![
        OutboundMessage {
            role: RelayRole::Content,
            relay_url: RelayRole::Content.url().to_string(),
            text: "[\"REQ\",\"gated-2\",{\"kinds\":[1]}]".to_string(),
        },
        OutboundMessage {
            role: RelayRole::Indexer,
            relay_url: RelayRole::Indexer.url().to_string(),
            text: "[\"REQ\",\"healthy-1\",{\"kinds\":[0]}]".to_string(),
        },
    ]);
    assert_eq!(
        partitioned.len(),
        1,
        "only the healthy relay's REQ passes through"
    );
    assert_eq!(partitioned[0].role, RelayRole::Indexer);
    assert!(partitioned[0].text.contains("healthy-1"));
    assert!(
        !kernel
            .deferred_outbound
            .iter()
            .any(|m| m.text.contains("gated-2")),
        "fail-closed: new gated REQ dropped, not buffered"
    );

    // (5) recovery is reconnect-only: reconnect resets the driver.
    kernel.relay_connected(RelayRole::Content);
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::NotRequired,
        "reconnect resets the NIP-42 driver for a fresh challenge"
    );
    assert!(!kernel.relay_auth_failed(RelayRole::Content));
}
