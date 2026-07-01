//! Browser production composition parity gates (#2061).
//!
//! Native production-composition coverage already asserts
//! `nmp_codegen::canonical_default_action_namespaces()` through the FFI-backed
//! app. These browser-runtime tests pin the same canonical namespace source and
//! the browser builder's deferred substrate slots.

use std::sync::Arc;

use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::started_handle;

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
fn browser_start_registers_every_canonical_default_action_namespace() {
    let handle = started_handle();
    let registered = handle.runtime.action_registry.action_namespaces();

    for ns in nmp_codegen::canonical_default_action_namespaces() {
        assert!(
            registered.iter().any(|registered| registered == ns),
            "browser production composition omitted canonical action namespace `{ns}`; \
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

#[test]
fn browser_production_composition_defers_required_substrate_slots_before_start() {
    let mut builder = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default());

    crate::builder::install_browser_production_composition(&mut builder);

    let inner = builder
        .inner
        .lock()
        .expect("browser builder mutex must not be poisoned");

    assert!(inner.routing_substrate_factory.is_some());
    assert!(inner.publish_resolver_factory.is_some());
    assert!(inner.mailbox_cache_reader.is_some());
    assert!(inner.profile_lookup.is_some());
    assert!(inner.dm_inbox_relay_lookup.is_some());
    assert!(inner.blocked_relay_lookup.is_some());
    assert!(inner.coverage_hook.is_some());
}

#[test]
#[should_panic(
    expected = "BrowserAppBuilder production composition must be installed exactly once"
)]
fn browser_production_composition_rejects_duplicate_install() {
    let mut builder = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default());

    crate::builder::install_browser_production_composition(&mut builder);
    crate::builder::install_browser_production_composition(&mut builder);
}
