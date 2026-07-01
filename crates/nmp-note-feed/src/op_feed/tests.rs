//! Instance tests for the note-feed OP-feed binding.
//!
//! These drive the *real* `RootIndexedFeed` engine through `register_op_feed`
//! with the real note-feed resolver, `Nip10ReplyAttribution`, and
//! `NoteFeedItem`, against a synthetic kernel read-cache + a recording
//! event lookup. The repost rules L-1…L-5 are exercised through NIP-10 /
//! NIP-18 wire shapes; the engine's generic behaviour is already covered by
//! `nmp-feed`'s synthetic-payload tests, so here we assert the NIP-10 *binding*
//! is correct.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use nmp_core::substrate::{EventId, KernelEvent, SuppressionLookup};
use nmp_core::ObservedProjectionSink;
use nmp_feed::{AttributionPayload, EventLookup, FollowPredicate};

use crate::NoteFeedItem;

use super::attribution::Nip10ReplyAttribution;
use super::test_support::*;
use super::typed_wire::{OP_FEED_FILE_IDENTIFIER, OP_FEED_SCHEMA_ID};
use super::wiring::{op_feed_observer, register_op_feed};

#[derive(Default)]
struct TestSuppression {
    authors: Mutex<HashSet<String>>,
    events: Mutex<HashSet<String>>,
}

impl TestSuppression {
    fn suppress_author(&self, author: &str) {
        self.authors.lock().unwrap().insert(author.to_string());
    }

    fn suppress_event(&self, event_id: &str) {
        self.events.lock().unwrap().insert(event_id.to_string());
    }
}

impl SuppressionLookup for TestSuppression {
    fn is_suppressed_author(&self, author_pubkey: &str) -> bool {
        self.authors
            .lock()
            .map(|authors| authors.contains(author_pubkey))
            .unwrap_or(false)
    }

    fn is_suppressed_event(&self, event_id: &str) -> bool {
        self.events
            .lock()
            .map(|events| events.contains(event_id))
            .unwrap_or(false)
    }
}

struct SuppressionHarness {
    engine: Arc<super::wiring::OpFeedEngine>,
    observer: Arc<super::wiring::OpFeedObserver>,
    lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>>,
    suppression: Arc<TestSuppression>,
}

impl SuppressionHarness {
    fn new(follows: &[&str]) -> Self {
        let follow_set: HashSet<String> = follows.iter().map(|s| (*s).to_string()).collect();
        let follow: FollowPredicate = Arc::new(move |pk: &str| follow_set.contains(pk));
        let lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let lookup_for_cb = Arc::clone(&lookup);
        let event_lookup: EventLookup =
            Arc::new(move |id: &EventId| lookup_for_cb.lock().unwrap().get(id).cloned());
        let suppression = Arc::new(TestSuppression::default());
        let suppression_lookup: Arc<dyn SuppressionLookup> = suppression.clone();
        let engine = register_op_feed(ALICE.to_string(), follow, event_lookup.clone());
        let observer = op_feed_observer(engine.clone(), event_lookup, suppression_lookup);
        Self {
            engine,
            observer,
            lookup,
            suppression,
        }
    }

    fn observe(&self, event: &KernelEvent) {
        self.lookup
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.clone());
        self.observer.on_kernel_event(event);
    }

    fn snapshot(&self) -> nmp_feed::RootFeedSnapshot<NoteFeedItem, Nip10ReplyAttribution> {
        self.engine.snapshot(&nmp_feed::FeedRequest::default())
    }
}

fn delete_event(id: &str, author: &str, target: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 5,
        created_at: 30,
        tags: vec![vec!["e".to_string(), target.to_string()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

// ─── Attribution unit tests ─────────────────────────────────────────────────

#[test]
fn from_reply_requires_kind1_follow_and_reply_marker() {
    let follow = |pk: &str| pk == ALICE;

    // kind:1 reply from a follow → Some.
    let reply = reply_event(REPLY_ID, ALICE, 10, OP_ID);
    let attribution = Nip10ReplyAttribution::from_reply(&reply, &follow);
    let attribution = attribution.expect("reply qualifies");
    assert_eq!(attribution.author_pubkey, ALICE);
    assert_eq!(attribution.reply_event_id(), REPLY_ID);
    assert_eq!(attribution.reply_created_at, 10);

    // non-follow → None.
    assert!(
        Nip10ReplyAttribution::from_reply(&reply_event(REPLY_ID, BOB, 10, OP_ID), &follow)
            .is_none(),
        "non-follow reply dropped"
    );

    // root note (no reply marker) → None.
    assert!(
        Nip10ReplyAttribution::from_reply(&op_event(OP_ID, ALICE, 10, "hi"), &follow).is_none(),
        "root note is not attribution"
    );

    // kind:6 → None (reposts go through the engine's repost arm).
    assert!(
        Nip10ReplyAttribution::from_reply(&repost_etag(REPOST_ID, ALICE, 10, OP_ID), &follow)
            .is_none(),
        "kind:6 is not a reply attribution"
    );
}

#[test]
fn from_reply_uses_raw_author_without_profile_dependency() {
    let follow = |pk: &str| pk == ALICE;

    let reply = reply_event(REPLY_ID, ALICE, 10, OP_ID);
    let attribution = Nip10ReplyAttribution::from_reply(&reply, &follow).expect("qualifies");
    assert_eq!(attribution.author_pubkey, ALICE);
    assert_eq!(attribution.reply_event_id(), REPLY_ID);
}

// ─── Wiring / engine binding tests ──────────────────────────────────────────

#[test]
fn follow_reply_to_unfollowed_op_buffers_until_root_arrives() {
    let h = Harness::new(&[ALICE]); // Alice followed; Bob (OP author) is not.

    // Alice replies to Bob's (not-yet-local) OP.
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID));

    // The attribution is buffered (pending) — no card yet.
    assert!(
        h.snapshot().cards.is_empty(),
        "no root card until OP arrives"
    );

    // Bob's OP arrives through the normal event stream → card surfaces and
    // attribution attaches. The feed does not own any root claim/release.
    h.ingest(&op_event(OP_ID, BOB, 9, "Building with Marmot"));
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    let card = &snap.cards[0];
    assert_eq!(card.card.id, OP_ID);
    assert_eq!(card.card.author_pubkey, BOB);
    assert_eq!(card.attribution.len(), 1);
    assert_eq!(card.attribution[0].author_pubkey, ALICE);
}

#[test]
fn non_follow_reply_is_dropped() {
    let h = Harness::new(&[ALICE]); // Carol not followed.
    h.ingest(&reply_event(REPLY_ID, CAROL, 10, OP_ID));
    assert!(h.snapshot().cards.is_empty());
}

#[test]
fn observer_removes_placeholder_when_suppressed_target_arrives() {
    let h = SuppressionHarness::new(&[ALICE]);
    h.suppression.suppress_author(BOB);

    h.observe(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));
    assert_eq!(
        h.snapshot().cards.len(),
        1,
        "e-tag-only repost can only render a placeholder before target author is known"
    );

    h.observe(&op_event(OP_ID, BOB, 9, "muted target"));
    assert!(
        h.snapshot().cards.is_empty(),
        "target arrival reveals muted author and removes the existing placeholder"
    );
}

#[test]
fn observer_drops_muted_repost_without_removing_visible_target() {
    let h = SuppressionHarness::new(&[ALICE]);
    h.observe(&op_event(OP_ID, BOB, 9, "visible root"));
    h.suppression.suppress_author(ALICE);

    h.observe(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));
    let snapshot = h.snapshot();
    assert_eq!(snapshot.cards.len(), 1);
    assert_eq!(snapshot.cards[0].card.id, OP_ID);
    assert!(
        snapshot.cards[0].card.reposted_by.is_none(),
        "muted reposter must not mutate an already-visible target card"
    );
}

#[test]
fn observer_drops_event_id_suppressed_root_and_repost_target() {
    let h = SuppressionHarness::new(&[ALICE]);
    h.suppression.suppress_event(OP_ID);

    h.observe(&op_event(OP_ID, BOB, 9, "muted by id"));
    assert!(
        h.snapshot().cards.is_empty(),
        "event-id suppressed root must not surface"
    );

    h.observe(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));
    assert!(
        h.snapshot().cards.is_empty(),
        "repost wrapper of an event-id suppressed target must not create a placeholder"
    );
}

#[test]
fn observer_applies_only_author_validated_kind5_deletes() {
    let h = SuppressionHarness::new(&[ALICE]);
    h.observe(&op_event(OP_ID, BOB, 9, "visible root"));

    h.observe(&delete_event(REPLY_ID, CAROL, OP_ID));
    assert_eq!(
        h.snapshot().cards.len(),
        1,
        "foreign kind:5 must not remove someone else's visible row"
    );

    h.observe(&delete_event(REPOST_ID, BOB, OP_ID));
    assert!(
        h.snapshot().cards.is_empty(),
        "author's own kind:5 removes the visible root"
    );
}

#[test]
fn observer_kind5_deletes_repost_row_by_wrapper_id() {
    // A repost-sourced row is keyed by the wrapped TARGET id; deleting the
    // kind:6 wrapper names the WRAPPER id. The delete must still drop the row
    // (validated against the reposter), otherwise a deleted repost keeps
    // surfacing an out-of-perspective target.
    let h = SuppressionHarness::new(&[ALICE]);
    // ALICE (followed) reposts BOB's note. Row surfaces keyed by OP_ID.
    h.observe(&op_event(OP_ID, BOB, 9, "reposted target"));
    h.observe(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.id, OP_ID);

    // A foreign kind:5 naming the wrapper id must not remove the repost row.
    h.observe(&delete_event(REPLY_ID, CAROL, REPOST_ID));
    assert_eq!(
        h.snapshot().cards.len(),
        1,
        "foreign delete of the wrapper id must not remove the repost row"
    );

    // The reposter's own kind:5 naming the wrapper id removes the repost row.
    let del = "0000000000000000000000000000000000000000000000000000000000000de5";
    h.observe(&delete_event(del, ALICE, REPOST_ID));
    assert!(
        h.snapshot().cards.is_empty(),
        "reposter's kind:5 of the wrapper id removes the repost-sourced row"
    );
}

#[test]
fn snapshot_shape_is_root_card_with_raw_attribution() {
    let h = Harness::new(&[ALICE, CAROL]);
    h.ingest(&op_event(OP_ID, BOB, 9, "root body"));
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID));
    let reply2 = "0000000000000000000000000000000000000000000000000000000000000de2";
    h.ingest(&reply_event(reply2, CAROL, 11, OP_ID));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    let card = &snap.cards[0];
    // Raw card data: id, raw pubkey, body.
    assert_eq!(card.card.id, OP_ID);
    assert_eq!(card.card.author_pubkey, BOB);
    assert_eq!(card.card.content, "root body");
    // Two raw attributions (no display formatting baked in).
    assert_eq!(card.attribution.len(), 2);
    let authors: Vec<_> = card
        .attribution
        .iter()
        .map(|a| a.author_pubkey.as_str())
        .collect();
    assert!(authors.contains(&ALICE));
    assert!(authors.contains(&CAROL));
    // Snapshot is JSON-serializable (FFI surface).
    let json = serde_json::to_string(&snap).expect("snapshot serializes");
    assert!(json.contains(OP_ID));
}

// ─── Repost rules L-1 … L-5 (§3-L) ──────────────────────────────────────────

#[test]
fn repost_l1_embedded_surfaces_target_root() {
    // L-1: a follow reposts an OP (embedded note) → the target surfaces as a
    // root card with the repost provenance.
    let h = Harness::new(&[ALICE]);
    let op = op_event(OP_ID, BOB, 9, "Bob's original");
    h.ingest(&repost_embedded(REPOST_ID, ALICE, 20, &op));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1, "target surfaces once");
    let card = &snap.cards[0];
    assert_eq!(card.card.id, OP_ID, "card keyed by the target id");
    assert_eq!(card.card.content, "Bob's original");
    let reposted = card.card.reposted_by.as_ref().expect("repost provenance");
    assert_eq!(reposted.author_pubkey, ALICE, "reposter is the follow");
    // The embedded note renders immediately. The feed does not claim a
    // canonical target copy; a mounted preview/content component owns any
    // stronger target dependency it needs.
}

#[test]
fn repost_l2_reply_to_kind6_wrapper_rekeys_to_target() {
    // L-2: Alice replies to a kind:6 repost wrapper (locally known). The
    // attribution must re-key onto the wrapped target so it attaches to the
    // original note, not the wrapper.
    let h = Harness::new(&[ALICE]);
    // The kind:6 wrapper (e-tag only) is in the read cache.
    let wrapper = repost_etag(REPOST_ID, CAROL, 19, OP_ID);
    h.store(&wrapper);
    // Bob's OP is local so the re-keyed attribution attaches immediately.
    h.ingest(&op_event(OP_ID, BOB, 9, "Bob's original"));

    // Alice replies to the WRAPPER id, not the OP id.
    h.ingest(&reply_to_parent(REPLY_ID, ALICE, 21, REPOST_ID));

    let snap = h.snapshot();
    let target_card = snap
        .cards
        .iter()
        .find(|c| c.card.id == OP_ID)
        .expect("target card present");
    assert_eq!(
        target_card.attribution.len(),
        1,
        "attribution re-keyed onto the wrapped target (L-2)"
    );
    assert_eq!(target_card.attribution[0].author_pubkey, ALICE);
}

#[test]
fn repost_l3_etag_only_surfaces_target_placeholder_without_claiming() {
    // L-3: an e-tag-only repost (no embedded note) of a not-local target →
    // the feed surfaces a placeholder keyed by the target id. It does not
    // claim the target; a mounted row component can do that if it needs the
    // target body immediately.
    let h = Harness::new(&[ALICE]);
    h.ingest(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));

    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].card.id, OP_ID);
}

#[test]
fn repost_l4_multiple_reposts_of_same_target_render_once() {
    // L-4: two follows repost the same target → it surfaces once (keyed by the
    // target id), not twice.
    let h = Harness::new(&[ALICE, CAROL]);
    let op = op_event(OP_ID, BOB, 9, "Bob's original");
    h.ingest(&repost_embedded(REPOST_ID, ALICE, 20, &op));
    let repost2 = "0000000000000000000000000000000000000000000000000000000000000f07";
    h.ingest(&repost_embedded(repost2, CAROL, 21, &op));

    let snap = h.snapshot();
    let target_cards: Vec<_> = snap.cards.iter().filter(|c| c.card.id == OP_ID).collect();
    assert_eq!(target_cards.len(), 1, "target renders exactly once (L-4)");
}

#[test]
fn repost_l5_etag_target_hydrates_later_rebuilds_card() {
    // L-5: an e-tag-only repost arrives first (placeholder),
    // then the target kind:1 arrives later → the card body hydrates while
    // keeping the repost provenance (the engine re-fetches the wrapper via
    // `wrapper_event_id` and rebuilds from the `(wrapper, target)` pair).
    let h = Harness::new(&[ALICE]);
    h.ingest(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));

    // Placeholder card present, body empty (no inner note yet).
    let snap = h.snapshot();
    assert_eq!(snap.cards[0].card.id, OP_ID);
    assert!(
        snap.cards[0].card.content.is_empty(),
        "placeholder body before target arrives"
    );

    // The target kind:1 arrives.
    h.ingest(&op_event(OP_ID, BOB, 9, "the real body"));
    let snap = h.snapshot();
    let card = &snap.cards[0];
    assert_eq!(card.card.id, OP_ID);
    assert_eq!(
        card.card.content, "the real body",
        "card body hydrated (L-5)"
    );
    let reposted = card
        .card
        .reposted_by
        .as_ref()
        .expect("repost provenance kept");
    assert_eq!(reposted.author_pubkey, ALICE);
}

#[test]
fn op_feed_owns_schema_not_projection_key() {
    assert_eq!(OP_FEED_SCHEMA_ID, "nmp.note_feed.opfeed");
    assert_eq!(OP_FEED_FILE_IDENTIFIER, b"NNFS");
}

#[test]
fn pending_reply_survives_until_root_arrives() {
    // Missing-root buffering is owned by the feed's local state, not by kernel
    // event-claim lifetime signals.
    let h = Harness::new(&[ALICE]);
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID)); // buffers a pending attribution

    // The OP later arrives → the pending attribution still attaches, proving
    // absent-root buffering is not coupled to claim/release state.
    h.ingest(&op_event(OP_ID, BOB, 9, "arrived after release signal"));
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].attribution.len(), 1);
}

#[test]
fn identity_reset_clears_pending_feed_state_without_claims() {
    let h = Harness::new(&[ALICE]);
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID));

    h.engine.reset_for_identity_change();

    assert!(
        h.snapshot().cards.is_empty(),
        "identity reset clears OP-feed snapshot state"
    );
}
