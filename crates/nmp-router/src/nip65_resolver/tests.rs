//! Unit tests for `Nip65OutboxResolver`.
//!
//! Split from the implementation file to keep `nip65_resolver.rs` under the
//! 500 LOC hand-authored ceiling (AGENTS.md). Tests cover: author writes,
//! fail-closed on missing kind:10002, recipient `#p` reads, explicit
//! pass-through, malformed-tag tolerance, unmarked-tag = both, invalid-hex
//! author, and per-code-path rationale strings (`ResolvedRelay::reason`).
//!
//! T-publish-resolver-indexer (codex f81f735): the indexer-fallback tests
//! have been updated to assert the new fail-closed semantics — an author with
//! no kind:10002 resolves to an empty relay set, causing `NoTargets` upstream,
//! rather than silently widening to arbitrary public relays.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::{Nip65OutboxResolver, RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD};
use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason, ResolvedRelay};
use nmp_core::slots::{
    new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot,
};
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};

/// Test helper — typed [`IndexerRelaysSlot`] pre-populated with `urls`.
/// Centralizes typed-slot construction so tests that need a non-empty
/// indexer set don't each spell `Arc::new(Mutex::new(...))` inline.
fn indexer_slot_with(urls: Vec<String>) -> IndexerRelaysSlot {
    let slot = new_indexer_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

/// Test helper — typed [`LocalWriteRelaysSlot`] pre-populated with
/// `urls`. Same rationale as [`indexer_slot_with`].
fn local_write_slot_with(urls: Vec<String>) -> LocalWriteRelaysSlot {
    let slot = new_local_write_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

/// Test helper — collapse the trait's `Vec<ResolvedRelay>` to the set of URLs.
/// Most assertions in this file only care about which URLs were selected;
/// reason-specific assertions use `find_reason` directly.
fn urls_of(resolved: &[ResolvedRelay]) -> BTreeSet<String> {
    resolved.iter().map(|r| r.url.clone()).collect()
}

/// Test helper — find the first `reason` variant for the given URL, or None
/// if the URL was not selected.
fn find_reason<'a>(resolved: &'a [ResolvedRelay], url: &str) -> Option<&'a RelaySelectionReason> {
    resolved.iter().find(|r| r.url == url).map(|r| &r.reason)
}

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
    Nip65OutboxResolver::new(store, new_indexer_relays_slot())
}

fn pk(n: u8) -> String {
    format!("{n:02x}").repeat(32)
}

fn threshold_recipients() -> Vec<String> {
    let mut recipients = vec![RECIPIENT_HEX.to_string()];
    recipients.extend((0..RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD - 1).map(|i| pk((i + 3) as u8)));
    recipients
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
    let urls = urls_of(&out);
    assert!(urls.contains("wss://write.example"));
    // Read-only relays are NOT used for the author's own writes.
    assert!(!urls.contains("wss://read.example"));
    // Fallback NOT consulted when author has writes.
    assert!(!urls.contains("wss://fallback.example"));
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
fn nip65_resolver_uses_local_writes_for_active_account_only() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = Nip65OutboxResolver::with_local_relays(
        store,
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );

    let own = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert_eq!(
        urls_of(&own),
        BTreeSet::from(["wss://local-write.example".to_string()])
    );

    let other = resolver.resolve(RECIPIENT_HEX, &[], &PublishTarget::Auto, 1);
    assert!(
        other.is_empty(),
        "local relay rows must not route already-signed events for other authors"
    );
}

#[test]
fn nip65_resolver_unions_recipient_reads_for_p_tags() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://author-write.example".into(),
            "write".into(),
        ]],
    );
    store_kind10002(
        store.as_ref(),
        RECIPIENT_HEX,
        vec![vec![
            "r".into(),
            "wss://recipient-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
    );
    let urls = urls_of(&out);
    assert!(urls.contains("wss://author-write.example"));
    assert!(urls.contains("wss://recipient-read.example"));
}

#[test]
fn nip65_resolver_skips_recipient_reads_at_p_tag_threshold() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://author-write.example".into(),
            "write".into(),
        ]],
    );
    store_kind10002(
        store.as_ref(),
        RECIPIENT_HEX,
        vec![vec![
            "r".into(),
            "wss://recipient-read.example".into(),
            "read".into(),
        ]],
    );
    let recipients = threshold_recipients();

    let resolver = mk_resolver(store);
    let out = resolver.resolve(AUTHOR_HEX, &recipients, &PublishTarget::Auto, 1);
    let urls = urls_of(&out);

    assert!(urls.contains("wss://author-write.example"));
    assert!(
        !urls.contains("wss://recipient-read.example"),
        "15+ distinct p-tagged pubkeys must not fan out to recipient inbox relays"
    );
}

#[test]
fn nip65_resolver_keeps_discovery_indexers_when_p_tag_threshold_skips_inboxes() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://author-write.example".into(),
            "write".into(),
        ]],
    );
    store_kind10002(
        store.as_ref(),
        RECIPIENT_HEX,
        vec![vec![
            "r".into(),
            "wss://recipient-read.example".into(),
            "read".into(),
        ]],
    );
    let recipients = threshold_recipients();
    let resolver = Nip65OutboxResolver::new(
        store,
        indexer_slot_with(vec!["wss://indexer.example".to_string()]),
    );

    let out = resolver.resolve(AUTHOR_HEX, &recipients, &PublishTarget::Auto, 3);
    let urls = urls_of(&out);

    assert!(urls.contains("wss://author-write.example"));
    assert!(urls.contains("wss://indexer.example"));
    assert!(!urls.contains("wss://recipient-read.example"));
}

#[test]
fn nip65_resolver_returns_explicit_unchanged() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = mk_resolver(store);
    let explicit = vec!["wss://a.example".to_string(), "wss://b.example".to_string()];
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::Explicit {
            relays: explicit.clone(),
        },
        1,
    );
    assert_eq!(urls_of(&out), explicit.into_iter().collect::<BTreeSet<_>>());
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
    let urls = urls_of(&out);
    assert!(urls.contains("wss://valid.example"));
    assert!(!urls.contains("https://example.com"));
    assert!(!urls.contains("wss://wrong-tag.example"));
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
    let urls = urls_of(&out);
    // Unmarked counts as both → write goes here.
    assert!(urls.contains("wss://both.example"));
    // Recipient unmarked also reads here.
    assert!(urls.contains("wss://recipient-both.example"));
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

// ---------------- ResolvedRelay::reason coverage ----------------

/// Code path 1 — author kind:10002 write relays carry the
/// `RelaySelectionReason::AuthorWriteRelay` variant. The variant is the
/// resolver contract; the kernel projection formats it into English at the
/// wire boundary (`publish_outbox::format_relay_reason`).
#[test]
fn resolve_returns_nip65_write_relay_reason() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://write.example".into(),
            "write".into(),
        ]],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(matches!(
        find_reason(&out, "wss://write.example"),
        Some(RelaySelectionReason::AuthorWriteRelay)
    ));
}

/// Code path 2 — when no kind:10002 is on file, the active account's locally
/// configured write relays appear with the
/// `RelaySelectionReason::LocalConfigRelay` variant.
#[test]
fn resolve_returns_app_relay_reason_when_no_kind10002() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = Nip65OutboxResolver::with_local_relays(
        store,
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(matches!(
        find_reason(&out, "wss://local-write.example"),
        Some(RelaySelectionReason::LocalConfigRelay)
    ));
}

/// Code path 3 — discovery kinds (kind:0 / kind:3 / kind:10000–19999) fan out
/// to indexer relays with a `RelaySelectionReason::DiscoveryIndexer { kind }`
/// variant carrying the originating kind so the user can tell whether the
/// relay was targeted for the profile (kind:0), contacts (kind:3), or a NIP
/// replaceable.
#[test]
fn resolve_returns_discovery_indexer_reason_for_kind0() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    // No kind:10002 → the discovery indexer is the only source. This isolates
    // code path 3 (indexer) from code path 1 (author writes).
    let resolver = Nip65OutboxResolver::new(
        store,
        indexer_slot_with(vec!["wss://indexer.example".to_string()]),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 0);
    assert!(matches!(
        find_reason(&out, "wss://indexer.example"),
        Some(RelaySelectionReason::DiscoveryIndexer { kind: 0 })
    ));
}

/// Code path 4 — recipient-inbox fan-out from `#p` tags carries a
/// `RelaySelectionReason::RecipientInbox { pubkey }` variant. The raw hex
/// pubkey rides on the variant; the kernel projection abbreviates it via
/// `short_npub` at the wire boundary.
#[test]
fn resolve_returns_inbox_relay_reason_for_p_tags() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://author-write.example".into(),
            "write".into(),
        ]],
    );
    store_kind10002(
        store.as_ref(),
        RECIPIENT_HEX,
        vec![vec![
            "r".into(),
            "wss://recipient-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
    );
    let reason = find_reason(&out, "wss://recipient-read.example")
        .expect("recipient read relay must be present");
    match reason {
        RelaySelectionReason::RecipientInbox { pubkey } => {
            assert_eq!(
                pubkey, RECIPIENT_HEX,
                "recipient pubkey rides verbatim on the variant; \
                 abbreviation is the projection's responsibility"
            );
        }
        other => panic!("expected RecipientInbox, got {other:?}"),
    }
}

/// Code path 5 — explicit targets short-circuit and every relay carries the
/// `RelaySelectionReason::Explicit` variant.
#[test]
fn resolve_returns_explicit_relay_reason() {
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
    assert_eq!(out.len(), 2);
    for url in &explicit {
        assert!(matches!(
            find_reason(&out, url),
            Some(RelaySelectionReason::Explicit)
        ));
    }
}
