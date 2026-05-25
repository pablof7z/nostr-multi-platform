//! Shared fixtures for the NIP-42 kernel integration tests. Split out so
//! `auth_tests.rs` (handshake happy/sad paths) and
//! `auth_fail_closed_tests.rs` (T76 fail-closed regressions) stay under
//! the AGENTS.md LOC ceiling and share one signer/frame vocabulary.

use super::*;
use std::sync::{Arc, Mutex};

/// Test pubkey hex — 32 bytes / 64 hex chars / arbitrary.
pub(super) const SIGNER_PUBKEY: &str =
    "abababababababababababababababababababababababababababababababab";
pub(super) const AUTH_EVENT_ID: &str =
    "1234567812345678123456781234567812345678123456781234567812345678";
pub(super) const AUTH_EVENT_ID_2: &str =
    "9876987698769876987698769876987698769876987698769876987698769876";

/// Build a "passing" signer that returns a `SignedEvent` whose id is the
/// supplied fixed id. Tracks invocation count so tests can assert re-AUTH
/// cycles.
pub(super) fn make_signer(
    fixed_id: &'static str,
) -> (crate::kernel::auth::AuthSignerFn, Arc<Mutex<usize>>) {
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

pub(super) fn auth_frame(challenge: &str) -> String {
    serde_json::json!(["AUTH", challenge]).to_string()
}

pub(super) fn ok_frame(event_id: &str, accepted: bool, reason: &str) -> String {
    serde_json::json!(["OK", event_id, accepted, reason]).to_string()
}

pub(super) fn auth_state_of(kernel: &Kernel, role: RelayRole) -> crate::subs::RelayAuthState {
    kernel
        .auth_drivers
        .get(&role)
        .map(|d| d.state.clone())
        .unwrap_or(crate::subs::RelayAuthState::NotRequired)
}
