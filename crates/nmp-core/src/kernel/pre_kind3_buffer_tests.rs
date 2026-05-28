//! Tests for the V-59 rung 1 (Q7) pre-kind:3 ingest buffer.
//!
//! The buffer parks kind:1 / kind:6 events whose author is not (yet) in the
//! active account's follow set, instead of dropping them. A later
//! `sync_follow_feed_interests` that adds the author replays the parked event;
//! authors that never become followed are dropped on the next sync. The buffer
//! is cleared on identity change so a switched-out account's parked events
//! never leak into the new account's stream.

use super::nostr::NostrEvent;
use super::Kernel;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};

fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let nostr_event = EventBuilder::text_note(content)
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

/// A kind:1 from an author NOT in `timeline_authors` is PARKED in the
/// pre-kind:3 buffer rather than dropped: the store projections stay empty but
/// the buffer holds the event keyed by its id.
#[test]
fn unfollowed_author_note_is_parked_not_dropped() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "note from a stranger", 1_700_000_000);
    let event_id = event.id.clone();

    let stored = kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    assert!(!stored, "the gate must reject (return false) — author not followed");
    assert!(
        kernel.events.is_empty() && kernel.timeline.is_empty(),
        "a parked event must NOT be stored or enter the timeline"
    );
    assert_eq!(
        kernel.pre_kind3_buffer_len_for_test(),
        1,
        "the event must be parked in the pre-kind:3 buffer"
    );
    assert!(
        kernel.pre_kind3_buffer_contains_for_test(&event_id),
        "the buffer must be keyed by the parked event's id"
    );
}

/// When a later `sync_follow_feed_interests` adds the parked event's author to
/// the follow set, the buffer is flushed and the event is finally stored.
#[test]
fn sync_follow_feed_replays_parked_event_for_newly_followed_author() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "from a soon-to-be-followed author", 1_700_000_010);
    let event_id = event.id.clone();
    let author = event.pubkey.clone();

    // Park it (author not yet followed).
    let _ = kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );
    assert_eq!(kernel.pre_kind3_buffer_len_for_test(), 1);

    // A kind:3 names the author → sync_follow_feed_interests rebuilds
    // timeline_authors and flushes the buffer.
    kernel.sync_follow_feed_interests(&[author.clone()]);

    assert!(
        kernel.events.contains_key(&event_id),
        "the parked event must be replayed and stored once its author is followed"
    );
    assert!(
        kernel.timeline.iter().any(|id| id == &event_id),
        "the replayed event must enter the timeline ordering projection"
    );
    assert_eq!(
        kernel.pre_kind3_buffer_len_for_test(),
        0,
        "the buffer must be drained after the flush"
    );
}

/// A parked event whose author is NOT added by the sync is DROPPED, not
/// re-parked: the buffer is empty after the flush and the event never stores.
#[test]
fn sync_follow_feed_drops_parked_event_for_still_unfollowed_author() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let parked_keys = ::nostr::Keys::generate();
    let parked = signed_note(&parked_keys, "never followed", 1_700_000_020);
    let parked_id = parked.id.clone();

    let _ = kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        parked,
    );
    assert_eq!(kernel.pre_kind3_buffer_len_for_test(), 1);

    // Sync names a DIFFERENT author; the parked one stays unfollowed.
    let other = ::nostr::Keys::generate().public_key().to_hex();
    kernel.sync_follow_feed_interests(&[other]);

    assert!(
        !kernel.events.contains_key(&parked_id),
        "a still-unfollowed author's parked event must not be stored"
    );
    assert_eq!(
        kernel.pre_kind3_buffer_len_for_test(),
        0,
        "the flush must DROP non-matching parked entries (not re-park them)"
    );
}

/// An identity change clears the buffer BEFORE the follow-set sync, so the
/// prior account's parked events are never replayed into the new account —
/// even if the new account happens to follow that same author.
#[test]
fn identity_change_clears_parked_events() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "parked under the old identity", 1_700_000_030);
    let event_id = event.id.clone();
    let author = event.pubkey.clone();

    let _ = kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );
    assert_eq!(kernel.pre_kind3_buffer_len_for_test(), 1);

    // New identity that DOES follow the same author. The reconcile must still
    // clear the parked event first — it belongs to the prior identity's
    // context and must not leak forward.
    kernel.active_account = Some(author.clone());
    kernel.seed_contacts.insert(author.clone(), vec![author.clone()]);
    kernel.reconcile_follow_feed_after_identity_change();

    assert_eq!(
        kernel.pre_kind3_buffer_len_for_test(),
        0,
        "identity change must clear the pre-kind:3 buffer"
    );
    assert!(
        !kernel.events.contains_key(&event_id),
        "the prior identity's parked event must NOT be replayed into the new account"
    );
}
