//! Raw C FFI surface for native shells.
//!
//! The Rust runtime owner is `nmp-native-runtime`. This crate owns only C ABI
//! symbols, raw pointer/string conversion, panic-safe callback glue, and
//! Rust-side re-exports of active symbols for app composition crates.

#[cfg(test)]
mod action;
#[cfg(test)]
#[path = "active_account_handle_tests.rs"]
mod active_account_handle_tests;
mod app_ctor;
mod app_lifecycle_ffi;
mod debug_info;
#[cfg(test)]
#[path = "event_by_id_tests.rs"]
mod event_by_id_tests;
#[cfg(feature = "external-signer")]
mod external_signer;
mod free;
mod group_feed;
mod identity;
mod intent_ffi;
#[cfg(test)]
#[path = "interest_feed_tests.rs"]
mod interest_feed_tests;
#[cfg(test)]
#[path = "keyring_forget_tests.rs"]
mod keyring_forget_tests;
#[cfg(test)]
#[path = "passive_start_tests.rs"]
mod passive_start_tests;
pub mod pull;
mod resolve_ref;
#[cfg(test)]
#[path = "resolve_ref_tests.rs"]
mod resolve_ref_tests;
#[cfg(test)]
#[path = "search_tests.rs"]
mod search_tests;
#[cfg(feature = "signer-broker")]
mod signer_broker;
#[cfg(any(test, feature = "test-support"))]
mod signer_ports_test_support;
mod snapshot;
mod storage;
#[cfg(any(test, feature = "test-support"))]
mod testing;
#[cfg(any(test, feature = "test-support"))]
mod testing_stats;
#[cfg(any(test, feature = "test-support"))]
mod testing_sync;

pub use nmp_native_runtime::{
    decode_and_validate_feed_params, handle_projection_key, validate_feed_params, FeedAdmission,
    FeedCompiler, FeedHandle, FeedOpenError, FeedParams, FeedParamsDecodeError, FeedRanking,
    FeedRender, FeedScope, FeedSessionBuild, FeedSessionId, FeedTeardown, FeedWindow,
    IdentityChangeObserverId, Nip29GroupDiscoveryHandle, Nip29GroupDiscoverySession,
    Nip29GroupEventsHandle, Nip29GroupEventsSession, NmpApp, NmpConfigStatus, PrimaryKindError,
    ProjectionKey, PubkeySetExpr,
};

#[cfg(test)]
pub(crate) use app_ctor::test_app_new;
#[cfg(test)]
pub(crate) use app_lifecycle_ffi::{
    test_app_free, test_app_reset, test_app_set_update_callback, test_app_start, TestUpdateCallback,
};

pub use free::nmp_free_string;
#[cfg(feature = "native")]
pub use group_feed::{open_group_discovery_handle, GroupFeedHandle};
#[cfg(feature = "native")]
pub use identity::{
    create_new_account_with_initial_follows, nmp_app_add_relay, nmp_app_create_new_account,
    nmp_app_register_agent_nsec, nmp_app_remove_account, nmp_app_remove_relay,
    nmp_app_signin_bunker, nmp_app_signin_nsec, nmp_app_switch_active,
};
// #1726 — unified diagnostic pull accessor (routing/composition/merged).
// Replaces the deleted `nmp_app_recent_routing_decisions` and
// `nmp_app_composition_report` symbols. No compat shims kept.
#[cfg(feature = "native")]
pub use debug_info::nmp_app_debug_info;
#[cfg(feature = "external-signer")]
pub use external_signer::{
    nmp_app_deliver_external_signer_response, nmp_app_signin_nip55, nmp_external_signer_init,
};
#[cfg(feature = "signer-broker")]
pub use signer_broker::{
    nmp_app_cancel_bunker_handshake, nmp_app_nostrconnect_uri, nmp_signer_broker_init,
};
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use snapshot::{
    nmp_app_consume_all_builtin_projections, nmp_app_declare_consumed_projections,
    nmp_app_declare_incremental_apply,
};
#[cfg(feature = "native")]
pub use storage::nmp_app_set_storage_path;
// `nmp_app_active_following_count` deleted (#1726). See comment near `debug_info` mod.
// #2443: feed/search/URI app-session C exports were deleted after migration to
// the typed UniFFI native session API.
// ADR-0063 Lane D — ref-resolution C-ABI entry points. Hosts should prefer the
// typed adapters; the raw resolve_ref/release_ref surface remains as the
// compatibility boundary for generated or legacy bindings.
#[cfg(feature = "native")]
pub use resolve_ref::{
    nmp_app_release_event_ref, nmp_app_release_profile_ref, nmp_app_release_ref,
    nmp_app_resolve_event_embed, nmp_app_resolve_event_embed_live,
    nmp_app_resolve_event_embed_live_with_metadata, nmp_app_resolve_event_embed_with_metadata,
    nmp_app_resolve_profile_card_live, nmp_app_resolve_profile_ref, nmp_app_resolve_ref,
    nmp_app_resolve_ref_with_metadata,
};

// ── test-support delta ───────────────────────────────────────────────────
#[cfg(any(test, feature = "test-support"))]
pub use signer_ports_test_support::{
    install_bunker_hook_for_test, install_external_signer_hook_for_test,
    invoke_bunker_connect_hook_for_test, invoke_external_signer_restore_hook_for_test,
};
#[cfg(any(test, feature = "test-support"))]
pub use testing::{
    nmp_app_configure_gc_budget, nmp_app_inject_pre_verified_events,
    nmp_app_inject_signed_event_json, nmp_app_inject_signed_events,
    nmp_app_inject_unpinned_events_for_gc, nmp_app_read_author_event_ids,
    nmp_app_read_projection_churn_stats, nmp_app_read_ram_eviction_stats, nmp_app_trigger_gc_step,
};
#[cfg(any(test, feature = "test-support"))]
pub use testing_stats::nmp_app_read_command_lane_stats;
#[cfg(any(test, feature = "test-support"))]
pub use testing_sync::nmp_app_wait_barrier;

// ── Shared FFI helpers ────────────────────────────────────────────────────
use std::ffi::{c_char, CStr};

#[must_use]
pub(crate) fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() {
        None
    } else {
        // SAFETY: caller guarantees non-null app is a valid NmpApp pointer.
        Some(unsafe { &*app })
    }
}

#[must_use]
pub(crate) fn c_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    // Validation: to_str() will reject invalid UTF-8.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

/// Optional-string FFI argument. Unlike `c_string_argument` (which collapses
/// NULL / empty / whitespace to `None` for a REQUIRED arg and the caller
/// drops the call), this returns `Some(value)` for non-empty content and
/// `None` for absent — so a NULL `reply_to_id` means "top-level note" rather
/// than "drop the publish". Build-doc §1.1 contract.
#[must_use]
pub(crate) fn c_optional_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller guarantees ptr is a valid null-terminated C string.
    let value = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}
