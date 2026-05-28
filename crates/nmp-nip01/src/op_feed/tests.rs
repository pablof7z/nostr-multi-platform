//! Instance tests for the NIP-10 OP-feed binding (V-80 rung 5).
//!
//! These drive the *real* `RootIndexedFeed` engine through `register_op_feed`
//! with the *real* `Nip10Resolver`, `Nip10ReplyAttribution`, and
//! `TimelineEventCard`, against a synthetic kernel read-cache + a recording
//! claim dispatcher. The repost rules L-1…L-5 (§3-L) are exercised through
//! NIP-10 / NIP-18 wire shapes; the engine's generic behaviour is already
//! covered by `nmp-feed`'s synthetic-payload tests, so here we assert the
//! NIP-10 *binding* is correct.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nmp_core::nip19::decode_nevent;
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::{ActorCommand, KernelEventObserver};
use nmp_feed::{AttributionPayload, EventLookup, FeedRequest, FollowPredicate};

use super::attribution::Nip10ReplyAttribution;
use super::wiring::{build_actor_claim_sink, register_op_feed, OpFeedEngine, OP_FEED_SNAPSHOT_KEY};
use crate::profile_display::ProfileDisplay;

const ALICE: &str = "aaaa000000000000000000000000000000000000000000000000000000000001";
const BOB: &str = "bbbb000000000000000000000000000000000000000000000000000000000002";
const CAROL: &str = "cccc000000000000000000000000000000000000000000000000000000000003";

// 64-hex event ids so the nevent encoder (32-byte TLV) accepts them.
const OP_ID: &str = "0000000000000000000000000000000000000000000000000000000000000abc";
const REPLY_ID: &str = "0000000000000000000000000000000000000000000000000000000000000de1";
const REPOST_ID: &str = "0000000000000000000000000000000000000000000000000000000000000f06";

// ─── Harness ────────────────────────────────────────────────────────────────

/// A recorded actor command — `ActorCommand` is not `Clone`, so the test
/// dispatcher captures only the fields the assertions care about.
#[derive(Clone, Debug, PartialEq)]
enum RecordedCmd {
    Claim { uri: String, consumer_id: String },
    Release { uri: String, consumer_id: String },
}

struct Harness {
    engine: Arc<OpFeedEngine>,
    claims: Arc<Mutex<Vec<RecordedCmd>>>,
    lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>>,
}

impl Harness {
    fn new(follows: &[&str]) -> Self {
        let follow_set: std::collections::HashSet<String> =
            follows.iter().map(|s| (*s).to_string()).collect();
        let follow: FollowPredicate = Arc::new(move |pk: &str| follow_set.contains(pk));

        let lookup: Arc<Mutex<HashMap<EventId, KernelEvent>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let lookup_for_cb = Arc::clone(&lookup);
        let event_lookup: EventLookup =
            Arc::new(move |id: &EventId| lookup_for_cb.lock().unwrap().get(id).cloned());

        let claims: Arc<Mutex<Vec<RecordedCmd>>> = Arc::new(Mutex::new(Vec::new()));
        let claims_for_cb = Arc::clone(&claims);
        let dispatch: super::wiring::ActorCommandDispatch = Arc::new(move |cmd| {
            let recorded = match cmd {
                ActorCommand::ClaimEvent { uri, consumer_id } => {
                    RecordedCmd::Claim { uri, consumer_id }
                }
                ActorCommand::ReleaseEvent { uri, consumer_id } => {
                    RecordedCmd::Release { uri, consumer_id }
                }
                _ => return,
            };
            claims_for_cb.lock().unwrap().push(recorded);
        });
        let claim_sink = build_actor_claim_sink(dispatch);

        let engine = register_op_feed(ALICE.to_string(), follow, event_lookup, claim_sink);
        Self {
            engine,
            claims,
            lookup,
        }
    }

    /// Feed an event the way the kernel observer fan-out would: it is in the
    /// read cache AND the observer fires.
    fn ingest(&self, event: &KernelEvent) {
        self.lookup
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.clone());
        self.engine.on_kernel_event(event);
    }

    /// Store an event in the read cache WITHOUT firing the observer — models a
    /// kernel-resolved event the engine can look up but has not observed.
    fn store(&self, event: &KernelEvent) {
        self.lookup
            .lock()
            .unwrap()
            .insert(event.id.clone(), event.clone());
    }

    fn claims(&self) -> Vec<RecordedCmd> {
        self.claims.lock().unwrap().clone()
    }

    fn snapshot(
        &self,
    ) -> nmp_feed::RootFeedSnapshot<
        crate::timeline_projection::TimelineEventCard,
        Nip10ReplyAttribution,
    > {
        self.engine.snapshot(&FeedRequest::default())
    }
}

// ─── Event builders ───────────────────────────────────────────────────────

fn op_event(id: &str, author: &str, created_at: u64, body: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: Vec::new(),
        content: body.to_string(),
    }
}

fn reply_event(id: &str, author: &str, created_at: u64, root_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "root".to_string(),
            ],
            vec![
                "e".to_string(),
                root_id.to_string(),
                String::new(),
                "reply".to_string(),
            ],
        ],
        content: "a reply".to_string(),
    }
}

/// A NIP-10 reply whose reply marker points at `parent_id` (used for L-2: the
/// parent is a kind:6 wrapper).
fn reply_to_parent(id: &str, author: &str, created_at: u64, parent_id: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 1,
        created_at,
        tags: vec![vec![
            "e".to_string(),
            parent_id.to_string(),
            String::new(),
            "reply".to_string(),
        ]],
        content: "reply to a repost".to_string(),
    }
}

/// An e-tag-only kind:6 repost of `target` (no embedded note → L-3 / L-5).
fn repost_etag(id: &str, author: &str, created_at: u64, target: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target.to_string()]],
        content: String::new(),
    }
}

/// A kind:6 repost with the original note embedded in `content` (L-1).
fn repost_embedded(id: &str, author: &str, created_at: u64, target: &KernelEvent) -> KernelEvent {
    let embedded = serde_json::json!({
        "id": target.id,
        "pubkey": target.author,
        "kind": target.kind,
        "created_at": target.created_at,
        "tags": target.tags,
        "content": target.content,
    });
    KernelEvent {
        id: id.to_string(),
        author: author.to_string(),
        kind: 6,
        created_at,
        tags: vec![vec!["e".to_string(), target.id.clone()]],
        content: embedded.to_string(),
    }
}

fn profile_event(author: &str, created_at: u64, display_name: &str) -> KernelEvent {
    KernelEvent {
        id: format!("profile-{author}"),
        author: author.to_string(),
        kind: 0,
        created_at,
        tags: Vec::new(),
        content: serde_json::json!({ "display_name": display_name }).to_string(),
    }
}

fn claimed_event_ids(claims: &[RecordedCmd]) -> Vec<String> {
    claims
        .iter()
        .filter_map(|c| match c {
            RecordedCmd::Claim { uri, .. } => Some(uri.clone()),
            RecordedCmd::Release { .. } => None,
        })
        .collect()
}

/// Assert a claim URI is a `nostr:nevent…` carrying exactly `event_id`.
fn assert_nevent_for(uri: &str, event_id: &str) {
    let bech = uri.strip_prefix("nostr:").expect("nostr: prefix");
    assert!(bech.starts_with("nevent1"), "expected nevent, got {bech}");
    let data = decode_nevent(bech).expect("decodes nevent");
    assert_eq!(data.event_id, event_id);
}

// ─── Attribution unit tests ─────────────────────────────────────────────────

#[test]
fn from_reply_requires_kind1_follow_and_reply_marker() {
    let follow = |pk: &str| pk == ALICE;
    let no_profile = |_: &str| None;

    // kind:1 reply from a follow → Some.
    let reply = reply_event(REPLY_ID, ALICE, 10, OP_ID);
    let attribution = Nip10ReplyAttribution::from_reply(&reply, &follow, &no_profile);
    let attribution = attribution.expect("reply qualifies");
    assert_eq!(attribution.author_pubkey(), ALICE);
    assert_eq!(attribution.reply_event_id(), REPLY_ID);
    assert_eq!(attribution.reply_created_at(), 10);
    assert_eq!(attribution.author_display_name, None);

    // non-follow → None.
    assert!(
        Nip10ReplyAttribution::from_reply(
            &reply_event(REPLY_ID, BOB, 10, OP_ID),
            &follow,
            &no_profile
        )
        .is_none(),
        "non-follow reply dropped"
    );

    // root note (no reply marker) → None.
    assert!(
        Nip10ReplyAttribution::from_reply(&op_event(OP_ID, ALICE, 10, "hi"), &follow, &no_profile)
            .is_none(),
        "root note is not attribution"
    );

    // kind:6 → None (reposts go through the engine's repost arm).
    assert!(
        Nip10ReplyAttribution::from_reply(
            &repost_etag(REPOST_ID, ALICE, 10, OP_ID),
            &follow,
            &no_profile
        )
        .is_none(),
        "kind:6 is not a reply attribution"
    );
}

#[test]
fn from_reply_mirrors_profile_then_refresh_updates_in_place() {
    let follow = |pk: &str| pk == ALICE;
    let profile = ProfileDisplay {
        display: Some("Alice A.".to_string()),
        picture_url: Some("https://example.com/a.png".to_string()),
        created_at: 5,
        event_id: "p1".to_string(),
    };
    let profile_for = |pk: &str| (pk == ALICE).then(|| profile.clone());

    let reply = reply_event(REPLY_ID, ALICE, 10, OP_ID);
    let mut attribution =
        Nip10ReplyAttribution::from_reply(&reply, &follow, &profile_for).expect("qualifies");
    assert_eq!(attribution.author_display_name.as_deref(), Some("Alice A."));
    assert_eq!(
        attribution.author_picture_url.as_deref(),
        Some("https://example.com/a.png")
    );

    // refresh_for_profile updates the mirrors in place without touching keys.
    let newer = ProfileDisplay {
        display: Some("Alice Renamed".to_string()),
        picture_url: None,
        created_at: 20,
        event_id: "p2".to_string(),
    };
    attribution.refresh_for_profile(&newer);
    assert_eq!(
        attribution.author_display_name.as_deref(),
        Some("Alice Renamed")
    );
    assert_eq!(attribution.author_picture_url, None);
    assert_eq!(attribution.author_pubkey(), ALICE);
    assert_eq!(attribution.reply_event_id(), REPLY_ID);
}

// ─── Wiring / engine binding tests ──────────────────────────────────────────

#[test]
fn follow_reply_to_unfollowed_op_emits_claim_with_correct_nevent_then_attaches() {
    let h = Harness::new(&[ALICE]); // Alice followed; Bob (OP author) is not.

    // Alice replies to Bob's (not-yet-local) OP.
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID));

    // A claim went out for the OP, encoded as nostr:nevent carrying OP_ID.
    let claimed = claimed_event_ids(&h.claims());
    assert_eq!(claimed.len(), 1, "exactly one claim for the missing OP");
    assert_nevent_for(&claimed[0], OP_ID);

    // The attribution is buffered (pending) — no card yet.
    assert!(
        h.snapshot().cards.is_empty(),
        "no root card until OP arrives"
    );

    // Bob's OP arrives → card surfaces, attribution attaches, Release emitted.
    h.ingest(&op_event(OP_ID, BOB, 9, "Building with Marmot"));
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    let card = &snap.cards[0];
    assert_eq!(card.card.id, OP_ID);
    assert_eq!(card.card.author_pubkey, BOB);
    assert_eq!(card.attribution.len(), 1);
    assert_eq!(card.attribution[0].author_pubkey, ALICE);

    let released: Vec<_> = h
        .claims()
        .into_iter()
        .filter(|c| matches!(c, RecordedCmd::Release { .. }))
        .collect();
    assert_eq!(released.len(), 1, "Release emitted once OP is local");
}

#[test]
fn profile_refresh_updates_buffered_attribution() {
    let h = Harness::new(&[ALICE]);
    // Alice replies to Bob's OP (buffered, pending).
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID));
    // Bob's OP arrives, attaching the attribution.
    h.ingest(&op_event(OP_ID, BOB, 9, "hi"));
    // Alice's kind:0 arrives → the attribution row's display refreshes.
    h.ingest(&profile_event(ALICE, 11, "Alice A."));

    let snap = h.snapshot();
    let attribution = &snap.cards[0].attribution[0];
    assert_eq!(attribution.author_display_name.as_deref(), Some("Alice A."));
}

#[test]
fn non_follow_reply_is_dropped() {
    let h = Harness::new(&[ALICE]); // Carol not followed.
    h.ingest(&reply_event(REPLY_ID, CAROL, 10, OP_ID));
    assert!(h.claims().is_empty(), "no claim for a non-follow reply");
    assert!(h.snapshot().cards.is_empty());
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
    // The embedded note renders immediately, but the canonical target event is
    // not in the kernel read-cache, so the engine claims it (to fetch the
    // authoritative copy). The claim is a nostr:nevent for the target id.
    let claimed = claimed_event_ids(&h.claims());
    assert_eq!(claimed.len(), 1, "claim the canonical target");
    assert_nevent_for(&claimed[0], OP_ID);
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
fn repost_l3_etag_only_surfaces_target_and_claims_it() {
    // L-3: an e-tag-only repost (no embedded note) of a not-local target →
    // the engine claims the target.
    let h = Harness::new(&[ALICE]);
    h.ingest(&repost_etag(REPOST_ID, ALICE, 20, OP_ID));

    let claimed = claimed_event_ids(&h.claims());
    assert_eq!(claimed.len(), 1, "claim the e-tag-only target");
    assert_nevent_for(&claimed[0], OP_ID);
    // A placeholder card is keyed by the target id meanwhile.
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
    // L-5: an e-tag-only repost arrives first (placeholder, claim emitted),
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
fn op_feed_snapshot_key_matches_chirp_home_key() {
    // The instance registers under the standard home-feed key (rung 7 swap).
    assert_eq!(OP_FEED_SNAPSHOT_KEY, "nmp.feed.home");
}

#[test]
fn release_signal_is_non_terminal_pending_survives() {
    // V-81: an event_claim_released signal must NOT drop a pending attribution
    // (Phase-1 EOSE is not the final give-up). Assert via the diagnostic
    // counter, not by checking pending is gone.
    let h = Harness::new(&[ALICE]);
    h.ingest(&reply_event(REPLY_ID, ALICE, 10, OP_ID)); // buffers a pending attribution

    h.engine.on_event_claim_released(&OP_ID.to_string());
    assert_eq!(h.engine.released_signals_seen(), 1);

    // The OP later arrives → the pending attribution still attaches, proving
    // the release signal did not drop it.
    h.ingest(&op_event(OP_ID, BOB, 9, "arrived after release signal"));
    let snap = h.snapshot();
    assert_eq!(snap.cards.len(), 1);
    assert_eq!(snap.cards[0].attribution.len(), 1);
}
