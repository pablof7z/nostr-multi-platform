//! Browser/default composition parity gates (#2061).
//!
//! Native default-composition coverage already asserts
//! `nmp_codegen::canonical_default_action_namespaces()` through the FFI-backed
//! app. These browser-runtime tests pin the same canonical namespace source and
//! the browser builder's deferred substrate slots, so #2053 cannot pass with a
//! browser start path that forgot NMP defaults or left the routing/publish
//! substrate unwired. There are no browser exclusions for canonical action
//! namespaces; signer/capability provider implementations remain an explicit
//! app/provider decision and are not installed by `register_defaults`.

use std::sync::Arc;

use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::started_handle;

/// #1007 PR-7 — injection identity: a store handed to `inject_store` must be the
/// exact `Arc` the kernel reducer holds after `start()` (no wrapping, no swap).
///
/// This is the native, always-runnable analog of the wasm OPFS injection: the
/// async hook (`NmpWasmRuntime::prepare_store`) parks an `Arc<dyn EventStore>`
/// that `handle_start` feeds straight into this same `inject_store` seam, so
/// proving the seam preserves pointer identity proves the OPFS store reaches the
/// reducer intact.
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
        "inject_store must hand the exact Arc to the kernel reducer — \
         the store the OPFS hook opens (#1007 PR-7) must reach the reducer unwrapped"
    );
}

/// Control: the default `in_memory()` start path must NOT alias an unrelated
/// injected store — guards the identity assertion above against a false positive.
#[test]
fn in_memory_start_does_not_alias_an_injected_store() {
    let unrelated: Arc<dyn nmp_store::EventStore> = Arc::new(nmp_store::MemEventStore::new());
    let handle = started_handle();
    assert!(
        !Arc::ptr_eq(&unrelated, &handle.event_store_handle()),
        "in_memory() start must use its own store, not some unrelated Arc"
    );
}

/// #1007 PR-8 — degraded-open diagnostic via the builder path: a reason handed
/// to `with_store_open_failure` must be recorded on the kernel and readable as
/// `store_open_failure` after `start()`. This is the builder-path analog of the
/// native LMDB `v67_store_open_failure` channel: a browser session that fell
/// back to in-memory reports the SAME Tier-3 diagnostic.
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
        "with_store_open_failure must reach the kernel's Tier-3 store_open_failure diagnostic"
    );
}

/// Control: a builder that never declares a degraded reason (`None` / unset)
/// must start clean — no false-positive `store_open_failure`.
#[test]
fn healthy_in_memory_start_reports_no_store_open_failure() {
    let cleared = BrowserAppBuilder::new()
        .in_memory()
        .with_store_open_failure(None)
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default())
        .start();
    assert!(
        cleared.store_open_failure().is_none(),
        "an explicit None degraded reason must leave store_open_failure absent"
    );

    // And the default path (setter never called) is equally clean.
    assert!(
        started_handle().store_open_failure().is_none(),
        "a default in-memory start must not report a store_open_failure"
    );
}

#[test]
fn browser_start_registers_every_canonical_default_action_namespace() {
    let handle = started_handle();
    let registered = handle.runtime.action_registry.action_namespaces();

    for ns in nmp_codegen::canonical_default_action_namespaces() {
        assert!(
            registered.iter().any(|registered| registered == ns),
            "browser default composition omitted canonical action namespace `{ns}`; \
             registered namespaces: {registered:?}"
        );
    }

    assert!(
        !registered
            .iter()
            .any(|ns| ns == "nmp.template.never.registered"),
        "control case: an unregistered namespace must not appear in the browser action registry"
    );
}

#[test]
fn browser_defaults_defer_required_substrate_slots_before_start() {
    let mut builder = BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(BrowserRunConfig::default());

    nmp_defaults::register_defaults(&mut builder);

    let inner = builder
        .inner
        .lock()
        .expect("browser builder mutex must not be poisoned");

    assert!(
        inner.routing_substrate_factory.is_some(),
        "browser defaults must install the routing-substrate factory"
    );
    assert!(
        inner.publish_resolver_factory.is_some(),
        "browser defaults must install the publish-resolver factory"
    );
    assert!(
        inner.mailbox_cache_reader.is_some(),
        "browser defaults must install the shared mailbox-cache reader"
    );
    assert!(
        inner.profile_lookup.is_some(),
        "browser defaults must install the profile lookup substrate"
    );
    assert!(
        inner.contacts_lookup.is_some(),
        "browser defaults must install the contacts lookup substrate"
    );
    assert!(
        inner.dm_inbox_relay_lookup.is_some(),
        "browser defaults must install the DM-inbox relay lookup substrate"
    );
    assert!(
        inner.blocked_relay_lookup.is_some(),
        "browser defaults must install the blocked-relay lookup substrate"
    );
    assert!(
        inner.coverage_hook.is_some(),
        "browser defaults must install the coverage hook substrate"
    );
}
