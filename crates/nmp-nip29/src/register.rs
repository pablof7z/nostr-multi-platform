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

use nmp_core::substrate::{
    ActionRegistrar, EventObserverRegistrar, RegistrationError, SnapshotProjectionRegistrar,
};
use nmp_core::{KernelEventObserver, KernelEventObserverId};

/// Opaque handle for a single group-discovery session.
///
/// Created by [`open_group_discovery`] and torn down by
/// [`close_group_discovery`]. The handle holds a borrow-free reference to the
/// kernel via the `snapshot_projections` key string and the registered
/// observer id; on close both are revoked cleanly so there is no bounded
/// event-observer leak between relay switches or screen dismissals.
pub struct GroupDiscoveryHandle {
    /// The observer registered with the kernel at open time.
    observer_id: KernelEventObserverId,
    /// Teardown function: unregisters the observer and removes the typed
    /// snapshot projection, restoring the app to the state before
    /// `open_group_discovery` was called.
    ///
    /// Stored as a `Box<dyn FnOnce(KernelEventObserverId)>` so the concrete
    /// app type does not leak through the handle's generic parameters — the
    /// caller captures the concrete `app` pointer at open time and the handle
    /// remains type-erased for storage.
    ///
    /// SAFETY: the concrete app type `A` used in `open_group_discovery` must
    /// outlive this handle (the same contract as before, when the handle stored
    /// `*const NmpApp`).
    teardown_fn: Box<dyn FnOnce(KernelEventObserverId) + Send>,
}

// SAFETY: `GroupDiscoveryHandle` is Send because `NmpApp` (the concrete `A`
// captured at open time) is `Sync + Send`, and we only access it through
// shared-ref methods (`remove_snapshot_projection`, `unregister_event_observer`)
// which are internally lock-guarded.
unsafe impl Send for GroupDiscoveryHandle {}
// SAFETY: `teardown_fn` is `FnOnce + Send`; the `GroupDiscoveryHandle` is
// accessed single-threadedly (the caller drives open and close).
unsafe impl Sync for GroupDiscoveryHandle {}

use crate::action::{
    CreateInviteAction, CreatePublicGroupAction, DiscoverGroupsAction, JoinGroupAction,
    LeaveGroupAction, PostChatMessageAction, PutUserAction, ReactInGroupAction,
    RepostInGroupAction, SetParentAction, ShareEventInGroupAction,
};
use crate::group_id::GroupId;
use crate::projection::{
    DiscoveredGroupsProjection, GroupChatProjection, GroupDefaultsProjection,
    GroupDefaultsSnapshot, GroupEventsProjection, JoinedGroupsProjection,
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
pub fn wire_group_chat(
    app: &(impl EventObserverRegistrar + SnapshotProjectionRegistrar),
    group_id: GroupId,
) {
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

/// Open a group-discovery session for `relay_url` and wire the read
/// projection into `app`.
///
/// Registers the [`DiscoveredGroupsProjection`] as a [`KernelEventObserver`]
/// (ingest) and exposes its snapshot read under
/// `"nmp.nip29.discovered_groups"`. Kind:39000/39001/39002 events for that
/// relay then appear on every snapshot tick under that key.
///
/// Returns a [`GroupDiscoveryHandle`] the caller MUST pass to
/// [`close_group_discovery`] to tear down the observer and remove the
/// projection when the session ends (e.g. the discover screen is dismissed or
/// the user switches to a different relay). Returns `None` on an empty URL,
/// or a poisoned observer slot.
///
/// `app` must outlive the returned handle.
pub fn open_group_discovery<A>(app: &A, relay_url: String) -> Option<GroupDiscoveryHandle>
where
    A: EventObserverRegistrar + SnapshotProjectionRegistrar + Send + Sync,
{
    if relay_url.is_empty() {
        return None;
    }
    let projection = Arc::new(DiscoveredGroupsProjection::new(relay_url));
    let observer_id =
        app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        return None;
    }
    // Typed FlatBuffers sidecar (ADR-0037), registered ALONGSIDE the generic
    // `Value` projection under the same key. Clone the `Arc` first so the
    // typed closure gets its own handle while the observer holds another.
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

    // Capture the concrete `app` as a raw pointer for the teardown closure.
    // SAFETY: `app` must outlive the returned handle (the caller's contract).
    //
    // We store the raw pointer as a `usize` (the address) so the closure can
    // be `Send + 'static` without the Rust 2021 precise-capture analysis
    // flagging the `*const A` field of a Send newtype.  The `usize` is Send
    // by definition; we reconstruct the pointer at teardown time.
    let app_addr: usize = (app as *const A) as usize;
    Some(GroupDiscoveryHandle {
        observer_id,
        teardown_fn: Box::new(move |id| {
            // SAFETY: `app` is valid for the lifetime of the handle (caller
            // contract — same as when the handle stored `*const NmpApp`).
            let app = unsafe { &*(app_addr as *const A) };
            app.unregister_event_observer(id);
            app.remove_snapshot_projection("nmp.nip29.discovered_groups");
        }),
    })
}

/// Tear down the group-discovery session opened by [`open_group_discovery`].
///
/// Unregisters the event observer and removes the
/// `"nmp.nip29.discovered_groups"` typed snapshot projection so no stale
/// subtree is emitted after the session ends. Idempotent on the app
/// internals (unknown observer ids / projection keys are silent no-ops).
/// The `handle` itself is consumed and must not be used after this call.
pub fn close_group_discovery(handle: GroupDiscoveryHandle) {
    (handle.teardown_fn)(handle.observer_id);
}

/// Wire the active account's joined-groups projection into `app`.
///
/// If `host_relay_url` is non-empty, the projection is scoped to that host
/// relay. If it is empty, the projection derives host identity from
/// `KernelEvent.relay_provenance` and ignores events that carry no provenance.
/// The read model is exposed under `"nmp.nip29.joined_groups"` as a typed
/// FlatBuffers sidecar (`NJGS`).
pub fn wire_joined_groups(
    app: &(impl EventObserverRegistrar + SnapshotProjectionRegistrar),
    active_pubkey: String,
    host_relay_url: String,
) {
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

/// Wire a raw `h`-tagged group-event projection for `group_id` into `app`.
///
/// The read model is exposed under `"nmp.nip29.group_events"` as a typed
/// FlatBuffers sidecar (`NGES`). It preserves raw event fields and the complete
/// tag matrix; consumers own any app-specific joins layered on top.
pub fn wire_group_events(
    app: &(impl EventObserverRegistrar + SnapshotProjectionRegistrar),
    group_id: GroupId,
) {
    let projection = Arc::new(GroupEventsProjection::new(group_id));
    let observer_id =
        app.register_event_observer(Arc::clone(&projection) as Arc<dyn KernelEventObserver>);
    if observer_id.0 == 0 {
        return;
    }

    let projection_typed = Arc::clone(&projection);
    app.register_typed_snapshot_projection("nmp.nip29.group_events", move || {
        let snapshot = projection_typed.snapshot();
        Some(nmp_core::TypedProjectionData {
            key: "nmp.nip29.group_events".to_string(),
            schema_id: crate::wire::group_events_fb::GROUP_EVENTS_SCHEMA_ID.to_string(),
            schema_version: crate::wire::group_events_fb::GROUP_EVENTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(
                crate::wire::group_events_fb::GROUP_EVENTS_FILE_IDENTIFIER,
            )
            .into_owned(),
            payload: crate::wire::group_events_fb::encode_group_events_snapshot(&snapshot),
            ..Default::default()
        })
    });
}

/// Wire an empty NIP-29 group-create defaults projection into `app`.
///
/// Exposes [`GroupDefaultsProjection::snapshot`] under
/// `"nmp.nip29.group_defaults"` as a typed FlatBuffers snapshot projection
/// (`NGDF`). No generic `Value` projection is registered for this key. The
/// no-arg path emits an empty `suggested_relay_url`: shared crates do not own
/// public relay/operator policy.
///
/// Output-only: unlike [`wire_group_chat`] / [`open_group_discovery`] this
/// projection observes no kernel events — its snapshot is a pure function of
/// the registration-time config — so no [`KernelEventObserver`] is registered.
/// `app` must outlive the registration.
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
/// Binds the typed [`ActionModule`] impls for:
/// - `nmp.nip29.post_chat_message`
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
/// Returns `Err(`[`RegistrationError`]`)` on the FIRST namespace collision
/// detected (#1724 criterion 1: structured error in both dev and release).
/// A collision means two init calls for the same app — the caller's bug.
pub fn register_actions(app: &mut impl ActionRegistrar) -> Result<(), RegistrationError> {
    app.register_action(PostChatMessageAction)?;
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
