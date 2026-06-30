//! Bug 3 — lane 6 (Indexer) must scope the REQ to the discovery-kind subset.
//!
//! When an interest carries a MIX of discovery and content kinds (e.g.
//! `[1, 3]`), lane 6 fires on the strength of the discovery kind (kind:3) and
//! adds the operator's indexer relays to the routed set. Before the fix the
//! kernel then sent the FULL interest kinds `[1, 3]` to the indexer — leaking
//! kind:1 notes onto an indexer that should only carry discovery kinds. The
//! fix records a per-relay kind scope (`RoutedRelaySet::kind_overrides`) so the
//! frame-builder can constrain the indexer REQ to the discovery subset.
//!
//! Split into its own file to keep `tests.rs` / `tests_lanes.rs` under the
//! 500-LOC ceiling.

use super::*;
use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{BlockedRelaySet, MailboxCache, SessionKeySet};
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, InterestShape};
use nmp_store::{RawEvent, VerifiedEvent};

use crate::{InMemoryMailboxCache, Kind10002Parser};

const INDEXER: &str = "wss://indexer.example";

fn interest_with_kinds(kinds: &[u32]) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(7),
        scope: InterestScope::Global,
        shape: InterestShape {
            kinds: kinds.iter().copied().collect(),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: true,
    }
}

fn ctx<'a>(
    cache: &'a dyn MailboxCache,
    blocked: &'a BlockedRelaySet,
    indexer_relays: &'a [String],
) -> RoutingContext<'a> {
    RoutingContext {
        active_account: None,
        session_keys: SessionKeySet {
            indexer_relays,
            ..SessionKeySet::default()
        },
        mailbox_cache: cache,
        blocked_relays: blocked,
    }
}

#[test]
fn lane6_kind_scope_contains_only_discovery_kinds_for_mixed_interest() {
    // Interest kinds [1, 3] — kind:3 is discovery, kind:1 is content.
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let indexers = vec![INDEXER.to_string()];
    let c = ctx(&cache, &blocked, &indexers);
    let router = GenericOutboxRouter::new();

    let interest = interest_with_kinds(&[1, 3]);
    let out = router
        .route_subscription(&interest, &c)
        .expect("indexer lane resolves a relay");

    // The indexer relay is in the routed set...
    assert!(
        out.urls().any(|u| u == INDEXER),
        "indexer relay must be present for a mixed discovery/content interest"
    );
    // ...and scoped to ONLY the discovery kind (kind:3), not kind:1.
    let scope = out
        .kind_scope_for(&INDEXER.to_string())
        .expect("mixed interest must record a kind scope for the indexer");
    assert_eq!(
        scope,
        &BTreeSet::from([3u32]),
        "indexer kind scope must be exactly the discovery subset {{3}}, not {{1,3}}"
    );
}

#[test]
fn lane6_no_kind_scope_when_all_kinds_are_discovery() {
    // Interest kinds [0, 3] — BOTH are discovery kinds, so no content kind
    // would leak; no per-relay scope override is needed (the relay receives
    // the full, already-discovery-only interest kinds).
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let indexers = vec![INDEXER.to_string()];
    let c = ctx(&cache, &blocked, &indexers);
    let router = GenericOutboxRouter::new();

    let interest = interest_with_kinds(&[0, 3]);
    let out = router
        .route_subscription(&interest, &c)
        .expect("indexer lane resolves a relay");

    assert!(
        out.urls().any(|u| u == INDEXER),
        "indexer relay must be present for an all-discovery interest"
    );
    assert!(
        out.kind_scope_for(&INDEXER.to_string()).is_none(),
        "all-discovery interest must NOT record a kind-scope override (use full kinds)"
    );
}

#[test]
fn lane6_does_not_fire_for_content_only_kinds() {
    // Interest kinds [1, 6] — neither is a discovery kind, so lane 6 never
    // fires and the indexer relay is absent from the routed set. With no
    // author NIP-65 and no app-relay fallback the route is Unroutable.
    let cache = InMemoryMailboxCache::new();
    let blocked = BlockedRelaySet::new();
    let indexers = vec![INDEXER.to_string()];
    let c = ctx(&cache, &blocked, &indexers);
    let router = GenericOutboxRouter::new();

    let interest = interest_with_kinds(&[1, 6]);
    let result = router.route_subscription(&interest, &c);

    match result {
        Ok(out) => assert!(
            !out.urls().any(|u| u == INDEXER),
            "indexer relay must NOT appear for a content-only interest, got {:?}",
            out.urls().collect::<Vec<_>>()
        ),
        // No lane resolved anything → Unroutable is the honest outcome and
        // also satisfies "indexer relay is absent".
        Err(_) => {}
    }
}

// ─── Bug 2 ⇄ Bug 1 — canonicalisation makes the blocked filter match ───────

#[test]
fn blocked_relay_casing_mismatch_is_caught() {
    // The author publishes a kind:10002 with a MIXED-CASE read relay
    // `wss://BLOCKED.EXAMPLE`. The ingest parser canonicalises it to
    // `wss://blocked.example`. A blocked set carrying the canonical lowercase
    // form must therefore exclude it from the subscribe route — before Bug 2's
    // fix the kind:10002 entry stayed mixed-case and never matched the
    // lowercase block, silently defeating the filter.
    let cache = Arc::new(InMemoryMailboxCache::new());
    let parser = Kind10002Parser::new(Arc::clone(&cache));

    let evt = VerifiedEvent::from_raw_unchecked(RawEvent {
        id: "00".repeat(32),
        pubkey: "alice".into(),
        created_at: 0,
        kind: 10_002,
        tags: vec![
            vec!["r".into(), "wss://BLOCKED.EXAMPLE".into(), "read".into()],
            vec!["r".into(), "wss://Good.Example".into(), "read".into()],
        ],
        content: String::new(),
        sig: "22".repeat(64),
    });
    parser.parse_event(&evt);

    // Block the canonical (lowercase) form — the only form the block-list
    // ingest path ever produces.
    let mut blocked = BlockedRelaySet::new();
    blocked.insert("wss://blocked.example".to_string());

    let indexers: Vec<String> = vec![];
    let c = ctx(cache.as_ref(), &blocked, &indexers);
    let router = GenericOutboxRouter::new();

    let interest = LogicalInterest {
        id: InterestId(11),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: ["alice".to_string()].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..InterestShape::default()
        },
        hints: vec![],
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    };

    let out = router
        .route_subscription(&interest, &c)
        .expect("good.example read relay keeps the route routable");
    let urls: Vec<&String> = out.urls().collect();

    assert!(
        urls.iter().any(|u| *u == "wss://good.example"),
        "the unblocked (canonicalised) read relay must survive, got {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| *u == "wss://blocked.example"),
        "canonicalisation must let the lowercase block match the mixed-case \
         kind:10002 entry, got {urls:?}"
    );
}
