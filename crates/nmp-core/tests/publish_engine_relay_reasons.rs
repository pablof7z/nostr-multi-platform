//! Integration tests for the per-relay rationale carried through the publish
//! engine snapshot projection (Steps 1–4 of
//! `docs/architecture-audit/per-relay-publish-status-rationale.md`).
//!
//! The publish-engine surface tested here is:
//!   - `InFlight.relay_reasons` is populated from `OutboxResolver::resolve()`
//!     once, at publish time.
//!   - The reason survives a `mark_relay_unavailable()` / `mark_relay_available()`
//!     cycle unchanged (write-once at publish time, never mutated by retry /
//!     availability logic).
//!   - The snapshot's `EventPublishStatus.relay_reasons` carries the same
//!     map (parallel key set to `per_relay`).
//!   - When the resolver returns the same canonical URL with multiple
//!     distinct reasons, the engine collects them into a `Vec` so the
//!     projection surfaces both rationales (e.g. NIP-65 write relay AND a
//!     discovery indexer for kind:0).
//!
//! These live in `tests/` (not in-crate) because they exercise the public
//! engine API and the public projection contract. No relay sockets are used;
//! the dispatcher is `ReplayDispatcher` and time is injected.

use std::sync::Arc;

use nmp_core::publish::{
    InMemoryPublishStore, NoopSigner, OutboxResolver, PublishAction, PublishEngine, PublishStore,
    PublishTarget, RelayDispatcher, RelaySelectionReason, RelayUrl, ReplayDispatcher,
    ResolvedRelay, RetryPolicy,
};
use nmp_core::substrate::{BlockedRelaySet, SignedEvent, UnsignedEvent};

fn signed(id: &str, author: &str, kind: u32) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{}", id),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: format!("content-{}", id),
            created_at: 1_700_000_000,
        },
    }
}

fn engine(
    outbox: Arc<dyn OutboxResolver>,
    dispatcher: Arc<ReplayDispatcher>,
    store: Arc<dyn PublishStore>,
) -> PublishEngine {
    PublishEngine::new(
        outbox,
        dispatcher as Arc<dyn RelayDispatcher>,
        store,
        Arc::new(NoopSigner),
        RetryPolicy::default(),
    )
}

/// Test resolver — returns a pre-baked `Vec<ResolvedRelay>` regardless of
/// input. Lets a single test inject "this URL appears twice with different
/// reasons" without needing the full NIP-65 lookup chain.
struct FixedResolver(Vec<ResolvedRelay>);

impl OutboxResolver for FixedResolver {
    fn resolve(
        &self,
        _author_pubkey: &str,
        _p_tags: &[String],
        target: &PublishTarget,
        _kind: u32,
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        if let PublishTarget::Explicit { relays } = target {
            return relays
                .iter()
                .filter(|url| !blocked.contains(url))
                .map(|url| ResolvedRelay {
                    url: url.clone(),
                    reason: RelaySelectionReason::Explicit,
                })
                .collect();
        }
        self.0
            .iter()
            .filter(|r| !blocked.contains(&r.url))
            .cloned()
            .collect()
    }
}

/// `InFlight.relay_reasons` is populated at publish time and surfaced on the
/// snapshot's `EventPublishStatus.relay_reasons` field with the exact reason
/// variants the resolver returned.
#[test]
fn relay_reasons_are_threaded_from_resolver_through_snapshot() {
    let outbox = Arc::new(FixedResolver(vec![
        ResolvedRelay {
            url: "wss://write.example".to_string(),
            reason: RelaySelectionReason::AuthorWriteRelay,
        },
        ResolvedRelay {
            url: "wss://inbox.example".to_string(),
            reason: RelaySelectionReason::RecipientInbox {
                pubkey: "abc".repeat(21) + "a", // 64-char-ish placeholder
            },
        },
    ]));
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut engine = engine(outbox, dispatcher, store);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "h-reasons".to_string(),
                event: signed("ev-reasons", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .expect("publish accepted by engine");

    let snapshot = engine.snapshot();
    assert_eq!(
        snapshot.in_flight.len(),
        1,
        "exactly one in-flight publish row"
    );
    let row = &snapshot.in_flight[0];
    let reasons: std::collections::BTreeMap<RelayUrl, Vec<RelaySelectionReason>> =
        row.relay_reasons.iter().cloned().collect();
    assert!(
        matches!(
            reasons.get("wss://write.example").map(Vec::as_slice),
            Some([RelaySelectionReason::AuthorWriteRelay])
        ),
        "write-relay reason must thread through to the snapshot verbatim, got {:?}",
        reasons.get("wss://write.example"),
    );
    assert!(
        matches!(
            reasons.get("wss://inbox.example").map(Vec::as_slice),
            Some([RelaySelectionReason::RecipientInbox { .. }])
        ),
        "inbox-relay reason must thread through to the snapshot verbatim, got {:?}",
        reasons.get("wss://inbox.example"),
    );
    // Per-relay state map keys MUST be a subset of the relay-reasons keys —
    // every relay tracked for status carries an annotated rationale.
    for (url, _state) in &row.per_relay {
        assert!(
            reasons.contains_key(url),
            "every per-relay key needs a reason: {url}"
        );
    }
}

/// Engine-side dedup: when the resolver emits the same canonical URL with two
/// distinct reasons (e.g. a relay that is BOTH the author's NIP-65 write
/// relay AND a discovery indexer for kind:0), the engine deduplicates the
/// URL and collects both reasons into the `Vec` so the projection carries
/// both. A future regression that silently drops one reason would surface
/// here.
#[test]
fn relay_reasons_merge_when_same_url_appears_with_two_reasons() {
    let outbox = Arc::new(FixedResolver(vec![
        ResolvedRelay {
            url: "wss://shared.example".to_string(),
            reason: RelaySelectionReason::AuthorWriteRelay,
        },
        ResolvedRelay {
            url: "wss://shared.example".to_string(),
            reason: RelaySelectionReason::DiscoveryIndexer { kind: 0 },
        },
    ]));
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut engine = engine(outbox, dispatcher, store);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "h-merge".to_string(),
                event: signed("ev-merge", "alice", 0),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .expect("publish accepted by engine");

    let snapshot = engine.snapshot();
    let row = &snapshot.in_flight[0];
    // Exactly one per-relay entry (the URL was deduplicated by canonical
    // identity) and exactly one reasons entry carrying both rationales in
    // the Vec.
    assert_eq!(
        row.per_relay.len(),
        1,
        "same canonical URL must dedupe to one per-relay entry"
    );
    assert_eq!(
        row.relay_reasons.len(),
        1,
        "same canonical URL must dedupe to one reasons entry"
    );
    let (url, reasons) = &row.relay_reasons[0];
    assert_eq!(url, "wss://shared.example");
    assert_eq!(
        reasons.len(),
        2,
        "both rationales must be preserved: {reasons:?}",
    );
    assert!(
        reasons.contains(&RelaySelectionReason::AuthorWriteRelay),
        "merged reasons must retain AuthorWriteRelay: {reasons:?}",
    );
    assert!(
        reasons.contains(&RelaySelectionReason::DiscoveryIndexer { kind: 0 }),
        "merged reasons must retain DiscoveryIndexer{{kind:0}}: {reasons:?}",
    );
}

/// Duplicate reasons are NOT pushed twice — the engine checks for an
/// existing identical variant before appending. Without this guard a relay
/// that the resolver listed twice with the same rationale would produce a
/// noisy `[Reason, Reason]` projection.
#[test]
fn relay_reasons_do_not_duplicate_identical_reasons() {
    let outbox = Arc::new(FixedResolver(vec![
        ResolvedRelay {
            url: "wss://dup.example".to_string(),
            reason: RelaySelectionReason::AuthorWriteRelay,
        },
        ResolvedRelay {
            url: "wss://dup.example".to_string(),
            reason: RelaySelectionReason::AuthorWriteRelay,
        },
    ]));
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut engine = engine(outbox, dispatcher, store);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "h-dup".to_string(),
                event: signed("ev-dup", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .expect("publish accepted by engine");

    let snapshot = engine.snapshot();
    let row = &snapshot.in_flight[0];
    let (_url, reasons) = &row.relay_reasons[0];
    assert_eq!(
        reasons.as_slice(),
        &[RelaySelectionReason::AuthorWriteRelay],
        "duplicate identical reasons must NOT be appended to themselves"
    );
}

/// `mark_relay_unavailable` followed by `mark_relay_available` must leave the
/// per-relay rationale unchanged. The reason is captured once, at publish
/// time, and is never mutated by retry / availability transitions — the
/// snapshot's `relay_reasons` is the single source of truth for "why".
#[test]
fn relay_reason_survives_availability_cycle() {
    let outbox = Arc::new(FixedResolver(vec![ResolvedRelay {
        url: "wss://oscillating.example".to_string(),
        reason: RelaySelectionReason::AuthorWriteRelay,
    }]));
    let dispatcher = Arc::new(ReplayDispatcher::new());
    let store: Arc<dyn PublishStore> = Arc::new(InMemoryPublishStore::new());
    let mut engine = engine(outbox, dispatcher, store);

    engine
        .start_publish(
            PublishAction::Publish {
                handle: "h-osc".to_string(),
                event: signed("ev-osc", "alice", 1),
                target: PublishTarget::Auto,
            },
            100,
            None,
        )
        .expect("publish accepted by engine");

    // Initial snapshot carries the resolved reason.
    let reasons_before: Vec<RelaySelectionReason> = engine
        .snapshot()
        .in_flight
        .iter()
        .find(|row| row.handle == "h-osc")
        .expect("publish row must be present")
        .relay_reasons
        .iter()
        .find(|(url, _)| url == "wss://oscillating.example")
        .map(|(_, r)| r.clone())
        .expect("relay must carry a reason after publish");

    // Cycle the relay: in-flight → unavailable → available again.
    engine
        .mark_relay_unavailable("wss://oscillating.example", 200)
        .expect("mark unavailable");
    engine
        .mark_relay_available("wss://oscillating.example", 300)
        .expect("mark available");

    let reasons_after: Vec<RelaySelectionReason> = engine
        .snapshot()
        .in_flight
        .iter()
        .find(|row| row.handle == "h-osc")
        .expect("publish row must persist through the cycle")
        .relay_reasons
        .iter()
        .find(|(url, _)| url == "wss://oscillating.example")
        .map(|(_, r)| r.clone())
        .expect("relay must still carry its reason after the availability cycle");

    assert_eq!(
        reasons_before, reasons_after,
        "reason is write-once at publish time — availability transitions must NOT mutate it"
    );
}
