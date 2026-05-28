//! Synthetic engine tests. Fixtures (`Harness`, fake resolver/payload/card,
//! event builders) live in `support.rs`; this file holds the behavioral
//! assertions. Covers every arrival case in design §3-J plus the V-81
//! release-signal-is-not-terminal contract.

mod support;

use nmp_threading::pointer::ThreadPointer;

use crate::root_indexed::card::RootFeedSnapshot;
use crate::root_indexed::claim::ClaimRequest;
use crate::root_indexed::engine::MAX_ATTRIBUTION_PER_ROOT;
use crate::FeedRequest;
use support::{
    profile_event, reply_event, repost_event, root_event, Harness, TestCard, TestPayload,
};

#[test]
fn root_first_arrival_surfaces_root() {
    let h = Harness::new(&["alice"]);
    h.ingest(&root_event("op1", "bob", 10, "hello"));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.root_id, "op1");
    assert!(snap.cards[0].attribution.is_empty());
    // A root that is locally present needs no claim.
    assert!(h.claims().is_empty());
}

#[test]
fn reply_before_root_buffers_and_emits_claim() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));

    // No root yet → nothing surfaces.
    assert!(h.snapshot().cards.is_empty());

    // Exactly one Claim for the missing root, carrying an Event pointer.
    let claims = h.claims();
    assert_eq!(claims.len(), 1);
    match &claims[0] {
        ClaimRequest::Claim {
            pointer,
            consumer_id,
            ..
        } => {
            assert_eq!(pointer.event_id(), Some("op1"));
            assert_eq!(consumer_id, "nmp.feed.home");
        }
        other => panic!("expected Claim, got {other:?}"),
    }
}

#[test]
fn reply_from_non_follow_is_dropped() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "mallory", 11, "op1"));

    assert!(h.snapshot().cards.is_empty());
    // Non-follow reply produced no claim and no buffered attribution.
    assert!(h.claims().is_empty());
}

#[test]
fn root_arrival_drains_pending_attribution() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    h.ingest(&root_event("op1", "bob", 10, "hello"));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].attribution.len(), 1);
    assert_eq!(snap.cards[0].attribution[0].author, "alice");

    // Claim emitted for the buffered reply, then Release once the root landed.
    let claims = h.claims();
    assert_eq!(claims.len(), 2);
    assert!(matches!(claims[0], ClaimRequest::Claim { .. }));
    assert!(matches!(claims[1], ClaimRequest::Release { .. }));
}

#[test]
fn profile_refresh_fans_into_attribution() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    // No display name yet.
    assert_eq!(h.snapshot().cards[0].attribution[0].display_name, None);

    h.ingest(&profile_event("alice", "alice", "Alice A."));
    assert_eq!(
        h.snapshot().cards[0].attribution[0].display_name,
        Some("Alice A.".to_string())
    );
}

#[test]
fn profile_refresh_reaches_pending_attribution() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    // Profile arrives while attribution is still pending (root absent).
    h.ingest(&profile_event("alice", "alice", "Alice A."));
    // Root arrives last → drained attribution must already carry the name.
    h.ingest(&root_event("op1", "bob", 10, "hello"));

    assert_eq!(
        h.snapshot().cards[0].attribution[0].display_name,
        Some("Alice A.".to_string())
    );
}

#[test]
fn repost_l1_surfaces_target_and_claims_when_absent() {
    let h = Harness::new(&["alice"]);
    // Followed user reposts an OP we do not hold.
    h.ingest(&repost_event("rp1", "alice", 20, "op1", ""));

    // The target op1 is surfaced as a single root (keyed under op1 even though
    // only the wrapper rp1 is local — the card body is the wrapper's until the
    // target hydrates via L-5).
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);

    // Exactly one Claim, for the absent target op1.
    let claims = h.claims();
    assert_eq!(claims.len(), 1);
    assert_eq!(claims[0].pointer().event_id(), Some("op1"));

    // When op1 lands, L-5 rebuilds the card body and the slot stays single.
    h.ingest(&root_event("op1", "bob", 10, "the real post"));
    let after = h.snapshot();
    assert_eq!(after.cards.len(), 1);
    assert_eq!(after.cards[0].card.root_id, "op1");
    assert_eq!(after.cards[0].card.body, "the real post");
    assert_eq!(after.cards[0].card.reposted_by, Some("alice".to_string()));
}

#[test]
fn repost_l2_reply_to_wrapper_rekeys_to_target() {
    let h = Harness::new(&["alice"]);
    // The kind:6 wrapper rp1 supersedes op1, and is locally available.
    h.store(&repost_event("rp1", "carol", 20, "op1", ""));
    // Alice replies to the wrapper rp1 (root tag points at rp1).
    let mut reply = reply_event("r1", "alice", 21, "rp1");
    reply.tags = vec![
        vec!["root".to_string(), "rp1".to_string()],
        vec!["parent".to_string(), "rp1".to_string()],
    ];
    h.ingest(&reply);
    // The attribution must be re-keyed to op1, so when op1 lands it drains.
    h.ingest(&root_event("op1", "bob", 10, "hello"));

    let snap = h.snapshot();
    let op1 = snap
        .cards
        .iter()
        .find(|c| c.card.root_id == "op1")
        .expect("op1 surfaced");
    assert_eq!(op1.attribution.len(), 1, "attribution re-keyed to op1");
    assert_eq!(op1.attribution[0].reply_id, "r1");
}

#[test]
fn repost_l5_etag_only_hydrates_when_target_arrives() {
    let h = Harness::new(&["alice"]);
    // E-tag-only repost: empty content, target not yet local.
    h.ingest(&repost_event("rp1", "alice", 20, "op1", ""));
    let early = h.snapshot();
    assert_eq!(early.cards[0].card.body, "", "card empty before target");

    // Target arrives later → card rebuilds from the (wrapper, target) pair,
    // hydrating the body AND preserving the repost provenance (L-5 backward).
    h.ingest(&root_event("op1", "bob", 10, "the real post"));
    let late = h.snapshot();
    let op1 = late
        .cards
        .iter()
        .find(|c| c.card.root_id == "op1")
        .expect("op1 surfaced");
    assert_eq!(op1.card.body, "the real post", "card hydrated after target");
    assert_eq!(
        op1.card.reposted_by,
        Some("alice".to_string()),
        "repost provenance survives L-5 backward hydration"
    );
}

#[test]
fn address_pointer_emits_address_claim() {
    let h = Harness::new(&["alice"]);
    let mut reply = reply_event("r1", "alice", 11, "ignored");
    reply.tags = vec![vec![
        "root_addr".to_string(),
        "30023:bob:my-article".to_string(),
    ]];
    h.ingest(&reply);

    let claims = h.claims();
    assert_eq!(claims.len(), 1);
    match claims[0].pointer() {
        ThreadPointer::Address { coord, .. } => assert_eq!(coord, "30023:bob:my-article"),
        other => panic!("expected Address pointer, got {other:?}"),
    }
}

#[test]
fn external_pointer_attaches_surrogate_no_claim() {
    let h = Harness::new(&["alice"]);
    let mut reply = reply_event("r1", "alice", 11, "ignored");
    reply.tags = vec![vec![
        "root_ext".to_string(),
        "https://example.com/post".to_string(),
    ]];
    h.ingest(&reply);

    // External roots are terminal: no claim emitted.
    assert!(h.claims().is_empty());
    // Attribution is buffered against the surrogate (no surfaced card, since
    // an external root is never hydrated into `roots`).
    assert!(h.snapshot().cards.is_empty());
}

#[test]
fn per_root_submap_evicts_oldest_without_release() {
    let h = Harness::new(&["alice"]);
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    // Fill the per-root attribution sub-map beyond its cap.
    let overflow = MAX_ATTRIBUTION_PER_ROOT + 5;
    for i in 0..overflow {
        let reply = reply_event(&format!("r{i}"), "alice", 100 + i as u64, "op1");
        h.ingest(&reply);
    }
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(
        snap.cards[0].attribution.len(),
        MAX_ATTRIBUTION_PER_ROOT,
        "per-root attribution bounded by D5 cap"
    );
    // No Release was emitted: the root is still referenced.
    assert!(h
        .claims()
        .iter()
        .all(|c| !matches!(c, ClaimRequest::Release { .. })));
}

#[test]
fn d5_visible_window_bounds_card_count_and_json() {
    let h = Harness::new(&["alice"]);
    // Populate many roots.
    for i in 0..2_000 {
        h.ingest(&root_event(&format!("op{i}"), "bob", 1_000 + i as u64, "body"));
    }
    let snap = h.engine.snapshot(&FeedRequest::newest(80));
    assert_eq!(snap.cards.len(), 80, "window bounded to request limit");
    assert!(snap.page.as_ref().unwrap().has_more);
    assert_eq!(snap.page.as_ref().unwrap().total_blocks, 2_000);

    // Bounded JSON: 80 small cards must serialize well under a generous bound.
    let json = serde_json::to_string(&snap).unwrap();
    assert!(
        json.len() < 200_000,
        "visible-window JSON is bounded ({} bytes)",
        json.len()
    );
    // Newest-first ordering.
    assert_eq!(snap.cards[0].card.root_id, "op1999");
}

#[test]
fn snapshot_serde_round_trips() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    let snap = h.snapshot();

    let json = serde_json::to_string(&snap).unwrap();
    let restored: RootFeedSnapshot<TestCard, TestPayload> = serde_json::from_str(&json).unwrap();
    assert_eq!(snap, restored);
}

#[test]
fn claim_request_carries_event_thread_pointer() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    let claims = h.claims();
    match &claims[0] {
        ClaimRequest::Claim { pointer, .. } => {
            assert!(matches!(pointer, ThreadPointer::Event { .. }));
            assert_eq!(pointer.event_id(), Some("op1"));
        }
        other => panic!("expected Claim, got {other:?}"),
    }
}

#[test]
fn v81_release_signal_does_not_drop_pending_attribution() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));

    // A Phase-1-EOSE release signal fires for the still-claimed root.
    h.engine.on_event_claim_released(&"op1".to_string());
    assert_eq!(h.engine.released_signals_seen(), 1);

    // V-81: the pending attribution MUST survive — Phase-2 retargeting may
    // still fetch the root. Prove it by delivering the root afterwards and
    // observing the attribution still attaches.
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(
        snap.cards[0].attribution.len(),
        1,
        "release signal must not have evicted the pending attribution (V-81)"
    );
}

#[test]
fn reset_for_identity_change_clears_all_state() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    assert_eq!(h.snapshot().cards.len(), 1);

    h.engine.reset_for_identity_change();
    assert!(h.snapshot().cards.is_empty());
}
