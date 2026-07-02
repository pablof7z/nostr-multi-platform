//! Native runtime owner for NMP applications.
//!
//! This crate owns the Rust-native app runtime: `NmpApp`, actor lifecycle,
//! runtime slots, native registration APIs, feed/search/group session
//! orchestration, and the typestate builder. C ABI crates wrap this surface;
//! they do not own runtime state.

pub mod action_dispatch;
mod app_config_hooks;
mod app_config_intent;
mod app_config_search;
mod app_config_substrate;
mod app_ctor;
mod app_host_impl;
mod app_impl_accessors;
mod app_impl_core;
mod app_impl_feeds;
pub mod app_mirror;
mod app_struct;
mod app_sub_structs;
mod capability;
mod debug_info;
mod declared_projections;
#[cfg(feature = "external-signer")]
mod external_signer;
mod feed;
mod feed_facade;
mod feed_session;
mod feed_session_host;
mod group_feed;
mod incremental_apply;
mod intent;
mod keyring_forget;
#[cfg(feature = "marmot")]
mod marmot;
mod observed_feed_source;
mod observed_projection_handle;
mod passive_start;
mod prestart_config;
mod relay_config;
mod search;
#[cfg(feature = "signer-broker")]
mod signer_broker;
mod signer_ports;
#[cfg(any(test, feature = "test-support"))]
mod signer_ports_test_support;
mod snapshot;
mod storage;
#[cfg(any(test, feature = "test-support"))]
mod testing;

pub mod builder;
pub mod op_feed_session;
#[cfg(test)]
pub(crate) mod op_pointer_source;

pub use action_dispatch::{dispatch_action_bytes_typed, DispatchOutcome};
pub use app_ctor::new_app;
pub use app_struct::{IdentityChangeObserverId, NmpApp, UpdateListener};
pub use builder::{
    NmpAppBuilder, ProjectionsDeclared, RelaysDeclared, RunConfig, StorageSet, Unstarted,
};
pub use debug_info::{empty_debug_info_json, DOMAIN_COMPOSITION, DOMAIN_MERGED, DOMAIN_ROUTING};
pub use feed::{
    decode_and_validate_feed_params, validate_feed_params, CustomAdmissionDef, CustomAdmissionId,
    CustomOrderDef, CustomOrderId, CustomSourceDef, CustomSourceId, FeedAdmission, FeedHandle,
    FeedItemProjection, FeedKey, FeedOrder, FeedParams, FeedParamsDecodeError, FeedScope,
    FeedSessionId, FeedShape, FeedSourceExpr, FeedSpec, FeedSpecError, FeedWindowPolicy,
    ProjectionKey,
};
pub use feed_facade::{FeedSessions, FeedSpecOpenError};
pub use feed_session::{handle_projection_key, FeedOpenError};
pub use group_feed::{
    Nip25GroupReactionsHandle, Nip25GroupReactionsSession, Nip29GroupDiscoveryHandle,
    Nip29GroupDiscoverySession, Nip29GroupEventsHandle, Nip29GroupEventsSession,
    Nip29GroupRosterHandle, Nip29GroupRosterSession, Nip29JoinedGroupsHandle,
    Nip29JoinedGroupsSession, DISCOVERED_GROUPS_KEY, GROUP_EVENTS_KEY, GROUP_REACTIONS_KEY,
    GROUP_ROSTER_KEY, JOINED_GROUPS_KEY,
};
pub use intent::InputIntentDispatch;
pub use nmp_core::__ffi_internal::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
pub use nmp_nip18::PrimaryKindError;
pub use nmp_nip50::SearchRequest;
pub use op_feed_session::{
    active_follows_op_feed_params, open_active_follows_op_feed,
    open_active_follows_op_feed_with_mute, ActiveFollowsOpFeedSession,
};
pub use prestart_config::NmpConfigStatus;
pub use search::{parse_search_request, Nip50SearchHandle, Nip50SearchSession};
#[cfg(any(test, feature = "test-support"))]
pub use signer_ports_test_support::{
    install_bunker_hook_for_test, install_external_signer_hook_for_test,
    invoke_bunker_connect_hook_for_test, invoke_external_signer_restore_hook_for_test,
};

#[must_use]
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() {
        None
    } else {
        Some(unsafe { &*app })
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
