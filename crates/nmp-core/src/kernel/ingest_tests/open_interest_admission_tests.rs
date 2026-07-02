//! ADR-0076 §5.1 — generic `open_interest` store admission.
//!
//! `should_store_event` must admit an inbound event when it matches the
//! `InterestShape` of ANY active registered interest — not only the bespoke
//! follow-set / sub-id-prefix clauses (V-112: `author_view` deleted). This makes a generic
//! `open_interest` REQ functional end-to-end: a non-followed author's notes (or
//! an arbitrary `#t` hashtag feed) reach `self.events` and the
//! `notify_event_observers` fan-out (so `nmp-feed` can expose them) WITHOUT
//! polluting the follow-only home `timeline` ordering projection.

use super::ingest_support::{signed_note, FOLLOW_A};
use super::*;

/// Build one real Schnorr-signed kind:1 event carrying a single `#t` hashtag
/// tag, in the `NostrEvent` shape the kernel ingest path consumes.
fn signed_note_with_hashtag(
    keys: &::nostr::Keys,
    content: &str,
    ts: u64,
    hashtag: &str,
) -> NostrEvent {
    use ::nostr::{EventBuilder, Tag, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
        .tag(Tag::hashtag(hashtag))
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    NostrEvent {
        id: nostr_event.id.to_hex(),
        pubkey: nostr_event.pubkey.to_hex(),
        created_at: nostr_event.created_at.as_secs(),
        kind: nostr_event.kind.as_u16() as u32,
        tags: nostr_event
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: nostr_event.content.clone(),
        sig: nostr_event.sig.to_string(),
    }
}

/// Register a generic `open_interest` directly on the kernel registry — the
/// same `ensure_sub` body the `ActorCommand::OpenInterest` dispatch arm runs.
/// `shape` is the parsed `InterestShape` (the test passes the equivalent of a
/// verbatim NIP-01 filter).
fn register_open_interest(kernel: &mut Kernel, shape: crate::planner::InterestShape) {
    use crate::planner::{InterestLifecycle, InterestScope, LogicalInterest};
    use crate::subs::sub_key::{SubIdentity, SubKey, SubOwnerKey, SubScope};

    let key = SubKey::builder("open-interest").with(&shape).finish();
    let identity = SubIdentity::new(SubOwnerKey::new("test-consumer"), key, SubScope::Global);
    let interest = LogicalInterest {
        scope: InterestScope::Global,
        shape,
        lifecycle: InterestLifecycle::Tailing,
        ..LogicalInterest::default()
    };
    let _ = kernel.open_interest_sub(identity, interest);
}

/// A signed kind:1 from an author who is NOT followed, but whose pubkey is
/// named by an active `open_interest` (`{"kinds":[1],"authors":["<hex>"]}`),
/// is admitted to the `events` read-cache (so the feed-engine observer fan-out
/// can expose it) — yet it must NOT enter the follow-only home `timeline`
/// ordering projection (ADR-0076 §5.1 exposure point 2).
#[test]
fn open_interest_admits_non_followed_author_event_without_home_timeline_pollution() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "note from a non-followed author", 1_700_000_300);
    let event_id = event.id.clone();
    let author = event.pubkey.clone();

    // Author is deliberately NOT in `timeline_authors`. Register a generic
    // tailing interest for exactly this author's kind:1 notes.
    let mut shape = crate::planner::InterestShape::default();
    shape.authors.insert(author.clone());
    shape.kinds.insert(1);
    register_open_interest(&mut kernel, shape);

    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        // A generic compiled interest sub id — NONE of the bespoke
        // gate-bypass prefixes. Admission must come from the registry match.
        "sub-deadbeef",
        event,
    );

    assert!(
        kernel.events.contains_key(&event_id),
        "an event matching an active open_interest must be stored in `events` \
         (so the feed-engine observer fan-out can expose it)",
    );
    assert!(
        kernel.timeline.iter().all(|id| id != &event_id),
        "a non-followed author's event must NOT enter the follow-only home \
         `timeline` ordering projection — exposure is via the feed engine",
    );
}

/// A signed kind:1 carrying a `#t` hashtag that matches an active hashtag
/// `open_interest` (`{"kinds":[1],"#t":["nostr"]}`) is admitted to `events`.
/// This is the path the migrated `openFirehoseTag` → `openInterest` call site
/// depends on (Step 3).
#[test]
fn open_interest_admits_matching_hashtag_event() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note_with_hashtag(&keys, "tagged note", 1_700_000_400, "nostr");
    let event_id = event.id.clone();

    let mut shape = crate::planner::InterestShape::default();
    shape.kinds.insert(1);
    // `InterestShape.tags` keys drop the leading `#` (see
    // `InterestShape::from_filter_json` — `#t` → `tags["t"]`).
    shape.tags.insert(
        "t".to_string(),
        std::iter::once("nostr".to_string()).collect(),
    );
    register_open_interest(&mut kernel, shape);

    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "sub-cafef00d",
        event,
    );

    assert!(
        kernel.events.contains_key(&event_id),
        "an event whose #t tag matches an active hashtag open_interest must be \
         stored in `events`",
    );
}

/// Negative control: an event matching NO active interest (no follow, no
/// open_interest, no bypass prefix) is still dropped — the generalisation must
/// not become an unconditional accept-all.
#[test]
fn open_interest_generalisation_still_drops_unmatched_event() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Register an interest for a DIFFERENT author.
    let mut shape = crate::planner::InterestShape::default();
    shape.authors.insert(FOLLOW_A.to_string());
    shape.kinds.insert(1);
    register_open_interest(&mut kernel, shape);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "unrelated stranger", 1_700_000_500);

    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "sub-00000000",
        event,
    );

    assert!(
        kernel.events.is_empty(),
        "an event matching no active interest must still be dropped",
    );
}
