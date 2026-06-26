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

use crate::{BrowserAppBuilder, BrowserRunConfig};

use super::started_handle;

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
