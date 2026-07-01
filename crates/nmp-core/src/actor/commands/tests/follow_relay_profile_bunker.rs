//! Bunker sign-in tests split from follow/relay/profile command coverage.

use super::*;
use std::sync::Arc;

#[test]
fn sign_in_bunker_seeds_handshake_progress() {
    // Stage 3 of NIP-46 wiring: a shape-valid bunker:// URI seeds the
    // snapshot with `"connecting"` so the SwiftUI sign-in flow can render
    // progress immediately. The broker (Stage 4) drives the real handshake
    // and pushes subsequent progress via `BunkerHandshakeProgress`.
    //
    // Stage 4 also added a fallback: if no broker hook is installed, the
    // actor clears the seeded "connecting" stage and surfaces a toast.
    // ADR-0052 §D3: install a no-op hook into this runtime's per-app slot so
    // the test exercises the happy path.
    let (mut id, mut kernel) = fresh();
    id.install_bunker_hook_for_test(Arc::new(|_req| {}));
    let pk = "c".repeat(64);
    sign_in_bunker(
        &mut id,
        &mut kernel,
        &format!("bunker://{pk}?relay=wss://r.example"),
    );
    let handshake = id.bunker_handshake_for_test().expect("handshake seeded");
    assert_eq!(handshake.stage, "connecting");
    assert!(handshake.message.is_some());
    assert!(kernel.last_error_toast_snapshot().is_none());
}

#[test]
fn sign_in_bunker_rejects_malformed_uri() {
    let (mut id, mut kernel) = fresh();
    sign_in_bunker(&mut id, &mut kernel, "bunker://nope");
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("invalid bunker")));
}

#[test]
fn sign_in_bunker_without_broker_clears_progress_and_toasts() {
    // Stage 4: if no broker hook is installed when a URI arrives, the actor
    // clears the seeded "connecting" stage and surfaces a toast.
    let (mut id, mut kernel) = fresh();
    let pk = "d".repeat(64);
    sign_in_bunker(
        &mut id,
        &mut kernel,
        &format!("bunker://{pk}?relay=wss://r.example"),
    );
    assert!(
        id.bunker_handshake_for_test().is_none(),
        "no-hook path must clear the seeded handshake progress"
    );
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|t| t.contains("broker not initialised")));
}
