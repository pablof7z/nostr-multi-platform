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
    CreateInviteAction, CreatePublicGroupAction, DiscoverGroupsAction, JoinGroupAction,
    PostChatMessageAction, PutUserAction, ReactInGroupAction,
};
use crate::group_id::GroupId;
use crate::projection::{
    DiscoveredGroupsProjection, GroupChatProjection, GroupDefaultsProjection,
    JoinedGroupsProjection,
};

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

    // Typed FlatBuffers sidecar (ADR-0037), registered ALONGSIDE the generic
    // `Value` projection under the same key. A `NGCS`-aware host prefers this
    // typed payload; an un-updated host falls back to the generic `Value`
    // subtree (the permanent fallback). Additive — un-updated hosts unaffected.
    // Clone the `Arc` first: the generic closure below consumes `projection`,
    // so the typed closure needs its own handle.
    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection("nmp.nip29.group_chat", move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip29.group_chat".to_string(),
            schema_id: crate::wire::group_chat_fb::GROUP_CHAT_SCHEMA_ID.to_string(),
            schema_version: crate::wire::group_chat_fb::GROUP_CHAT_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                crate::wire::group_chat_fb::GROUP_CHAT_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: crate::wire::group_chat_fb::encode_group_chat_snapshot(&snapshot),
            ..Default::default()
        })
    });
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
    // Typed FlatBuffers sidecar (ADR-0037), registered ALONGSIDE the generic
    // `Value` projection under the same key. Clone the `Arc` first: the generic
    // closure below consumes `projection`.
    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection("nmp.nip29.discovered_groups", move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip29.discovered_groups".to_string(),
            schema_id: crate::wire::discovered_groups_fb::DISCOVERED_GROUPS_SCHEMA_ID.to_string(),
            schema_version: crate::wire::discovered_groups_fb::DISCOVERED_GROUPS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                crate::wire::discovered_groups_fb::DISCOVERED_GROUPS_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: crate::wire::discovered_groups_fb::encode_discovered_groups_snapshot(
                &snapshot,
            ),
            ..Default::default()
        })
    });
}

/// Wire the active account's joined-groups projection into `app`.
///
/// If `host_relay_url` is non-empty, the projection is scoped to that host
/// relay. If it is empty, the projection derives host identity from
/// `KernelEvent.relay_provenance` and ignores events that carry no provenance.
/// The read model is exposed under `"nmp.nip29.joined_groups"` as a typed
/// FlatBuffers sidecar (`NJGS`).
pub fn wire_joined_groups(app: &NmpApp, active_pubkey: String, host_relay_url: String) {
    if active_pubkey.is_empty() {
        return;
    }
    let projection = if host_relay_url.is_empty() {
        Arc::new(JoinedGroupsProjection::new(active_pubkey))
    } else {
        Arc::new(JoinedGroupsProjection::new_for_host(
            active_pubkey,
            host_relay_url,
        ))
    };
    let observer_id =
        app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        return;
    }

    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection("nmp.nip29.joined_groups", move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip29.joined_groups".to_string(),
            schema_id: crate::wire::joined_groups_fb::JOINED_GROUPS_SCHEMA_ID.to_string(),
            schema_version: crate::wire::joined_groups_fb::JOINED_GROUPS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                crate::wire::joined_groups_fb::JOINED_GROUPS_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: crate::wire::joined_groups_fb::encode_joined_groups_snapshot(&snapshot),
            ..Default::default()
        })
    });
}

/// Wire the crate-owned NIP-29 group-create defaults projection into `app`.
///
/// Exposes [`GroupDefaultsProjection::snapshot`] under
/// `"nmp.nip29.group_defaults"` as both a typed FlatBuffers sidecar
/// (`NGDF`, ADR-0037) and the generic `Value` projection (the permanent
/// fallback). The projection carries the suggested public-group relay URL
/// (issue #626) sourced from the crate-owned
/// [`crate::projection::DEFAULT_PUBLIC_GROUP_RELAY_URL`] constant, so every host
/// shell reads the same default off the kernel snapshot instead of hardcoding
/// it in the shell.
///
/// Output-only: unlike [`wire_group_chat`] / [`wire_group_discovery`] this
/// projection observes no kernel events — its snapshot is a pure function of
/// the crate constant — so no [`KernelEventObserver`] is registered. `app` must
/// outlive the registration.
pub fn wire_group_defaults(app: &NmpApp) {
    // Typed FlatBuffers sidecar (ADR-0037), registered ALONGSIDE the generic
    // `Value` projection under the same key. A `NGDF`-aware host prefers this
    // typed payload; an un-updated host falls back to the generic `Value`
    // subtree (the permanent fallback). Additive — un-updated hosts unaffected.
    app.register_typed_snapshot_projection("nmp.nip29.group_defaults", || {
        let snapshot = GroupDefaultsProjection::new().snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip29.group_defaults".to_string(),
            schema_id: crate::wire::group_defaults_fb::GROUP_DEFAULTS_SCHEMA_ID.to_string(),
            schema_version: crate::wire::group_defaults_fb::GROUP_DEFAULTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                crate::wire::group_defaults_fb::GROUP_DEFAULTS_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: crate::wire::group_defaults_fb::encode_group_defaults_snapshot(&snapshot),
            ..Default::default()
        })
    });
}

/// Register the NIP-29 action namespaces against `app`'s action registry.
///
/// Binds the typed [`ActionModule`] impls for:
/// - `nmp.nip29.post_chat_message`
/// - `nmp.nip29.react_in_group`
/// - `nmp.nip29.create_public_group`
/// - `nmp.nip29.discover`
/// - `nmp.nip29.join`
/// - `nmp.nip29.put_user`
/// - `nmp.nip29.create_invite`
///
/// Must be called before `nmp_app_start` — the registry is write-locked
/// after the actor loop starts. Requires `&mut NmpApp` because registration
/// writes into the app's shared action registry.
pub fn register_actions(app: &mut NmpApp) {
    app.register_action(PostChatMessageAction);
    app.register_action(ReactInGroupAction);
    app.register_action(CreatePublicGroupAction);
    app.register_action(DiscoverGroupsAction);
    app.register_action(JoinGroupAction);
    app.register_action(PutUserAction);
    app.register_action(CreateInviteAction);
}
