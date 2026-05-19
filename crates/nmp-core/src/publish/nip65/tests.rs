//! Unit tests for `Nip65OutboxResolver`.
//!
//! Split from `mod.rs` to keep the implementation file under the 300 LOC
//! soft cap (AGENTS.md). Tests cover: author writes, fail-closed on missing
//! kind:10002, recipient `#p` reads, explicit pass-through, malformed-tag
//! tolerance, unmarked-tag = both, invalid-hex author.
//!
//! T-publish-resolver-indexer (codex f81f735): the indexer-fallback tests
//! have been updated to assert the new fail-closed semantics — an author with
//! no kind:10002 resolves to an empty relay set, causing `NoTargets` upstream,
//! rather than silently widening to arbitrary public relays.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::{Nip65OutboxResolver, PublishTarget};
use crate::publish::traits::OutboxResolver;
use crate::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

const AUTHOR_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RECIPIENT_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

fn store_kind10002(store: &dyn EventStore, author_hex: &str, tags: Vec<Vec<String>>) {
    // Construct a unique 64-hex id keyed off author + kind so multiple
    // inserts in the same test do not collide.
    let prefix = &author_hex[..2];
    let id = format!("{:0<64}", format!("{}e10002", prefix));
    let raw = RawEvent {
        id,
        pubkey: author_hex.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    let verified = VerifiedEvent::from_raw_unchecked(raw);
    store
        .insert(verified, &"wss://test".to_string(), 1_700_000_000_000)
        .expect("insert");
}

fn mk_resolver(store: Arc<dyn EventStore>) -> Nip65OutboxResolver {
    Nip65OutboxResolver::new(store, Arc::new(std::sync::Mutex::new(Vec::new())))
}

#[test]
fn nip65_resolver_uses_author_writes_when_present() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![
            vec!["r".into(), "wss://write.example".into(), "write".into()],
            vec!["r".into(), "wss://read.example".into(), "read".into()],
        ],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(out.contains("wss://write.example"));
    // Read-only relays are NOT used for the author's own writes.
    assert!(!out.contains("wss://read.example"));
    // Fallback NOT consulted when author has writes.
    assert!(!out.contains("wss://fallback.example"));
}

/// T-publish-resolver-indexer (codex f81f735): an author with no kind:10002
/// must resolve to an **empty relay set** (fail-closed). The engine maps this
/// to `PublishEngineError::NoTargets` — the user sees a visible toast ("no
/// relay to publish to") rather than a silent widening to arbitrary relays.
/// This mirrors T134's subscription-side `unroutable_authors` semantics.
#[test]
fn nip65_resolver_returns_empty_when_no_kind10002() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = mk_resolver(store);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(
        out.is_empty(),
        "author with no kind:10002 must resolve to empty set (fail-closed, NoTargets); \
         got {out:?}"
    );
}

#[test]
fn nip65_resolver_unions_recipient_reads_for_p_tags() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec!["r".into(), "wss://author-write.example".into(), "write".into()]],
    );
    store_kind10002(
        store.as_ref(),
        RECIPIENT_HEX,
        vec![vec!["r".into(), "wss://recipient-read.example".into(), "read".into()]],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
    );
    assert!(out.contains("wss://author-write.example"));
    assert!(out.contains("wss://recipient-read.example"));
}

#[test]
fn nip65_resolver_returns_explicit_unchanged() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = mk_resolver(store);
    let explicit = vec![
        "wss://a.example".to_string(),
        "wss://b.example".to_string(),
    ];
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::Explicit {
            relays: explicit.clone(),
        },
        1,
    );
    assert_eq!(out, explicit.into_iter().collect::<BTreeSet<_>>());
}

#[test]
fn nip65_resolver_handles_malformed_kind10002_gracefully() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
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
    let resolver = mk_resolver(store);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(out.contains("wss://valid.example"));
    assert!(!out.contains("https://example.com"));
    assert!(!out.contains("wss://wrong-tag.example"));
}

#[test]
fn nip65_resolver_unmarked_tag_is_both() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec!["r".into(), "wss://both.example".into()]],
    );
    store_kind10002(
        store.as_ref(),
        RECIPIENT_HEX,
        vec![vec!["r".into(), "wss://recipient-both.example".into()]],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
    );
    // Unmarked counts as both → write goes here.
    assert!(out.contains("wss://both.example"));
    // Recipient unmarked also reads here.
    assert!(out.contains("wss://recipient-both.example"));
}

/// T-publish-resolver-indexer: an unparseable (non-hex, wrong-length) author
/// pubkey means the kind:10002 lookup returns `None`. This is also unroutable
/// → empty relay set (fail-closed). Same `NoTargets` outcome upstream.
#[test]
fn nip65_resolver_invalid_author_hex_returns_empty() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = mk_resolver(store);
    // Short / non-hex author → lookup returns None → empty (fail-closed).
    let out = resolver.resolve("not-hex", &[], &PublishTarget::Auto, 1);
    assert!(
        out.is_empty(),
        "unparseable author pubkey must resolve to empty set (fail-closed); \
         got {out:?}"
    );
}
