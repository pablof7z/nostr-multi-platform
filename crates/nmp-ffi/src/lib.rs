//! Path-A raw C FFI surface. Struct definition, constructor, and `impl`
//! blocks live in the split submodules below; this file carries the
//! lifecycle/argument helpers and the re-export facade.

mod action;
#[cfg(test)]
#[path = "active_account_handle_tests.rs"]
mod active_account_handle_tests;
mod app_config_hooks;
mod app_config_intent;
mod app_config_search;
mod app_config_substrate;
mod app_host_impl;
mod capability;
#[cfg(test)]
#[path = "capability_quiescence_tests.rs"]
mod capability_quiescence_tests;
mod content_ffi;
mod declared_projections;
mod keyring_forget;
#[cfg(test)]
#[path = "passive_start_tests.rs"]
mod passive_start_tests;
#[cfg(test)]
#[path = "update_callback_quiescence_tests.rs"]
mod update_callback_quiescence_tests; // ADR-0053/E4: `impl NmpApp` consumed-projection-intent methods (LOC ceiling).
                                      // `nmp_app_active_following_count` deleted (#1726): follow count is in the
                                      // `nmp.follow_list` typed projection (`follows.len()`). Callers that
                                      // previously read this sync sentinel should read `follows.len()` from the
                                      // `nmp.follow_list` projection instead.
                                      // #1726 — unified diagnostic pull accessor (domain 0=routing, 1=composition, 2=merged).
mod debug_info;

// Canonical cross-cutting string-free symbol. Every `*mut c_char` returned
// by any NMP FFI function must be freed via `nmp_free_string`.
#[cfg(test)]
#[path = "event_by_id_tests.rs"]
mod event_by_id_tests;
mod event_observer;
#[cfg(feature = "external-signer")]
mod external_signer;
mod feed;
mod feed_session;
mod free;
mod identity;
mod incremental_apply;
mod intent_ffi;
#[cfg(test)]
#[path = "interest_feed_tests.rs"]
mod interest_feed_tests;
mod lifecycle;
mod nip19_ffi;
mod nip21_ffi;
mod passive_start;
mod prestart_config;
mod publish;
pub mod pull;
mod relay_config;
#[cfg(test)]
#[path = "sign_event_for_return_tests.rs"]
mod sign_event_for_return_tests;
#[cfg(feature = "signer-broker")]
mod signer_broker;
mod signer_ports;
// #1726: `mod routing_trace` and `mod composition_report` deleted.
// Callers use `nmp_app_debug_info(app, domain)` instead (domain 0 = routing,
// 1 = composition, 2 = merged). No compat shims kept.
// ADR-0063 Lane D — ref-resolution C-ABI symbols. Hosts should prefer the typed
// adapters; the raw resolve_ref/release_ref surface remains as the compatibility
// boundary for generated or legacy bindings.
mod resolve_ref;
#[cfg(test)]
#[path = "resolve_ref_tests.rs"]
mod resolve_ref_tests;
mod search;
mod snapshot;
mod storage;
mod timeline;

#[cfg(any(test, feature = "test-support"))]
mod testing;
#[cfg(any(test, feature = "test-support"))]
mod testing_sync;

#[cfg(any(test, feature = "test-support"))]
mod signer_ports_test_support;

// ── Split submodules ──────────────────────────────────────────────────────
mod app_ctor;
mod app_impl_accessors;
mod app_impl_core;
mod app_impl_feeds;
mod app_lifecycle_ffi;
mod app_struct;
mod app_sub_structs;

// ── Re-exports from split modules ────────────────────────────────────────
pub use app_struct::{IdentityChangeObserverId, NmpApp};
// Make update-callback types accessible via `super::` from inline test
// modules (passive_start_tests, update_callback_quiescence_tests).
pub use app_ctor::nmp_app_new;
pub use app_lifecycle_ffi::{
    nmp_app_configure, nmp_app_free, nmp_app_reset, nmp_app_set_update_callback, nmp_app_start,
    nmp_app_stop,
};
#[cfg(test)]
pub(crate) use app_struct::{UpdateCallback, UpdateCallbackGate, UpdateCallbackRegistration};

// ── Native re-export surface ──────────────────────────────────────────────
// Hoist every per-submodule FFI entry-point into the `ffi::` namespace so
// any native (non-WASM) Rust-side caller — composition-root crates
// (`nmp-defaults`, `nmp-app-*`), out-of-crate integration tests, the
// Android JNI shim — can name them through the rlib without an `extern "C"`
// block. The symbols themselves stay `#[no_mangle] extern "C"` in their
// owning submodules, so the Swift/C ABI is unaffected; the `pub use` only
// affects Rust-side reach.
//
// Gated on `native` (the default feature) so wasm32 (`--no-default-features`)
// continues to compile without these symbols. `android-ffi` implies `native`
// (see [features] in Cargo.toml), so the Android JNI surface inherits this
// block — the small `android-ffi` delta below adds only the four symbols
// that are android-only (account removal, bunker sign-in, full-actor stop,
// active-account switch). Likewise `test-support` implies `native` in
// practice (the `ffi` module itself is `#[cfg(feature = "native")]`), so the
// test-support delta only adds the harness-only injectors / read helpers.
//
// `allow(unused_imports)`: in-crate `tests` modules reach these symbols by
// their `super::` / module path, so the facade re-export is only consumed by
// out-of-crate clients; keeps `cargo test -p nmp-core --lib` clean.
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use action::{
    nmp_app_ack_action_stage, nmp_app_dispatch_action_bytes,
    nmp_app_register_action_result_observer,
};
// Test-support shim: re-export the deleted JSON doorway for integration tests
// in sibling crates that have not yet been migrated to the typed byte path.
// Never compiled into production binaries (only under test-support feature).
#[cfg(feature = "test-support")]
pub use action::nmp_app_dispatch_action;
#[cfg(feature = "native")]
pub use capability::{nmp_app_dispatch_capability, nmp_app_set_capability_callback};
#[cfg(feature = "native")]
pub use content_ffi::nmp_content_tokenize_text;
#[cfg(feature = "native")]
pub use event_observer::{nmp_app_register_event_observer, nmp_app_unregister_event_observer};
#[cfg(feature = "native")]
pub use feed::nmp_app_load_older_feed;
#[cfg(feature = "native")]
pub use feed::{
    decode_and_validate_feed_params, validate_feed_params, FeedAdmission, FeedHandle, FeedParams,
    FeedParamsDecodeError, FeedParamsError, FeedRanking, FeedRender, FeedScope, FeedSessionId,
    FeedWindow, ProjectionKey, PubkeySetExpr,
};
#[cfg(feature = "native")]
pub use feed_session::{
    handle_projection_key, FeedCompileOutput, FeedCompiler, FeedOpenError, FeedTeardown,
};
#[cfg(feature = "native")]
pub use free::nmp_free_string;
#[cfg(feature = "native")]
pub use identity::{
    create_new_account_with_initial_follows, nmp_app_add_relay, nmp_app_create_new_account,
    nmp_app_register_agent_nsec, nmp_app_remove_account, nmp_app_remove_relay,
    nmp_app_signin_bunker, nmp_app_signin_nsec, nmp_app_switch_active,
};
#[cfg(feature = "native")]
#[allow(unused_imports)]
pub use lifecycle::{
    nmp_app_is_alive, nmp_app_lifecycle_background, nmp_app_lifecycle_foreground,
    nmp_app_set_lifecycle_callback,
};
#[cfg(feature = "native")]
pub use nip19_ffi::nmp_app_encode_profile;
#[cfg(feature = "native")]
pub use nip21_ffi::nmp_nip21_decode_uri;
#[cfg(feature = "native")]
pub use publish::{nmp_app_cancel_action, nmp_app_retry_publish};
// #1726 — unified diagnostic pull accessor (routing/composition/merged).
// Replaces the deleted `nmp_app_recent_routing_decisions` and
// `nmp_app_composition_report` symbols. No compat shims kept.
#[cfg(feature = "native")]
pub use debug_info::nmp_app_debug_info;
#[cfg(feature = "external-signer")]
pub use external_signer::{
    nmp_app_deliver_external_signer_response, nmp_app_signin_nip55, nmp_external_signer_init,
};
pub use prestart_config::NmpConfigStatus;
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
#[cfg(feature = "native")]
pub use timeline::{
    // V-68 / V-112 (ADR-0042): nmp_app_open_author, nmp_app_close_author,
    // nmp_app_open_thread, nmp_app_close_thread deleted from timeline.rs.
    // V-68 Stage 2 (ADR-0042 amendment 2026-06-12): nmp_app_open_timeline
    // deleted from identity.rs.
    // ADR-0063 Lane H: nmp_app_claim_profile, nmp_app_release_profile deleted.
    // #1740/#2092: `nmp_app_open_contact_feed` / `nmp_app_close_contact_feed`
    // and the follow-feed declare/clear helpers are deleted. Apps open the
    // home feed through `nmp_app_open_feed(FeedScope::ActiveUserFollows)`.
    // #1946: event URI C-ABI front doors DELETED. Callers migrate to the typed
    // event-ref adapters below.
    nmp_app_close_interest,
    nmp_app_open_interest,
    nmp_app_open_uri,
};
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
