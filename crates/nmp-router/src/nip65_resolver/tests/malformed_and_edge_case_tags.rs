//! `kind:10002` input-tolerance edge cases: malformed relay tags are skipped
//! rather than aborting the parse, an unmarked `r` tag counts as both
//! read+write, and an unparseable author pubkey fails closed to an empty
//! relay set.

use std::sync::Arc;

use super::fixtures::{
    mk_resolver, no_block, relay_tag, seed_kind10002, urls_of, AUTHOR_HEX, RECIPIENT_HEX,
};
use crate::InMemoryMailboxCache;
use nmp_core::publish::{OutboxResolver, PublishTarget};

#[test]
fn nip65_resolver_handles_malformed_kind10002_gracefully() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![
            // Missing url tag → skip
            vec!["r".into()],
            // Non-relay scheme → skip
            vec!["r".into(), "https://example.com".into()],
            // Valid one to confirm we don't abort
            vec!["r".into(), "wss://valid.example".into(), "write".into()],
            // Garbage tag prefix → skip
            vec!["x".into(), "wss://wrong-tag.example".into()],
        ],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    let urls = urls_of(&out);
    assert!(urls.contains("wss://valid.example"));
    assert!(!urls.contains("https://example.com"));
    assert!(!urls.contains("wss://wrong-tag.example"));
}

#[test]
fn nip65_resolver_unmarked_tag_is_both() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![relay_tag("wss://both.example", None)],
    );
    seed_kind10002(
        &cache,
        RECIPIENT_HEX,
        vec![relay_tag("wss://recipient-both.example", None)],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let urls = urls_of(&out);
    // Unmarked counts as both → write goes here.
    assert!(urls.contains("wss://both.example"));
    // Recipient unmarked also reads here.
    assert!(urls.contains("wss://recipient-both.example"));
}

/// An unparseable (non-hex, wrong-length) author pubkey means the
/// kind:10002 lookup returns `None`. This is also unroutable → empty relay
/// set (fail-closed). Same `NoTargets` outcome upstream.
#[test]
fn nip65_resolver_invalid_author_hex_returns_empty() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = mk_resolver(&cache);
    // Short / non-hex author → lookup returns None → empty (fail-closed).
    let out = resolver.resolve("not-hex", &[], &PublishTarget::Auto, 1, &no_block());
    assert!(
        out.is_empty(),
        "unparseable author pubkey must resolve to empty set (fail-closed); \
         got {out:?}"
    );
}
