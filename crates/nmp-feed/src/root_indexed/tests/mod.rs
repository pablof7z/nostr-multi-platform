//! Synthetic engine tests. Fixtures (`Harness`, fake resolver/payload/card,
//! event builders) live in `support.rs`; this file holds the behavioral
//! assertions. Covers every arrival case in design §3-J plus the V-81
//! release-signal-is-not-terminal contract.

mod pagination_ordering;
mod root_admission;
mod support;

use crate::root_indexed::card::RootFeedSnapshot;
use crate::root_indexed::engine::MAX_ATTRIBUTION_PER_ROOT;
use crate::{DEFAULT_FEED_WINDOW_LIMIT, FeedRequest};
use support::{Harness, TestCard, TestPayload, reply_event, repost_event, root_event};

#[test]
fn root_first_arrival_surfaces_root() {
    let h = Harness::new(&["alice"]);
    h.ingest(&root_event("op1", "bob", 10, "hello"));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.root_id, "op1");
    assert!(snap.cards[0].attribution.is_empty());
}

#[test]
fn reply_before_root_buffers_without_claiming_secondary_data() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));

    // No root yet → nothing surfaces.
    assert!(h.snapshot().cards.is_empty());
}

#[test]
fn reply_from_non_follow_is_dropped() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "mallory", 11, "op1"));

    assert!(h.snapshot().cards.is_empty());
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
}

#[test]
fn followed_reply_hydrates_cached_root_without_broad_observer_delivery() {
    let h = Harness::new(&["alice"]);
    h.store(&root_event("op1", "bob", 10, "hello"));
    h.ingest(&reply_event("r1", "alice", 11, "op1"));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.root_id, "op1");
    assert_eq!(snap.cards[0].attribution.len(), 1);
    assert_eq!(snap.cards[0].attribution[0].author, "alice");
}

#[test]
fn repost_l1_surfaces_target_placeholder_without_claiming_when_absent() {
    let h = Harness::new(&["alice"]);
    // Followed user reposts an OP we do not hold.
    h.ingest(&repost_event("rp1", "alice", 20, "op1", ""));

    // The target op1 is surfaced as a single root (keyed under op1 even though
    // only the wrapper rp1 is local — the card body is the wrapper's until the
    // target arrives via L-5).
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.root_id, "op1");

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
fn repost_l5_etag_only_rebuilds_when_target_arrives() {
    let h = Harness::new(&["alice"]);
    // E-tag-only repost: empty content, target not yet local.
    h.ingest(&repost_event("rp1", "alice", 20, "op1", ""));
    let early = h.snapshot();
    assert_eq!(early.cards[0].card.body, "", "card empty before target");

    // Target arrives later → card rebuilds from the (wrapper, target) pair,
    // rebuilding the body AND preserving the repost provenance (L-5).
    h.ingest(&root_event("op1", "bob", 10, "the real post"));
    let late = h.snapshot();
    let op1 = late
        .cards
        .iter()
        .find(|c| c.card.root_id == "op1")
        .expect("op1 surfaced");
    assert_eq!(op1.card.body, "the real post", "card rebuilt after target");
    assert_eq!(
        op1.card.reposted_by,
        Some("alice".to_string()),
        "repost provenance survives L-5 rebuild"
    );
}

#[test]
fn address_pointer_buffers_without_claiming() {
    let h = Harness::new(&["alice"]);
    let mut reply = reply_event("r1", "alice", 11, "ignored");
    reply.tags = vec![vec![
        "root_addr".to_string(),
        "30023:bob:my-article".to_string(),
    ]];
    h.ingest(&reply);

    assert!(h.snapshot().cards.is_empty());
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
}

#[test]
fn d5_visible_window_bounds_card_count_and_json() {
    let h = Harness::new(&["alice"]);
    // Populate many roots.
    for i in 0..2_000 {
        h.ingest(&root_event(
            &format!("op{i}"),
            "bob",
            1_000 + i as u64,
            "body",
        ));
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
fn reset_for_identity_change_clears_all_state() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    assert_eq!(h.snapshot().cards.len(), 1);

    h.engine.reset_for_identity_change();
    assert!(h.snapshot().cards.is_empty());
}

#[test]
fn remove_root_drops_card_and_attribution_state() {
    let h = Harness::new(&["alice"]);
    h.ingest(&reply_event("r1", "alice", 11, "op1"));
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    assert_eq!(h.snapshot().cards[0].attribution.len(), 1);

    assert!(h.engine.remove_root("op1"));
    assert!(h.snapshot().cards.is_empty());

    h.ingest(&root_event("op1", "bob", 12, "again"));
    assert!(
        h.snapshot().cards[0].attribution.is_empty(),
        "stale attribution state must not survive root removal"
    );
}

#[test]
fn remove_root_if_keeps_card_when_predicate_rejects() {
    let h = Harness::new(&["alice"]);
    h.ingest(&root_event("op1", "bob", 10, "hello"));

    assert!(!h.engine.remove_root_if("op1", |card| card.body == "nope"));
    assert_eq!(h.snapshot().cards.len(), 1);
}

#[test]
fn perspective_reset_restores_default_window_limit() {
    let h = Harness::new(&["alice"]);
    for i in 0u64..90 {
        h.ingest(&root_event(
            &format!("old{i}"),
            "alice",
            1_000 + i,
            "old body",
        ));
    }
    assert!(
        h.engine.grow_visible_window(),
        "precondition: visible window grew past the default"
    );
    assert_eq!(h.engine.snapshot_current_window().cards.len(), 90);

    h.engine.reset_for_perspective_change();

    for i in 0u64..90 {
        h.ingest(&root_event(
            &format!("new{i}"),
            "alice",
            2_000 + i,
            "new body",
        ));
    }
    assert_eq!(
        h.engine.snapshot_current_window().cards.len(),
        DEFAULT_FEED_WINDOW_LIMIT,
        "a perspective reset must return paging to the first window"
    );
}

#[test]
fn gated_out_kind_never_becomes_a_phantom_root() {
    use std::sync::Arc;

    use nmp_core::substrate::KernelEvent;

    // Gate admits kind:1 only — modelling the NIP-10 wiring that drops kind:3
    // (contacts) and kind:10002 (relay list) echoes from account creation.
    let h = Harness::with_gate(&["alice"], Arc::new(|e: &KernelEvent| e.kind == 1));

    // A kind:3 echo from the freshly-created account: root-shaped (no parent
    // tags) so without the gate it would become a phantom root card.
    let contacts = KernelEvent {
        id: "contacts1".to_string(),
        author: "self".to_string(),
        kind: 3,
        created_at: 5,
        tags: Vec::new(),
        content: String::new(),
        relay_provenance: Vec::new(),
    };
    h.ingest(&contacts);

    // Gate dropped it before any state was touched: no card.
    assert!(h.snapshot().cards.is_empty());

    // A real kind:1 root still surfaces, proving the gate only filters kinds.
    h.ingest(&root_event("op1", "bob", 10, "hello"));
    assert_eq!(h.snapshot().cards.len(), 1);
}
