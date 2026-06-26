//! Host-app wiring helpers for NIP-29.
//!
//! These functions do the mechanical plumbing that every host app (Chirp,
//! a TUI, a test harness) would otherwise have to repeat verbatim: binding the
//! typed NIP-29 `ActionModule` impls against the action registry, and wiring
//! the output-only group-create defaults projection.
//!
//! ## Where the per-open read views live now (#2088)
//!
//! The four per-open read views — group chat, discovered groups, joined groups,
//! and the (deleted) raw group-events collector — used to be wired here via
//! an ambient all-event observer. That made a view opened AFTER its events
//! were already cached hydrate live-only: it missed the cached tail
//! (#2088). The hydrating composition now lives in `nmp-ffi`
//! (`crate::group_feed`), which registers the projection MUTED and routes its
//! ingest through `NmpApp::open_observed_interest_pinned` (the ADR-0062
//! read-cache replay door) so a late-opened view catches up. That composition
//! must name `NmpApp`, which is the FFI host type `nmp-nip29` may not name
//! (D0), so it cannot live in this crate. `nmp-nip29` contributes only the
//! NmpApp-free filter-builders (`GroupId::chat_filter_json`,
//! `group_metadata_filter_json`) the composer feeds to the open door.
//!
//! ## D0 compliance
//!
//! These helpers depend on `nmp-core` only through its public extension seams
//! (`register_action`, `register_typed_snapshot_projection`). `nmp-core` gains
//! zero NIP-29 nouns; composition happens here (defaults) and in `nmp-ffi`
//! (the hydrating read views), never in the kernel.

use nmp_core::substrate::{ActionRegistrar, RegistrationError, SnapshotProjectionRegistrar};

use crate::action::{
    CreateInviteAction, CreatePublicGroupAction, DiscoverGroupsAction, JoinGroupAction,
    LeaveGroupAction, PublishGroupEventAction, PutUserAction, ReactInGroupAction,
    RepostInGroupAction, SetParentAction, ShareEventInGroupAction,
};
use crate::projection::{GroupDefaultsProjection, GroupDefaultsSnapshot};

/// Wire an empty NIP-29 group-create defaults projection into `app`.
///
/// Exposes [`GroupDefaultsProjection::snapshot`] under
/// `"nmp.nip29.group_defaults"` as a typed FlatBuffers snapshot projection
/// (`NGDF`). No generic `Value` projection is registered for this key. The
/// no-arg path emits an empty `suggested_relay_url`: shared crates do not own
/// public relay/operator policy.
///
/// Output-only: this projection observes no kernel events — its snapshot is a
/// pure function of the registration-time config — so no `ObservedProjectionSink`
/// is registered. `app` must outlive the registration.
pub fn wire_group_defaults(app: &impl SnapshotProjectionRegistrar) {
    wire_group_defaults_with_snapshot(app, GroupDefaultsSnapshot::from_defaults());
}

/// Wire NIP-29 group-create defaults with an app/operator-supplied relay URL.
///
/// This is the app-owned policy path for clients such as Chirp: NIP-29 owns the
/// projection shape and typed transport, while the leaf app owns the concrete
/// public relay URL it wants to pre-fill.
pub fn wire_group_defaults_with_relay(
    app: &impl SnapshotProjectionRegistrar,
    suggested_relay_url: impl Into<String>,
) {
    wire_group_defaults_with_snapshot(
        app,
        GroupDefaultsSnapshot::with_suggested_relay_url(suggested_relay_url),
    );
}

/// Wire NIP-29 group-create defaults with an explicit snapshot.
pub fn wire_group_defaults_with_snapshot(
    app: &impl SnapshotProjectionRegistrar,
    snapshot: GroupDefaultsSnapshot,
) {
    let projection = GroupDefaultsProjection::with_snapshot(snapshot);
    // Typed FlatBuffers snapshot projection (`NGDF`). Unlike older
    // typed+generic registrations in this module, group defaults has no generic
    // `Value` projection under the same key; hosts read this typed payload or
    // treat the defaults as absent.
    app.register_typed_snapshot_projection("nmp.nip29.group_defaults", move || {
        let snapshot = projection.snapshot();
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
/// Binds the typed [`ActionModule`](nmp_core::substrate::ActionModule) impls for:
/// - `nmp.nip29.publish_group_event`
/// - `nmp.nip29.react_in_group`
/// - `nmp.nip29.share_event_in_group`
/// - `nmp.nip29.repost_in_group`
/// - `nmp.nip29.create_public_group`
/// - `nmp.nip29.discover`
/// - `nmp.nip29.join`
/// - `nmp.nip29.leave`
/// - `nmp.nip29.put_user`
/// - `nmp.nip29.create_invite`
/// - `nmp.nip29.set_parent` (NIP-29 subgroups, nips PR #2319)
///
/// Must be called before `nmp_app_start` — the registry is write-locked
/// after the actor loop starts.
///
/// Takes `&mut impl ActionRegistrar` rather than `&mut NmpApp` (D0 fix,
/// #1724): a pure-protocol crate must not name the FFI host type. The
/// concrete `NmpApp` implements `ActionRegistrar`; the caller upcasts it
/// via the trait, keeping this crate NIP-layer-only (D0 §3).
///
/// Group-publishing actions read recent group events for `["previous", …]` tags
/// through the execution [`ActionContext`](nmp_core::substrate::ActionContext)
/// at dispatch time. That keeps the local-store read dependency in the action
/// lifecycle instead of per-action constructor plumbing, while preserving the
/// cache-only `StoreQuery::Tags { #h, limit }` single source of truth.
///
/// Returns `Err(`[`RegistrationError`]`)` on the FIRST namespace collision
/// detected (#1724 criterion 1: structured error in both dev and release).
/// A collision means two init calls for the same app — the caller's bug.
pub fn register_actions(app: &mut impl ActionRegistrar) -> Result<(), RegistrationError> {
    app.register_action(PublishGroupEventAction)?;
    app.register_action(ReactInGroupAction)?;
    app.register_action(ShareEventInGroupAction)?;
    app.register_action(RepostInGroupAction)?;
    app.register_action(CreatePublicGroupAction)?;
    app.register_action(DiscoverGroupsAction)?;
    app.register_action(JoinGroupAction)?;
    app.register_action(LeaveGroupAction)?;
    app.register_action(PutUserAction)?;
    app.register_action(CreateInviteAction)?;
    app.register_action(SetParentAction)?;
    Ok(())
}
