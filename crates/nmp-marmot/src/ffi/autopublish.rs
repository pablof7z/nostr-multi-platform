//! PR-4 — shared key-package autopublish tail for the register entry points.
//!
//! Every local-key sign-in path (`nmp_app_signin_nsec`,
//! `nmp-marmot::identity::sign_in_nsec_with_keyring_account`,
//! `nmp-marmot::identity::restore_identity_with_keyring_account`,
//! `nmp_app_create_new_account`) sets `NmpApp::pending_mls_autopublish` via
//! `NmpApp::add_signer`. Consuming it HERE — in the tail shared by both
//! `register_with_secret_hex` and `nmp_marmot_register_active` — makes every
//! account MLS-capable on register without extra host plumbing.
//!
//! Idempotence: `take_pending_mls_autopublish` is a one-shot atomic swap, so a
//! re-register (account switch back) of an already-published account does NOT
//! republish.

use nmp_ffi::NmpApp;

use super::MarmotHandle;
use crate::projection::action::{MarmotAction, MarmotProtocolCommand};

/// If the sign-in path armed the autopublish flag, consume it and publish a
/// key package against the freshly-registered handle. A no-op when the flag is
/// clear. Synchronous (drives `ops::dispatch` via `with_inner`), so the publish
/// is attempted before the register call returns.
pub(super) fn maybe_autopublish_on_register(app_ref: &NmpApp, handle: *mut MarmotHandle) {
    if app_ref.take_pending_mls_autopublish() {
        publish_key_package_on_register(handle);
    }
}

fn publish_key_package_on_register(handle: *mut MarmotHandle) {
    let Some(handle) = (unsafe { handle.as_ref() }) else {
        return;
    };
    let action = MarmotAction::PublishKeyPackage { relays: Vec::new() };
    let cmd = nmp_core::actor::ActorCommand::Protocol(Box::new(
        MarmotProtocolCommand::new_internal(std::sync::Arc::clone(&handle.projection), action),
    ));
    let _ = handle.projection.with_inner(|h| h.send_actor_command(cmd));
}
