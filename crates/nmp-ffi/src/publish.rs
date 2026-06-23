//! Publish-lifecycle control plane — `retry` / `cancel` for a publish
//! handle. The one-door-per-capability rule DELETED the bespoke
//! event-producing FFI surface that used to live here
//! (`nmp_app_publish_unsigned_event`, `nmp_app_publish_signed_event`,
//! `nmp_app_publish_signed_event_to`). Their job — generic user / app
//! authored publish — is now served by the single
//! [`crate::ffi::action::nmp_app_dispatch_action`] entrypoint under the
//! `nmp.publish` namespace (per-NIP and host action modules build
//! `PublishAction::*` instead of constructing event JSON themselves).
//!
//! What stays here is the *control plane* for an already-queued publish:
//! retry and cancel address a publish handle, they do NOT produce events,
//! and they have no equivalent on the `dispatch_action` seam (by design —
//! the action seam is for content actions, the publish lifecycle is a
//! separate, narrow surface). The D11 lint
//! (`crates/nmp-testing/bin/doctrine-lint/rules/d11.rs`) whitelists these
//! two symbol names explicitly.
//!
//! Symbols in this module:
//!  * `nmp_app_retry_publish`  — control-plane: retry a failed publish handle.
//!  * `nmp_app_cancel_action`  — control-plane: cancel an in-flight operation by
//!    its dispatch `correlation_id` (S7, #1754). Replaced the bespoke
//!    `nmp_app_cancel_publish(handle)` symbol: cancel now addresses the
//!    operation identity (the id the host's spinner is keyed on), and the kernel
//!    reverse-resolves the publish handle from the durable handle↔correlation
//!    index so the `Cancelled` terminal lands under the ORIGINAL correlation_id
//!    (PD-036). One doorway, no parallel cancel lane.
//!
//! These reuse the parent module's validated-argument helpers
//! (`app_ref`, `c_string_argument`) and the shared `NmpApp` handle.
//!
//! ## Theme A discriminator
//!
//! See `crates/nmp-core/src/substrate/action.rs` for the codified rule:
//! generic user/app-authored publish-engine events go through
//! `dispatch_action`; system-authored / lifecycle / wallet capabilities
//! (this module's retry/cancel; Marmot's
//! `NmpApp::publish_signed_explicit` for MLS-credential-signed and
//! ephemeral-signed events the kernel signer cannot mint) stay on bespoke
//! kernel-internal entrypoints. The D11 lint catches accidental regressions
//! where a new `nmp_app_*` FFI body constructs
//! `ActorCommand::PublishSignedEvent` / `PublishUnsignedEvent` directly,
//! re-opening the deleted door.

use super::{app_ref, c_string_argument, NmpApp};
use std::ffi::c_char;

/// Retry a failed publish, addressed by its handle. This is the intentional
/// control-plane door for the publish lifecycle — `dispatch_action` deliberately
/// does NOT carry retry; the generic action seam is for *content* actions, while
/// publish cancel/retry stay on these dedicated symbols.
#[no_mangle]
pub extern "C" fn nmp_app_retry_publish(app: *mut NmpApp, handle: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(handle) = c_string_argument(handle) else {
        return;
    };
    app.retry_publish(handle);
}

/// Cancel an in-flight operation, addressed by its dispatch `correlation_id`
/// (S7, #1754). This is the ONE cancel doorway — the bespoke
/// `nmp_app_cancel_publish(handle)` symbol is deleted. The kernel reverse-
/// resolves the publish handle from the durable handle↔correlation index and
/// records the user-initiated `Cancelled` terminal under the ORIGINAL
/// `correlation_id` (the id the host's spinner is keyed on), fixing PD-036.
///
/// `dispatch_action` deliberately does NOT carry cancel; the generic action
/// seam is for *content* actions, while the publish lifecycle's control plane
/// (cancel / retry) stays on these dedicated symbols. A raw publish handle is
/// also accepted (the index self-maps it) so a caller that only knows the handle
/// still resolves.
#[no_mangle]
pub extern "C" fn nmp_app_cancel_action(app: *mut NmpApp, correlation_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(correlation_id) = c_string_argument(correlation_id) else {
        return;
    };
    app.cancel_publish(correlation_id);
}
