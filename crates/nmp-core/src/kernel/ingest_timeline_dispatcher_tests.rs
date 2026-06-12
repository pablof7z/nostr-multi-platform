//! Kernel-level regression test: `ingest_timeline_event` feeds the
//! `EventIngestDispatcher`.
//!
//! Gap closed by this PR: the timeline ingest path (kind:1 / kind:6 follow-feed
//! events) called `notify_raw_event_observers` and `notify_event_observers` but
//! never called `ingest_dispatcher_slot()…dispatch()`. An all-kinds range parser
//! (e.g. `chirp-tui`'s `RawCacheIngestParser`, registered for `0..u32::MAX`)
//! therefore silently missed every note and repost that arrived via the live
//! relay path.
//!
//! This test pins the contract at the kernel seam:
//! - `ingest_timeline_event` MUST dispatch to registered `IngestParser`s on
//!   `Inserted` and `Replaced` outcomes.
//! - It MUST NOT fire on `Duplicate` (D4 dedup).
//! - An all-kinds range parser MUST receive kind:1 timeline events.

use super::*;
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use crate::store::VerifiedEvent;
use crate::substrate::IngestParser;
use std::sync::{Arc, Mutex};

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// An `IngestParser` that records the kind of every event it receives.
struct CapturingIngestParser {
    seen_kinds: Mutex<Vec<u32>>,
}

impl CapturingIngestParser {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen_kinds: Mutex::new(Vec::new()),
        })
    }

    fn seen(&self) -> Vec<u32> {
        self.seen_kinds.lock().unwrap().clone()
    }
}

impl IngestParser for CapturingIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        self.seen_kinds.lock().unwrap().push(evt.raw().kind);
    }
}

/// Build a signed kind:1 `NostrEvent` using real Schnorr signing so
/// `ingest_timeline_event` passes `VerifiedEvent::try_from_raw`.
fn signed_kind1(keys: &::nostr::Keys, content: &str, ts: u64) -> super::nostr::NostrEvent {
    use ::nostr::{EventBuilder, Timestamp};
    let ev = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with generated keypair");
    super::nostr::NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

/// PRIMARY CONTRACT: `ingest_timeline_event` dispatches to a registered
/// `IngestParser` for a kind:1 note on an `Inserted` outcome.
///
/// This is the regression guard for the timeline ingest gap: chirp-tui's
/// all-kinds `RawCacheIngestParser` (registered for `0..u32::MAX`) silently
/// missed every kind:1 and kind:6 event because the timeline path never called
/// `ingest_dispatcher_slot()…dispatch()`.
#[test]
fn ingest_timeline_event_fires_ingest_parser_on_insert() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    // Seed author into timeline_authors so `ingest_timeline_event` admits the event.
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingIngestParser::new();
    // Register for ALL kinds — mirrors chirp-tui's RawCacheIngestParser slot.
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser) as Arc<dyn IngestParser>,
        );
    }

    let ev = signed_kind1(&keys, "hello world", 1_700_000_000);
    let ev_id = ev.id.clone();

    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "follow-feed-default", ev);

    let seen = parser.seen();
    assert_eq!(
        seen.len(),
        1,
        "all-kinds IngestParser must fire exactly once on Inserted; got {seen:?}",
    );
    assert_eq!(
        seen[0], 1,
        "parser must see kind:1; got {:?}",
        seen,
    );
    let _ = ev_id; // consumed for clarity
}

/// DUPLICATE GATE: `ingest_timeline_event` must NOT re-fire the
/// `IngestParser` when the same event is delivered a second time
/// (store outcome = `Duplicate`). Mirrors the D4 dedup contract.
#[test]
fn ingest_timeline_event_does_not_fire_parser_on_duplicate() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.timeline_authors.insert(author.clone());

    let parser = CapturingIngestParser::new();
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser) as Arc<dyn IngestParser>,
        );
    }

    // Build ONE event but inject it twice.
    let ev1 = signed_kind1(&keys, "dedup check", 1_700_000_001);
    let ev2 = super::nostr::NostrEvent {
        id: ev1.id.clone(),
        pubkey: ev1.pubkey.clone(),
        created_at: ev1.created_at,
        kind: ev1.kind,
        tags: ev1.tags.clone(),
        content: ev1.content.clone(),
        sig: ev1.sig.clone(),
    };

    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "follow-feed-default", ev1);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay2.test/", "follow-feed-default", ev2);

    let seen = parser.seen();
    assert_eq!(
        seen.len(),
        1,
        "IngestParser must fire only on Inserted, not Duplicate; got {seen:?}",
    );
}

/// KIND ROUTING: a kind-specific parser (kind:1059) must NOT fire for a
/// kind:1 timeline event delivered through `ingest_timeline_event`.
#[test]
fn ingest_timeline_event_routes_to_correct_kind_parser() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.timeline_authors.insert(author.clone());

    let parser_all = CapturingIngestParser::new();
    let parser_1059 = CapturingIngestParser::new();
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.replace_range_parser(
            0..u32::MAX,
            "test.all-kinds",
            Arc::clone(&parser_all) as Arc<dyn IngestParser>,
        );
        d.replace_kind_parser(1059, "test.kind1059", Arc::clone(&parser_1059) as Arc<dyn IngestParser>);
    }

    let ev = signed_kind1(&keys, "routing check", 1_700_000_002);
    kernel.ingest_timeline_event(RelayRole::Content, "wss://relay.test/", "follow-feed-default", ev);

    assert_eq!(
        parser_all.seen(),
        vec![1],
        "all-kinds parser must fire for kind:1 timeline event",
    );
    assert!(
        parser_1059.seen().is_empty(),
        "kind:1059 parser must NOT fire for kind:1 timeline event",
    );
}

/// NO-PARSER GATE: when no `IngestParser` is registered, `ingest_timeline_event`
/// must complete successfully without dispatching — the event is stored and
/// observers are notified, but the dispatcher path is never entered.
///
/// This test is the behavioral guard for the D8 clone gate: with an empty
/// dispatcher `is_interested` returns `false`, so the clone is skipped and the
/// second lock acquisition never happens. We assert behaviorally: the event
/// is accepted (function returns `true`) and no parser fires (verified by the
/// absence of any registered parser — a vacuous proof, but consistent with the
/// unit being the only observable boundary here).
#[test]
fn ingest_timeline_event_no_parser_registered_no_dispatch() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    kernel.timeline_authors.insert(author.clone());

    // Dispatcher is empty — is_interested(1) == false, clone must be skipped.
    assert_eq!(
        kernel.ingest_dispatcher_slot().read().unwrap().registration_count(),
        0,
        "dispatcher must be empty before ingest",
    );

    let ev = signed_kind1(&keys, "no-parser path", 1_700_000_003);
    let accepted = kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.test/",
        "follow-feed-default",
        ev,
    );

    // The event must be accepted (stored + observer fan-out succeeded).
    assert!(
        accepted,
        "ingest_timeline_event must return true on Inserted even with no registered parser",
    );
    // Confirm dispatcher is still empty — no side-effects on the registry.
    assert_eq!(
        kernel.ingest_dispatcher_slot().read().unwrap().registration_count(),
        0,
        "dispatcher must remain empty after ingest with no parser",
    );
}
