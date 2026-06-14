//! `Nip65OutboxResolver` tests — `ResolvedRelay::reason` coverage, the
//! fail-closed empty-write-set guards, and the Bug 1 blocked-relay PUBLISH
//! filter. Split from `nip65_resolver/tests.rs` to keep both files under the
//! 500-LOC hand-authored ceiling (AGENTS.md).

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use super::{Nip65OutboxResolver, RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD as _};
use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason, ResolvedRelay};
use nmp_core::slots::{new_indexer_relays_slot, new_local_write_relays_slot, IndexerRelaysSlot, LocalWriteRelaysSlot};
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
use nmp_core::substrate::{empty_blocked_relay_lookup, BlockedRelayLookup};

use crate::blocked_relays::{InMemoryBlockedRelayCache, Kind10006Parser};

const AUTHOR_HEX: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const RECIPIENT_HEX: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// Empty blocked-relay lookup — no relays blocked.
fn eb() -> Arc<dyn BlockedRelayLookup> {
    empty_blocked_relay_lookup()
}

/// A `BlockedRelayLookup` with `urls` blocked for `author_hex`, populated
/// through the real kind:10006 ingest parser so the canonicalisation path is
/// exercised end-to-end (mirrors production: the parser is the cache's sole
/// writer).
fn blocks_for(author_hex: &str, urls: &[&str]) -> Arc<dyn BlockedRelayLookup> {
    let cache = Arc::new(InMemoryBlockedRelayCache::new());
    let parser = Kind10006Parser::new(Arc::clone(&cache));
    let tags: Vec<Vec<String>> = urls
        .iter()
        .map(|u| vec!["relay".to_string(), (*u).to_string()])
        .collect();
    parser.parse_event(&VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "00".repeat(32),
        pubkey: author_hex.to_string(),
        created_at: 0,
        kind: 10_006,
        tags,
        content: String::new(),
        sig: "22".repeat(64),
    }));
    cache
}

fn indexer_slot_with(urls: Vec<String>) -> IndexerRelaysSlot {
    let slot = new_indexer_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

fn local_write_slot_with(urls: Vec<String>) -> LocalWriteRelaysSlot {
    let slot = new_local_write_relays_slot();
    if let Ok(mut guard) = slot.lock() {
        guard.replace(urls);
    }
    slot
}

fn urls_of(resolved: &[ResolvedRelay]) -> BTreeSet<String> {
    resolved.iter().map(|r| r.url.clone()).collect()
}

fn find_reason<'a>(resolved: &'a [ResolvedRelay], url: &str) -> Option<&'a RelaySelectionReason> {
    resolved.iter().find(|r| r.url == url).map(|r| &r.reason)
}

fn store_kind10002(store: &dyn EventStore, author_hex: &str, tags: Vec<Vec<String>>) {
    let prefix = &author_hex[..2];
    let id = format!("{:0<64}", format!("{prefix}e10002"));
    let raw = RawEvent {
        id,
        pubkey: author_hex.to_string(),
        created_at: 1_700_000_000,
        kind: 10002,
        tags,
        content: String::new(),
        sig: "0".repeat(128),
    };
    store
        .insert(VerifiedEvent::from_raw_unchecked(raw), &"wss://test".to_string(), 1_700_000_000_000)
        .expect("insert");
}

fn mk_resolver(store: Arc<dyn EventStore>) -> Nip65OutboxResolver {
    Nip65OutboxResolver::new(store, new_indexer_relays_slot(), eb())
}

// ---------------- ResolvedRelay::reason coverage ----------------

/// Code path 1 — author kind:10002 write relays carry the
/// `RelaySelectionReason::AuthorWriteRelay` variant.
#[test]
fn resolve_returns_nip65_write_relay_reason() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec!["r".into(), "wss://write.example".into(), "write".into()]],
    );
    let resolver = mk_resolver(store);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(matches!(
        find_reason(&out, "wss://write.example"),
        Some(RelaySelectionReason::AuthorWriteRelay)
    ));
}

/// Code path 2 — no kind:10002 on file → the active account's locally
/// configured write relays appear with `RelaySelectionReason::LocalConfigRelay`.
#[test]
fn resolve_returns_app_relay_reason_when_no_kind10002() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = Nip65OutboxResolver::with_local_relays(
        store,
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
        eb(),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(matches!(
        find_reason(&out, "wss://local-write.example"),
        Some(RelaySelectionReason::LocalConfigRelay)
    ));
}

/// Code path 3 — discovery kinds fan out to indexer relays with a
/// `RelaySelectionReason::DiscoveryIndexer { kind }` variant.
#[test]
fn resolve_returns_discovery_indexer_reason_for_kind0() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = Nip65OutboxResolver::new(
        store,
        indexer_slot_with(vec!["wss://indexer.example".to_string()]),
        eb(),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 0);
    assert!(matches!(
        find_reason(&out, "wss://indexer.example"),
        Some(RelaySelectionReason::DiscoveryIndexer { kind: 0 })
    ));
}

/// Code path 4 — recipient-inbox fan-out from `#p` tags carries a
/// `RelaySelectionReason::RecipientInbox { pubkey }` variant with the raw hex.
#[test]
fn resolve_returns_inbox_relay_reason_for_p_tags() {
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
    let out = resolver.resolve(AUTHOR_HEX, &[RECIPIENT_HEX.to_string()], &PublishTarget::Auto, 1);
    let reason = find_reason(&out, "wss://recipient-read.example")
        .expect("recipient read relay must be present");
    match reason {
        RelaySelectionReason::RecipientInbox { pubkey } => {
            assert_eq!(pubkey, RECIPIENT_HEX, "recipient pubkey rides verbatim (D6)");
        }
        other => panic!("expected RecipientInbox, got {other:?}"),
    }
}

// ─── Fail-closed empty-write-set tests (audit finding 13) ──────────────────

/// When the active account's cached kind:10002 has an explicitly empty write
/// set (all read-marked), a non-discovery kind MUST resolve empty (`NoTargets`),
/// NOT fall through to `local_write_relays` (D3 — route from durable state).
#[test]
fn resolve_fail_closed_when_kind10002_has_only_read_relays_non_discovery() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec!["r".into(), "wss://read-only.example".into(), "read".into()]],
    );
    let resolver = Nip65OutboxResolver::with_local_relays(
        store,
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
        eb(),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert!(out.is_empty(), "kind:10002 with only read relays must resolve empty — got {out:?}");
    let urls = urls_of(&out);
    assert!(!urls.contains("wss://read-only.example"));
    assert!(!urls.contains("wss://local-write.example"));
}

/// The bootstrap fallback MUST still fire when the active account has NO
/// kind:10002 cached at all (lookup returns `None`).
#[test]
fn resolve_local_write_fallback_fires_when_no_kind10002_at_all() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = Nip65OutboxResolver::with_local_relays(
        store,
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
        eb(),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    assert_eq!(
        urls_of(&out),
        BTreeSet::from(["wss://local-write.example".to_string()]),
        "active account with no kind:10002 must fall back to local_write_relays"
    );
    assert!(matches!(
        find_reason(&out, "wss://local-write.example"),
        Some(RelaySelectionReason::LocalConfigRelay)
    ));
}

/// Code path 5 — explicit targets short-circuit; every relay carries the
/// `RelaySelectionReason::Explicit` variant.
#[test]
fn resolve_returns_explicit_relay_reason() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = mk_resolver(store);
    let explicit = vec!["wss://a.example".to_string(), "wss://b.example".to_string()];
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::Explicit { relays: explicit.clone() },
        1,
    );
    assert_eq!(out.len(), 2);
    for url in &explicit {
        assert!(matches!(find_reason(&out, url), Some(RelaySelectionReason::Explicit)));
    }
}

// ─── Bug 1 — blocked-relay filter on the PUBLISH path ──────────────────────

/// Bug 1 (HIGH) — a relay the author has blocked (kind:10006) MUST NOT appear
/// in the resolved publish set even when listed as a write relay in the
/// author's kind:10002. Before the fix the resolver had no `BlockedRelayLookup`
/// and returned the blocked relay verbatim (the subscribe path already filtered
/// it; the publish path did not).
#[test]
fn blocked_relay_absent_from_publish_resolution() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![
            vec!["r".into(), "wss://blocked.example".into(), "write".into()],
            vec!["r".into(), "wss://ok.example".into(), "write".into()],
        ],
    );
    let resolver = Nip65OutboxResolver::new(
        store,
        new_indexer_relays_slot(),
        blocks_for(AUTHOR_HEX, &["wss://blocked.example"]),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    let urls = urls_of(&out);
    assert!(!urls.contains("wss://blocked.example"), "blocked relay must be filtered — got {urls:?}");
    assert!(urls.contains("wss://ok.example"), "non-blocked write relays must remain — got {urls:?}");
}

/// Bug 1 + Bug 2 interaction — the kind:10002 lists the blocked relay in a
/// different casing / with a trailing slash (`wss://BLOCKED.EXAMPLE/`). After
/// canonicalisation the cached write relay is `wss://blocked.example`, which
/// matches the canonicalised blocked-cache key, so it must be filtered.
#[test]
fn blocked_relay_casing_absent_from_publish_resolution() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store_kind10002(
        store.as_ref(),
        AUTHOR_HEX,
        vec![vec!["r".into(), "wss://BLOCKED.EXAMPLE/".into(), "write".into()]],
    );
    let resolver = Nip65OutboxResolver::new(
        store,
        new_indexer_relays_slot(),
        blocks_for(AUTHOR_HEX, &["wss://blocked.example"]),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1);
    let urls = urls_of(&out);
    assert!(!urls.contains("wss://blocked.example"), "case-variant blocked relay must be filtered — got {urls:?}");
    assert!(out.is_empty(), "the only write relay was the blocked one — set must be empty — got {out:?}");
}

/// Bug 1 — explicit targets win (D3). A blocked URL passed as
/// `PublishTarget::Explicit` must pass through unchanged; only Auto-resolved
/// relays are filtered.
#[test]
fn blocked_relay_does_not_affect_explicit_targets() {
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    let resolver = Nip65OutboxResolver::new(
        store,
        new_indexer_relays_slot(),
        blocks_for(AUTHOR_HEX, &["wss://blocked.example"]),
    );
    let explicit = vec!["wss://blocked.example".to_string(), "wss://other.example".to_string()];
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[],
        &PublishTarget::Explicit { relays: explicit.clone() },
        1,
    );
    assert_eq!(
        urls_of(&out),
        explicit.into_iter().collect::<BTreeSet<_>>(),
        "explicit targets win — a blocked URL passed explicitly must pass through (D3)"
    );
}
