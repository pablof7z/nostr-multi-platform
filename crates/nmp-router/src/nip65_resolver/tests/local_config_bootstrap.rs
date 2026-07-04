//! Code path 2 — the active account's locally configured write relays
//! (`RelaySelectionReason::LocalConfigRelay`).
//!
//! Regression guard for audit finding 13 (MEDIUM): once a `kind:10002` is
//! cached with an explicitly EMPTY write set (all entries read-marked), the
//! resolver MUST fail closed to `NoTargets` rather than fall through to
//! `local_write_relays`. Overriding an explicit "publish nowhere" signal with
//! locally configured relays would violate D3 (outbox automatic: routing from
//! durable state, not a hardcoded fallback).
//!
//! Distinct from the empty-write-set case above: when a cached `kind:10002`
//! has a NON-EMPTY write set that simply does not happen to list a relay the
//! active account has locally configured (Settings, or a test/dev harness),
//! `local_write_relays` is unioned in as an ADDITIONAL target, not skipped.
//! Before this fix the guard was `kind10002.is_none()` — any cached
//! kind:10002 at all, even one with unrelated content, silently and
//! permanently excluded a just-added local write relay from every
//! self-authored publish (kind:0/3/6). NIP-65 tells other clients where to
//! find your content; it is not an allowlist restricting where your OWN
//! client is permitted to write. See the write/outbox-routing investigation
//! for chirp#69.

use std::sync::{Arc, Mutex};

use super::fixtures::{
    find_reason, local_write_slot_with, mk_resolver, no_block, seed_relay, urls_of, AUTHOR_HEX,
    RECIPIENT_HEX,
};
use crate::nip65_resolver::Nip65OutboxResolver;
use crate::InMemoryMailboxCache;
use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason};
use nmp_core::slots::new_indexer_relays_slot;
use nmp_core::substrate::MailboxCache;
use std::collections::BTreeSet;

#[test]
fn nip65_resolver_returns_empty_when_no_kind10002() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert!(
        out.is_empty(),
        "author with no kind:10002 must resolve to empty set (fail-closed, NoTargets); \
         got {out:?}"
    );
}

#[test]
fn nip65_resolver_uses_local_writes_for_active_account_only() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = Nip65OutboxResolver::with_local_relays(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );

    let own = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert_eq!(
        urls_of(&own),
        BTreeSet::from(["wss://local-write.example".to_string()])
    );

    let other = resolver.resolve(RECIPIENT_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert!(
        other.is_empty(),
        "local relay rows must not route already-signed events for other authors"
    );
}

#[test]
fn resolve_returns_app_relay_reason_when_no_kind10002() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    let resolver = Nip65OutboxResolver::with_local_relays(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert!(matches!(
        find_reason(&out, "wss://local-write.example"),
        Some(RelaySelectionReason::LocalConfigRelay)
    ));
}

#[test]
fn resolve_fail_closed_when_kind10002_has_only_read_relays_non_discovery() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    // kind:10002 with a single read-only relay (no write entries).
    seed_relay(&cache, AUTHOR_HEX, "wss://read-only.example", "read");
    // Active account + non-empty local_write_relays — the fallback must NOT fire.
    let resolver = Nip65OutboxResolver::with_local_relays(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );
    // Non-discovery kind (kind:1 note).
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert!(
        out.is_empty(),
        "kind:10002 with only read relays must resolve to empty (fail-closed, NoTargets) \
         for a non-discovery kind; local_write_relays must NOT be used when kind:10002 \
         exists — got {out:?}"
    );
    // Confirm the read relay itself is also absent (it's not a write relay).
    let urls = urls_of(&out);
    assert!(!urls.contains("wss://read-only.example"));
    assert!(!urls.contains("wss://local-write.example"));
}

/// Complementary pin: the bootstrap fallback MUST still fire when the active
/// account has NO kind:10002 cached at all (lookup returns `None`). This pins
/// the deliberate bootstrap behavior so the fix cannot overshoot.
#[test]
fn resolve_local_write_fallback_fires_when_no_kind10002_at_all() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    // No kind:10002 stored at all → lookup returns None.
    let resolver = Nip65OutboxResolver::with_local_relays(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );
    // Non-discovery kind — bootstrap fallback must fire.
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert_eq!(
        urls_of(&out),
        BTreeSet::from(["wss://local-write.example".to_string()]),
        "active account with no kind:10002 must fall back to local_write_relays \
         (bootstrap window — user has onboarded but kind:10002 not yet confirmed)"
    );
    assert!(matches!(
        find_reason(&out, "wss://local-write.example"),
        Some(RelaySelectionReason::LocalConfigRelay)
    ));
}

/// The gap this fix closes: a cached kind:10002 with a NON-EMPTY write set
/// that does not include the account's locally-configured write relay must
/// NOT silently exclude that relay. Both the cached NIP-65 write relay AND
/// the local relay must appear in the resolved set.
#[test]
fn resolve_unions_local_write_relay_with_non_empty_kind10002() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    // kind:10002 exists and has a real (non-empty) write relay — but it is
    // NOT the relay the active account has locally configured (e.g. a
    // relay a user just added in Settings, or a live test harness pointed
    // at a local dev relay).
    seed_relay(&cache, AUTHOR_HEX, "wss://author-write.example", "write");
    let resolver = Nip65OutboxResolver::with_local_relays(
        {
            let mailbox_cache: Arc<dyn MailboxCache> = cache.clone();
            mailbox_cache
        },
        new_indexer_relays_slot(),
        local_write_slot_with(vec!["wss://local-write.example".to_string()]),
        Arc::new(Mutex::new(Some(AUTHOR_HEX.to_string()))),
    );
    // Non-discovery kind (kind:1) — the strictest case: no indexer fan-out
    // to accidentally carry the local relay in via a different lane.
    let out = resolver.resolve(AUTHOR_HEX, &[], &PublishTarget::Auto, 1, &no_block());
    assert_eq!(
        urls_of(&out),
        BTreeSet::from([
            "wss://author-write.example".to_string(),
            "wss://local-write.example".to_string(),
        ]),
        "a non-empty cached kind:10002 write set must NOT exclude the active \
         account's locally-configured write relay"
    );
    assert!(matches!(
        find_reason(&out, "wss://author-write.example"),
        Some(RelaySelectionReason::AuthorWriteRelay)
    ));
    assert!(matches!(
        find_reason(&out, "wss://local-write.example"),
        Some(RelaySelectionReason::LocalConfigRelay)
    ));
}
