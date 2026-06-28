//! Native runtime owner for NMP applications.
//!
//! This crate owns the Rust-native app runtime: `NmpApp`, actor lifecycle,
//! runtime slots, native registration APIs, feed/search/group session
//! orchestration, and the typestate builder. C ABI crates wrap this surface;
//! they do not own runtime state.

mod app_config_hooks;
mod app_config_intent;
mod app_config_search;
mod app_config_substrate;
mod app_ctor;
mod app_host_impl;
mod app_impl_accessors;
mod app_impl_core;
mod app_impl_feeds;
mod app_struct;
mod app_sub_structs;
mod capability;
mod debug_info;
mod declared_projections;
#[cfg(feature = "external-signer")]
mod external_signer;
mod feed;
mod feed_session;
mod group_feed;
mod incremental_apply;
mod intent;
mod keyring_forget;
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
pub mod op_feed_defaults;
pub mod op_pointer_source;

pub(crate) mod runtimes {
    pub(crate) mod active_observed_projection;
}

pub use app_ctor::new_app;
pub use app_struct::{IdentityChangeObserverId, NmpApp, UpdateListener};
pub use builder::{
    NmpAppBuilder, ProjectionsDeclared, RelaysDeclared, RunConfig, StorageSet, Unstarted,
};
pub use debug_info::{empty_debug_info_json, DOMAIN_COMPOSITION, DOMAIN_MERGED, DOMAIN_ROUTING};
pub use feed::{
    decode_and_validate_feed_params, validate_feed_params, FeedAdmission, FeedHandle, FeedParams,
    FeedParamsDecodeError, FeedParamsError, FeedRanking, FeedRender, FeedScope, FeedSessionId,
    FeedWindow, ProjectionKey, PubkeySetExpr,
};
pub use feed_session::{handle_projection_key, FeedCompiler, FeedOpenError, FeedTeardown};
pub use group_feed::{GroupFeedToken, DISCOVERED_GROUPS_KEY, GROUP_EVENTS_KEY, JOINED_GROUPS_KEY};
pub use intent::InputIntentDispatch;
pub use nmp_core::substrate::ObservedProjectionCommandHandle;
pub use nmp_feed::FeedSessionBuild;
pub use nmp_nip50::SearchRequest;
pub use observed_projection_handle::ObservedProjectionHandle;
pub use op_feed_defaults::{
    compile_feed_params, register_op_feed_defaults, register_op_feed_defaults_with_mute,
    OpFeedDefaults,
};
pub use prestart_config::NmpConfigStatus;
pub use search::parse_search_request;
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
