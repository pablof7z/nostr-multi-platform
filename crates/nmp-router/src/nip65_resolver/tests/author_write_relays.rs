//! Code path 1 — author `kind:10002` write relays take precedence over
//! read-only relays and any fallback, and carry the
//! `RelaySelectionReason::AuthorWriteRelay` variant.

use std::sync::Arc;

use super::fixtures::{
    find_reason, mk_resolver, no_block, relay_tag, seed_kind10002, seed_relay, urls_of, AUTHOR_HEX,
};
use crate::InMemoryMailboxCache;
use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason};

#[test]
fn nip65_resolver_uses_author_writes_when_present() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![
            relay_tag("wss://write.example", Some("write")),
            relay_tag("wss://read.example", Some("read")),
        ],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    let urls = urls_of(&out);
    assert!(urls.contains("wss://write.example"));
    // Read-only relays are NOT used for the author's own writes.
    assert!(!urls.contains("wss://read.example"));
    // Fallback NOT consulted when author has writes.
    assert!(!urls.contains("wss://fallback.example"));
}

/// The variant is the resolver contract; the kernel projection formats it
/// into English at the wire boundary (`publish_outbox::format_relay_reason`).
#[test]
fn resolve_returns_nip65_write_relay_reason() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_relay(&cache, AUTHOR_HEX, "wss://write.example", "write");
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert!(matches!(
        find_reason(&out, "wss://write.example"),
        Some(RelaySelectionReason::AuthorWriteRelay)
    ));
}
