//! Browser runtime-floor and test-owned composition gates (#2061 / #2907).
//!
//! Native production-composition coverage already asserts
//! `nmp_codegen::canonical_default_action_namespaces()` through the FFI-backed
//! app. Browser-runtime tests pin the same canonical namespace source through
//! an explicit test-owned concept composition, while also proving bare browser
//! start installs only the runtime floor.

use std::sync::Arc;

use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::started_handle;

const BROWSER_FEATURE_GATED_ACTION_NAMESPACES: &[&str] = &[
    "nmp.follow",
    "nmp.follow_many",
    "nmp.nip25.react",
    "nmp.nip25.unreact",
    "nmp.nip51.add_bookmark",
    "nmp.nip51.add_bookmark_set_item",
    "nmp.nip51.block_relay",
    "nmp.nip51.publish_web_bookmark",
    "nmp.nip51.remove_bookmark",
    "nmp.nip51.remove_bookmark_set_item",
    "nmp.nip51.unblock_relay",
    "nmp.nip84.publish_highlight",
    "nmp.unfollow",
];

/// #1007 PR-7 -- injection identity: a store handed to `inject_store` must be
/// the exact `Arc` the kernel reducer holds after `start()`.
#[test]
fn inject_store_reaches_reducer_with_pointer_identity() {
    let custom: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());

    let handle = BrowserAppBuilder::new()
        .inject_store(Arc::clone(&custom))
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert!(
        Arc::ptr_eq(&custom, &handle.event_store_handle()),
        "inject_store must hand the exact Arc to the kernel reducer"
    );
}

#[test]
fn in_memory_start_does_not_alias_an_injected_store() {
    let unrelated: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());
    let handle = started_handle();
    assert!(
        !Arc::ptr_eq(&unrelated, &handle.event_store_handle()),
        "in_memory() start must use its own store, not some unrelated Arc"
    );
}

/// #1007 PR-8 -- degraded-open diagnostic via the builder path.
#[test]
fn with_store_open_failure_surfaces_through_the_kernel() {
    let reason = "opfs_store_open_failure: quota_denied".to_string();
    let handle = BrowserAppBuilder::new()
        .in_memory()
        .with_store_open_failure(Some(reason.clone()))
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();

    assert_eq!(
        handle.store_open_failure(),
        Some(reason),
        "with_store_open_failure must reach the kernel diagnostic"
    );
}

#[test]
fn healthy_in_memory_start_reports_no_store_open_failure() {
    let cleared = BrowserAppBuilder::new()
        .in_memory()
        .with_store_open_failure(None)
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();
    assert!(cleared.store_open_failure().is_none());
    assert!(started_handle().store_open_failure().is_none());
}

#[test]
fn bare_browser_start_does_not_register_app_owned_concept_namespaces() {
    let handle = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();
    let registered = handle.runtime.action_registry.action_namespaces();

    for ns in [
        "nmp.follow",
        "nmp.replies.reply",
        "nmp.nip17.send",
        "nmp.nip18.repost",
        "nmp.nip25.react",
    ] {
        assert!(
            !registered.iter().any(|registered| registered == ns),
            "bare browser runtime floor must not register app-owned concept \
             namespace `{ns}`; registered namespaces: {registered:?}"
        );
    }
}

#[test]
fn test_browser_composition_registers_every_canonical_default_action_namespace() {
    let handle = started_handle();
    let registered = handle.runtime.action_registry.action_namespaces();

    for ns in browser_default_action_namespaces() {
        assert!(
            registered.iter().any(|registered| registered == ns),
            "test browser composition omitted canonical action namespace `{ns}`; \
             registered namespaces: {registered:?}"
        );
    }

    assert!(
        !registered
            .iter()
            .any(|ns| ns == "nmp.template.never.registered"),
        "control case: an unregistered namespace must not appear"
    );
}

#[cfg(feature = "reactions")]
#[test]
fn browser_reactions_feature_registers_nip25_action_namespaces() {
    assert_registered_action_namespaces(&["nmp.nip25.react", "nmp.nip25.unreact"]);
}

#[cfg(feature = "bookmarks")]
#[test]
fn browser_bookmarks_feature_registers_nip84_action_namespaces() {
    assert_registered_action_namespaces(&["nmp.nip84.publish_highlight"]);
}

#[cfg(feature = "social-graph")]
#[test]
fn browser_social_graph_feature_registers_nip02_and_nip51_action_namespaces() {
    assert_registered_action_namespaces(&[
        "nmp.follow",
        "nmp.follow_many",
        "nmp.nip51.add_bookmark",
        "nmp.nip51.add_bookmark_set_item",
        "nmp.nip51.block_relay",
        "nmp.nip51.publish_web_bookmark",
        "nmp.nip51.remove_bookmark",
        "nmp.nip51.remove_bookmark_set_item",
        "nmp.nip51.unblock_relay",
        "nmp.unfollow",
    ]);
}

fn browser_default_action_namespaces() -> Vec<&'static str> {
    nmp_codegen::canonical_default_action_namespaces()
        .into_iter()
        .filter(|ns| !BROWSER_FEATURE_GATED_ACTION_NAMESPACES.contains(ns))
        .collect()
}

#[cfg(any(feature = "bookmarks", feature = "reactions", feature = "social-graph"))]
fn assert_registered_action_namespaces(namespaces: &[&str]) {
    let handle = started_handle();
    let registered = handle.runtime.action_registry.action_namespaces();
    for ns in namespaces {
        assert!(
            registered.iter().any(|registered| registered == ns),
            "test browser composition omitted feature-gated action namespace `{ns}`; \
             registered namespaces: {registered:?}"
        );
    }
}

#[test]
fn browser_runtime_floor_defers_required_substrate_slots_before_start() {
    let mut builder = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default());

    crate::builder::install_browser_runtime_floor(&mut builder);

    let inner = builder
        .inner
        .lock()
        .expect("browser builder mutex must not be poisoned");

    assert!(inner.routing_substrate_factory.is_some());
    assert!(inner.publish_resolver_factory.is_some());
    assert!(inner.mailbox_cache_reader.is_some());
    assert!(inner.profile_lookup.is_some());
    assert!(inner.dm_inbox_relay_lookup.is_none());
    assert!(inner.blocked_relay_lookup.is_some());
    assert!(inner.coverage_hook.is_some());
}

#[test]
#[should_panic(expected = "BrowserAppBuilder runtime floor must be installed exactly once")]
fn browser_runtime_floor_rejects_duplicate_install() {
    let mut builder = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default());

    crate::builder::install_browser_runtime_floor(&mut builder);
    crate::builder::install_browser_runtime_floor(&mut builder);
}
