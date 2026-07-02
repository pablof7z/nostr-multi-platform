//! Code path 3 — discovery kinds (`kind:0` / `kind:3` / `kind:10000–19999`)
//! fan out to indexer relays with a `RelaySelectionReason::DiscoveryIndexer
//! { kind }` variant, and those indexer relays survive even when the
//! recipient p-tag threshold suppresses per-recipient inbox fan-out.

use std::sync::Arc;

use super::fixtures::{
    find_reason, indexer_slot_with, no_block, seed_relay, threshold_recipients, urls_of,
    AUTHOR_HEX, AUTHOR_WRITE_RELAY, RECIPIENT_HEX, RECIPIENT_READ_RELAY,
};
use crate::nip65_resolver::Nip65OutboxResolver;
use crate::InMemoryMailboxCache;
use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason};
use nmp_core::substrate::MailboxCache;

/// This isolates code path 3 (indexer) from code path 1 (author writes) by
/// seeding no `kind:10002` at all, so the discovery indexer is the only
/// source.
#[test]
fn resolve_returns_discovery_indexer_reason_for_kind0() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = Nip65OutboxResolver::new(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        indexer_slot_with(vec!["wss://indexer.example".to_string()]),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 0, &no_block());
    assert!(matches!(
        find_reason(&out, "wss://indexer.example"),
        Some(RelaySelectionReason::DiscoveryIndexer { kind: 0 })
    ));
}

#[test]
fn nip65_resolver_keeps_discovery_indexers_when_p_tag_threshold_skips_inboxes() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_relay(&cache, AUTHOR_HEX, AUTHOR_WRITE_RELAY, "write");
    seed_relay(&cache, RECIPIENT_HEX, RECIPIENT_READ_RELAY, "read");
    let recipients = threshold_recipients();
    let resolver = Nip65OutboxResolver::new(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        indexer_slot_with(vec!["wss://indexer.example".to_string()]),
    );

    let out = resolver.resolve(
        AUTHOR_HEX,
        &recipients,
        &PublishTarget::Auto,
        3,
        &no_block(),
    );
    let urls = urls_of(&out);

    assert!(urls.contains(AUTHOR_WRITE_RELAY));
    assert!(urls.contains("wss://indexer.example"));
    assert!(!urls.contains(RECIPIENT_READ_RELAY));
}
