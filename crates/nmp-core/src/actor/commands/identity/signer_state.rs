//! Signer-state handlers — NIP-46 / NIP-55 connection-state updates and
//! bunker-handshake lifecycle (start, progress, restore).

use crate::kernel::Kernel;

use super::dto::{BunkerHandshakeDto, SignerStateDto};
use super::runtime::IdentityRuntime;

/// Update the `"signer_state"` projection when the NIP-46 relay-layer
/// connection state changes. V-14 step b, generalised by ADR-0048 D6.
///
/// `state` is one of `"connected"` | `"reconnecting"` | `"failed"`.
/// `"connected"` is mapped to `"ready"` in the unified `SignerStateDto` surface
/// so NIP-46 and NIP-55 share the same state vocabulary.
/// `reason` carries the error message for `"reconnecting"` and `"failed"`.
///
/// D0: the connection state is an app noun — written to the shared
/// [`SignerStateSlot`] (read by the `"signer_state"` snapshot projection)
/// instead of a typed `KernelSnapshot` field. The slot write does NOT flip
/// `changed_since_emit`, so the kernel is marked dirty explicitly — otherwise
/// the refreshed projection could sit unemitted until an unrelated kernel
/// mutation triggered a tick.
pub(crate) fn bunker_connection_state_changed(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    state: String,
    reason: Option<String>,
) {
    identity.set_signer_state(Some(SignerStateDto::from_nip46_connection_state(
        &state, reason,
    )));
    kernel.mark_changed_since_emit();
}

/// Update the `"signer_state"` projection for a NIP-55 signer event.
///
/// ADR-0048 D6: called from the capability-bridge result path when the host
/// reports a NIP-55 operation outcome that affects the long-lived signer
/// health (e.g. signer unavailable, rejected, awaiting approval).
pub(crate) fn nip55_signer_state_changed(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    state: String,
    reason: Option<String>,
) {
    identity.set_signer_state(Some(SignerStateDto::new(
        "nip55".to_string(),
        state,
        reason,
    )));
    kernel.mark_changed_since_emit();
}

/// Broker adapter → actor: latest NIP-46 handshake progress. Stage `"idle"`
/// clears the projection; everything else replaces it.
///
/// D0: the handshake state is an app noun, so it is written to the shared
/// [`BunkerHandshakeSlot`] (read by the `"bunker_handshake"` snapshot
/// projection) instead of a typed `KernelSnapshot` field. The slot write does
/// NOT flip `changed_since_emit`, so the kernel is marked dirty explicitly —
/// otherwise the refreshed projection could sit unemitted until an unrelated
/// kernel mutation triggered a tick.
pub(crate) fn bunker_handshake_progress(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    stage: String,
    code: Option<String>,
    message: Option<String>,
) {
    let value = if stage == "idle" {
        None
    } else {
        Some(BunkerHandshakeDto::new(stage, code, message))
    };
    identity.set_bunker_handshake(value);
    kernel.mark_changed_since_emit();
}

/// Shape-validate a `bunker://` URI, seed the `bunker_handshake` projection
/// with `"connecting"`, and delegate the handshake to the registered broker.
///
/// Called by [`add_signer`]'s [`crate::actor::SignerSource::BunkerUri`] arm
/// (which has already stashed `make_active` in `pending_bunker_make_active`).
pub(super) fn start_bunker_handshake(identity: &IdentityRuntime, kernel: &mut Kernel, uri: &str) {
    // Stage 3 of NIP-46 wiring: actor exposes handshake-progress snapshot.
    // Stage 4 of NIP-46 wiring: actor delegates the handshake to the broker
    // hook installed in this app's per-app `bunker_hook` slot (ADR-0052 §D3 —
    // installed by `nmp_signer_broker_init`; no process-global).
    //
    // Here we shape-validate the URI, seed the snapshot with `"connecting"`
    // so the host sign-in flow renders progress immediately, then hand
    // the URI to the registered broker. The broker drives the connect /
    // get_public_key dance on its own thread and reports progress via
    // `BunkerHandshakeProgress` + `AddSigner { RemoteHandle, .. }`. D0 stays
    // clean: `nmp-core` imports neither the broker crate nor `nmp-signers`.
    if parse_bunker_remote(uri).is_none() {
        kernel.set_last_error_token(&crate::ui_token::UiToken::error(
            crate::ui_token::codes::SIGNER_BUNKER_INVALID_URI,
            "invalid bunker:// URI — expected bunker://<64-hex-pubkey>?relay=…",
        ));
        return;
    }
    identity.set_bunker_handshake(Some(BunkerHandshakeDto::progress(
        "connecting",
        &crate::ui_token::UiToken::progress(
            crate::ui_token::codes::PROGRESS_WAITING_FOR_BROKER,
            "Waiting for broker...",
        ),
    )));
    kernel.mark_changed_since_emit();
    if !identity.invoke_bunker_connect_hook(uri) {
        // Defence against init-order bugs: the broker should be registered
        // before any URI can reach the actor. If it isn't, surface a clear
        // toast and clear the progress projection (D6 — error becomes state,
        // never panic across FFI).
        identity.set_bunker_handshake(None);
        kernel.set_last_error_token(&crate::ui_token::UiToken::error(
            crate::ui_token::codes::SIGNER_BROKER_NOT_INITIALISED,
            "NIP-46 broker not initialised — call nmp_signer_broker_init",
        ));
    }
}

pub(crate) fn restore_bunker_session(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    payload_json: &str,
) {
    identity.set_bunker_handshake(Some(BunkerHandshakeDto::progress(
        "connecting",
        &crate::ui_token::UiToken::progress(
            crate::ui_token::codes::PROGRESS_RESTORING_BROKER_SESSION,
            "Restoring broker session...",
        ),
    )));
    kernel.mark_changed_since_emit();
    if !identity.invoke_bunker_restore_hook(payload_json) {
        identity.set_bunker_handshake(None);
        kernel.set_last_error_token(&crate::ui_token::UiToken::error(
            crate::ui_token::codes::SIGNER_BROKER_NOT_INITIALISED,
            "NIP-46 broker not initialised — call nmp_signer_broker_init",
        ));
    }
}

/// ADR-0048 D4 — restore a persisted NIP-55 account on cold start.
///
/// Unlike the bunker restore there is no handshake: the payload is
/// pubkey-only, so the registered driver hook synchronously reconstructs
/// the `Nip55Signer` and enqueues `AddSigner { RemoteHandle, .. }` back to
/// the actor. A missing hook degrades to a toast (D6) — defence against
/// init-order bugs, exactly like the bunker path.
pub(crate) fn restore_nip55_session(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    payload_json: &str,
) {
    if !identity.invoke_external_signer_restore_hook(payload_json) {
        identity.set_signer_state(Some(SignerStateDto::new(
            "nip55".to_string(),
            "unavailable".to_string(),
            Some("NIP-55 driver not initialised".to_string()),
        )));
        kernel.set_last_error_token(&crate::ui_token::UiToken::error(
            crate::ui_token::codes::SIGNER_NIP55_DRIVER_NOT_INITIALISED,
            "NIP-55 driver not initialised — call nmp_external_signer_init",
        ));
        kernel.mark_changed_since_emit();
    }
}

/// Minimal `bunker://<remote-pubkey-hex>?relay=…` shape check. Returns the
/// remote pubkey hex if the URI is well-formed.
fn parse_bunker_remote(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("bunker://")?;
    let pubkey = rest.split(['?', '/']).next()?;
    if pubkey.len() == 64 && pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(pubkey.to_string())
    } else {
        None
    }
}
