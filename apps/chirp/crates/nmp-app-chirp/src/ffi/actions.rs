//! Chirp-specific action-registration helper invoked from
//! [`super::register::nmp_app_chirp_register`].
//!
//! `super::register::nmp_app_chirp_register` calls
//! `nmp_defaults::register_defaults` for the canonical NMP action
//! modules (NIP-02 / NIP-17 / NIP-57 / NIP-65) and the production routing
//! substrate; this file owns the **Chirp-specific** registration that the
//! template intentionally does not ship.
//!
//! Today: just NIP-29 (relay-based group chat). A notes-only or DM-only
//! Nostr app on top of NMP would not register these — group chat is not
//! part of the canonical NMP composition.
//!
//! # History
//!
//! Pre-step-10 this file also held `register_chirp_actions` (NIP-02 +
//! NIP-25), `register_nip17_actions`, `register_nip57_actions`, and
//! `register_nip65_actions`. Those wrappers were each a one-line forward
//! to the corresponding NIP crate's `register_actions`; they all moved
//! into `nmp_defaults::register_defaults` so a second NMP-based app
//! inherits them through one call rather than re-copying five lines.
//!
//! The bespoke C-ABI symbols (`nmp_app_react` / `nmp_app_follow` /
//! `nmp_app_unfollow`) had been deleted in a prior cycle; the only door
//! into the social verbs is `nmp_app_dispatch_action` under the
//! `nmp.follow` / `nmp.unfollow` / `nmp.nip25.react` namespaces.

use nmp_ffi::NmpApp;

/// Register the NIP-29 group-chat action namespaces against `app`'s
/// action registry.
///
/// Wires typed `ActionModule` impls from the `nmp-nip29` protocol crate
/// via `NmpApp::register_action::<M>()` — the ADR-0027 single-call path
/// that eliminates the pre-ADR-0027 `register_action_module` +
/// `register_action_executor` split. Any NIP crate's typed `ActionModule`
/// can be reached through the generic `dispatch_action` path without
/// `nmp-core` learning any NIP-29 group nouns (D0).
///
/// Namespaces: `nmp.nip29.publish_group_event`, `nmp.nip29.react_in_group`,
/// `nmp.nip29.create_public_group`, `nmp.nip29.discover`,
/// `nmp.nip29.join`.
///
/// SCOPE: NIP-29 v1 ships public group creation, generic group-event publishing,
/// discovery, and join. The remaining admin / membership executors are
/// deliberately out of scope for this milestone.
pub(super) fn register_nip29_actions(app: &mut NmpApp) {
    // The generic publish action reads recent group events for `["previous", …]`
    // tags via the V-83 store-slot handle, so hand it in at composition (the
    // slot is empty until `nmp_app_start` publishes the store; reads before then
    // degrade to no `previous` tags).
    let store_slot = app.event_store_handle();
    // `NmpApp::register_action` already logs a structured tracing::error! on
    // collision in both dev and release (#1724 criterion 1). The Result is
    // only returned so callers that do NOT log internally (e.g. test spies or
    // WasmRuntime) can surface the error. Here we let the tracing log stand;
    // a collision means double-init, which the tracing error will surface.
    let _ = nmp_nip29::register_actions(app, store_slot);
}
