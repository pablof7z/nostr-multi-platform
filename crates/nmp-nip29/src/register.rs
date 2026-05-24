//! Host-app wiring helpers for NIP-29.
//!
//! These functions do the mechanical plumbing that every host app (Chirp,
//! a TUI, a test harness) would otherwise have to repeat verbatim:
//! registering `GroupChatProjection` as both a `KernelEventObserver`
//! (ingest) and a snapshot projection (output), and binding the typed
//! NIP-29 `ActionModule` impls against the action registry.
//!
//! Chirp's `extern "C"` wrappers (`nmp_app_chirp_register_group_chat`, etc.)
//! are thin delegates — they parse C strings and call these typed functions.
//! Any other host can call these directly in Rust.
//!
//! ## D0 compliance
//!
//! These helpers depend on `nmp-core` only through its public extension seams
//! (`register_event_observer`, `register_snapshot_projection`,
//! `register_action`, `swap_singleton_event_observer`). `nmp-core` gains zero
//! NIP-29 nouns; composition happens here, in the NIP-29 crate.

use std::sync::Arc;

use nmp_core::KernelEventObserver;
use nmp_ffi::NmpApp;

use crate::action::{
    DiscoverGroupsAction, JoinGroupAction, PostChatMessageAction, ReactInGroupAction,
};
use crate::group_id::GroupId;
use crate::projection::{DiscoveredGroupsProjection, GroupChatProjection};

/// Wire a [`GroupChatProjection`] for `group_id` into `app`.
///
/// Registers the projection as a [`KernelEventObserver`] (ingest) and
/// exposes its `snapshot_json` read under `"nmp.nip29.group_chat"` as a
/// snapshot projection (output). Chat messages for that group then appear
/// on every kernel snapshot tick under that key.
///
/// Re-invocation is idempotent: a second call unregisters the prior
/// projection before registering the new one via the per-app
/// `swap_singleton_event_observer` slot. There is no event-loss gap — the
/// new observer is live before the old one is dropped.
///
/// The `app` must outlive the registration; this function only borrows it
/// for the duration of the call. The projection itself is owned by the
/// kernel's observer list.
pub fn wire_group_chat(app: &NmpApp, group_id: GroupId) {
    let projection = Arc::new(GroupChatProjection::new(group_id));
    let observer_id =
        app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        // Observer slot poisoned — skip snapshot registration too so we
        // don't bind a closure for a projection that will never receive
        // events, and don't disturb any prior observer.
        return;
    }

    // Idempotent re-invoke: atomically install the new id and take the
    // prior id out of the per-app slot, then unregister the prior observer.
    // Swap-then-unregister order is intentional: the new observer is already
    // live when the old one is dropped, so there is no event-loss gap.
    if let Some(prev) = app.swap_singleton_event_observer(Some(observer_id)) {
        app.unregister_event_observer(prev);
    }

    app.register_snapshot_projection("nmp.nip29.group_chat", move || projection.snapshot_json());
}

/// Wire a [`DiscoveredGroupsProjection`] for `relay_url` into `app`.
///
/// Registers the projection as a [`KernelEventObserver`] (ingest) and
/// exposes its `snapshot_json` read under `"nmp.nip29.discovered_groups"`.
/// Kind:39000/39001/39002 events for that relay then appear on every
/// snapshot tick under that key.
///
/// An empty `relay_url` is a silent no-op. `app` must outlive the
/// registration.
pub fn wire_group_discovery(app: &NmpApp, relay_url: String) {
    if relay_url.is_empty() {
        return;
    }
    let projection = Arc::new(DiscoveredGroupsProjection::new(relay_url));
    let observer_id =
        app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        return;
    }
    app.register_snapshot_projection("nmp.nip29.discovered_groups", move || {
        projection.snapshot_json()
    });
}

/// Register the NIP-29 action namespaces against `app`'s action registry.
///
/// Binds the typed [`ActionModule`] impls for:
/// - `nmp.nip29.post_chat_message`
/// - `nmp.nip29.react_in_group`
/// - `nmp.nip29.discover`
/// - `nmp.nip29.join`
///
/// Must be called before `nmp_app_start` — the registry is write-locked
/// after the actor loop starts. Requires `&mut NmpApp` because registration
/// writes into the app's shared action registry.
pub fn register_actions(app: &mut NmpApp) {
    app.register_action::<PostChatMessageAction>();
    app.register_action::<ReactInGroupAction>();
    app.register_action::<DiscoverGroupsAction>();
    app.register_action::<JoinGroupAction>();
}
