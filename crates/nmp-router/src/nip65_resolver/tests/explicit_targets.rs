//! Code path 5 — `PublishTarget::explicit(..)` short-circuits kind:10002
//! resolution entirely: every relay is passed through unchanged and carries
//! the `RelaySelectionReason::Explicit { route_class }` variant.

use std::sync::Arc;

use super::fixtures::{find_reason, mk_resolver, no_block, urls_of, AUTHOR_HEX};
use crate::InMemoryMailboxCache;
use nmp_core::publish::{OutboxResolver, PublishRouteClass, PublishTarget, RelaySelectionReason};
use std::collections::BTreeSet;

#[test]
fn nip65_resolver_returns_explicit_unchanged() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = mk_resolver(&cache);
    let explicit = vec!["wss://a.example".to_string(), "wss://b.example".to_string()];
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::explicit(explicit.clone(), PublishRouteClass::ManualOverride),
        1,
        &no_block(),
    );
    assert_eq!(urls_of(&out), explicit.into_iter().collect::<BTreeSet<_>>());
}

#[test]
fn resolve_returns_explicit_relay_reason() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = mk_resolver(&cache);
    let explicit = vec!["wss://a.example".to_string(), "wss://b.example".to_string()];
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::explicit(explicit.clone(), PublishRouteClass::ManualOverride),
        1,
        &no_block(),
    );
    assert_eq!(out.len(), 2);
    for url in &explicit {
        assert!(matches!(
            find_reason(&out, url),
            Some(RelaySelectionReason::Explicit {
                route_class: PublishRouteClass::ManualOverride
            })
        ));
    }
}
