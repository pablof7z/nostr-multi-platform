//! Unit tests for the kernel ingest handler `ingest_contacts` (kind:3) in
//! `kernel/ingest/`.
//!
//! ## Scope vs. the existing `tests.rs` regression suite
//!
//! `kernel/tests.rs` already covers stale re-delivery (D4 supersession) by
//! driving events through `inject_replaceable_event` (store + ingest). These
//! tests are orthogonal: they call the `ingest_contacts` method *directly* —
//! the kernel method invoked AFTER `verify_and_persist` confirms an
//! `Inserted | Replaced`. No store round-trip, no signing: the ingest method
//! consumes a `NostrEvent` (the post-JSON-decode shape) and the contract under
//! test is the store-derived contacts transition + lifecycle mutation it
//! performs.
//!
//! `NostrEvent` is `pub(super)` within `kernel`, so this file (declared as
//! `#[cfg(test)] mod ingest_tests;` in `kernel/mod.rs`) constructs it directly
//! — that is the minimal, deterministic fixture for a unit test of these
//! handlers. Real Schnorr signing is unnecessary because the ingest method
//! does not re-verify; the `sig` field is never read past `verify_and_persist`.
//!
//! Pre-2026-05-25 this file also exercised `ingest_relay_list` (kind:10002,
//! NIP-65) directly. That kernel-side method was deleted alongside the
//! `10002 =>` arm in `kernel/ingest/mod.rs` when the substrate
//! `nmp_router::Kind10002Parser` became the production writer. Equivalent
//! coverage now lives in `crates/nmp-router/src/ingest.rs` (`parse_event` /
//! `IngestParser::parse`); the empty-list-removes-known-entry semantics
//! moved with it.

use super::nostr::NostrEvent;
use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

// 64-char hex pubkeys — `is_hex_pubkey` requires exactly 64 ascii hex digits,
// so the `p`-tag filter in `ingest_contacts` only keeps well-formed values.
const AUTHOR: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const FOLLOW_A: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const FOLLOW_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";

/// Build a `NostrEvent` of `kind` for `pubkey` with the supplied tags.
///
/// `id` is derived from `created_at` so two events for the same author have
/// distinct ids (the supersession tiebreak in `ingest_relay_list` compares
/// event ids on a `created_at` tie). `sig` is a placeholder — the ingest
/// methods never read it (they run post-verification).
fn make_event(
    id: &str,
    pubkey: &str,
    created_at: u64,
    kind: u32,
    tags: Vec<Vec<String>>,
) -> NostrEvent {
    NostrEvent {
        id: id.to_string(),
        pubkey: pubkey.to_string(),
        created_at,
        kind,
        tags,
        content: String::new(),
        sig: String::new(),
    }
}

/// A single NIP-65 `r` tag: `["r", url]` or `["r", url, marker]`.
///
/// Retained for the commented-out V-40 migration block below (the live
/// equivalent now lives in `crates/nmp-router/src/ingest.rs`).
#[allow(dead_code)]
fn r_tag(url: &str, marker: Option<&str>) -> Vec<String> {
    match marker {
        Some(m) => vec!["r".to_string(), url.to_string(), m.to_string()],
        None => vec!["r".to_string(), url.to_string()],
    }
}

/// A single kind:3 `p` tag: `["p", pubkey]`.
fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

/// A single NIP-17 kind:10050 `relay` tag: `["relay", url]`.
///
/// Retained for the commented-out V-40 migration block below (the live
/// equivalent now lives in `crates/nmp-nip17/src/kind10050_parser.rs`).
#[allow(dead_code)]
fn relay_tag(url: &str) -> Vec<String> {
    vec!["relay".to_string(), url.to_string()]
}

// ─── kind:10002 NIP-65 relay list (2026-05-25: moved to nmp-router) ─────────
//
// The kernel no longer parses kind:10002 directly — the substrate
// `IngestParser` registry fans the event to `nmp_router::Kind10002Parser`,
// which owns the `InMemoryMailboxCache`. Equivalent unit tests for the
// parser live in `crates/nmp-router/src/ingest.rs`.
//
// The kernel-side wildcard ingest arm's `Kernel::on_mailbox_changed`
// observer (the kind-agnostic seam that fires the recompile trigger after
// the substrate cache mutates) is exercised end-to-end by the kind:10002
// integration tests in `crates/nmp-core/src/kernel/outbox_tests.rs` and
// `t140_m1_retirement_tests.rs` — both drive kind:10002 events through
// `inject_replaceable_event`, which mirrors the production substrate
// path post-2026-05-25 (cache mutation + `Nip65Arrived` enqueue).

// ─── kind:10050 DM-relay list (V-40: moved to nmp-nip17) ────────────────────
//
// The kernel no longer parses kind:10050 directly — the substrate
// `IngestParser` registry fans the event to `nmp-nip17::Kind10050Parser`,
// which owns the `DmRelayCache`. Tests for that parser live in
// `crates/nmp-nip17/src/kind10050_parser.rs`. The kernel-side surface kept
// here is just the `recipient_dm_relays` lookup that reads through the
// injected `DmInboxRelayLookup` handle — exercised by the
// `recipient_dm_relays_none_for_uncached_pubkey` test below.

/// `recipient_dm_relays` returns `None` for a pubkey with no kind:10050 — the
/// genuinely-missing case the DM send path treats as not ready.
#[test]
fn recipient_dm_relays_none_for_uncached_pubkey() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert!(
        kernel.recipient_dm_relays(AUTHOR).is_none(),
        "a pubkey with no ingested kind:10050 must resolve to None",
    );
}

// ─── F-02 regression: on_dm_relays_changed enqueues DmRelayListChanged ─────
//
// These tests verify the seam the V-40 migration left as a production
// follow-up: `Kernel::on_dm_relays_changed` enqueues a
// `CompileTrigger::DmRelayListChanged` trigger so the planner re-routes
// `PTagRouting::Nip17DmRelays` interests after a kind:10050 fetch closes.
//
// The production trigger path is:
//   `verify_and_persist` → `Kind10050Parser` writes `DmRelayCache` →
//   wildcard arm snapshots `recipient_dm_relays` before/after →
//   transition detected → `on_dm_relays_changed` → trigger enqueued.
//
// These unit tests exercise `on_dm_relays_changed` directly (the new method
// added by the F-02 fix) so the contract is locked at the kernel level
// independently of the parser wiring. The end-to-end path (Kind10050Parser
// + wildcard arm + trigger fan-out) is covered by the integration test
// `real_relay_nip17_cold_start_kernel` in `crates/nmp-testing/`.

/// Calling `on_dm_relays_changed` enqueues exactly one
/// `CompileTrigger::DmRelayListChanged` trigger on the lifecycle inbox.
///
/// This is the F-02 regression: a returned `DmRelayListChanged` trigger
/// causes the planner to re-route `PTagRouting::Nip17DmRelays` interests
/// on the next `drain_lifecycle_tick` — the cold-start DM receive path.
#[test]
fn on_dm_relays_changed_enqueues_trigger() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "precondition: no pending triggers"
    );

    kernel.on_dm_relays_changed(AUTHOR, 1_000);

    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "on_dm_relays_changed must enqueue exactly one recompile trigger"
    );
}

/// Two calls for the same author at different timestamps enqueue two
/// triggers (coalescing happens at drain time, not at enqueue time).
#[test]
fn on_dm_relays_changed_two_calls_enqueue_two_triggers() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.on_dm_relays_changed(AUTHOR, 1_000);
    kernel.on_dm_relays_changed(AUTHOR, 2_000);
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        2,
        "two on_dm_relays_changed calls before drain must produce two queued triggers"
    );
}

// ─── contacts (kind:3) ────────────────────────────────────────────────────────

/// A kind:3 contact list with `p` tags updates the contacts-cache follow
/// graph: the followed hex pubkeys are stored under the author's key.
///
/// The author here is NOT the active account, so this isolates the
/// contacts-cache write from the active-account-only recompile trigger.
#[test]
fn ingest_contacts_with_p_tags_updates_follow_graph() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No active account → the active-only follow-feed sync branch is skipped.
    assert!(
        kernel.active_account.is_none(),
        "precondition: no active account"
    );

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000004",
        AUTHOR,
        1_000,
        3,
        vec![
            p_tag(FOLLOW_A),
            p_tag(FOLLOW_B),
            // A non-hex `p` value must be filtered out by `is_hex_pubkey`.
            vec!["p".to_string(), "not-a-pubkey".to_string()],
            // A non-`p` tag must be ignored entirely.
            vec!["e".to_string(), FOLLOW_A.to_string()],
        ],
    );
    // `inject_contacts` persists the kind:3, then runs the chokepoint
    // projection. The kernel reacts to a transition ONLY for the active account.
    kernel.inject_contacts(event);

    let follows = crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR)
        .expect("a kind:3 must be stored under the author pubkey");
    assert_eq!(
        follows,
        vec![FOLLOW_A.to_string(), FOLLOW_B.to_string()],
        "only well-formed hex `p`-tag values are kept, in tag order",
    );

    // Non-active author: the active-account contacts-transition signal does NOT
    // fire, so NO `FollowListChanged` trigger is enqueued (D4 — arbitrary peers'
    // kind:3 must not drive the kernel's follow-feed lifecycle) and
    // `timeline_authors` stays empty. (Pre-PR-3 the old `ingest_contacts`
    // enqueued an unconditional trigger even for non-active peers — a benign
    // over-fire that drove a no-op recompile; PR 3 tightens it to the active
    // account, matching the active-gate the effects always had.)
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "a non-active author's kind:3 must NOT enqueue a source recompile trigger",
    );
    assert!(
        kernel.timeline_authors_for_test().is_empty(),
        "a non-active author's kind:3 must NOT mutate the timeline_authors projection",
    );
}

/// An empty kind:3 (no `p` tags) stores an empty follow vector, which is the
/// correct "cleared follow set" representation.
#[test]
fn ingest_contacts_empty_list_stores_empty_follow_vector() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Seed a non-empty contact list first.
    let seed = make_event(
        "0000000000000000000000000000000000000000000000000000000000000004",
        AUTHOR,
        1_000,
        3,
        vec![p_tag(FOLLOW_A), p_tag(FOLLOW_B)],
    );
    kernel.inject_contacts(seed);
    assert_eq!(
        crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR).map(|f| f.len()),
        Some(2),
        "precondition: the seed contact list holds two follows",
    );

    // A newer kind:3 with no `p` tags → the author cleared their follow set.
    let cleared = make_event(
        "0000000000000000000000000000000000000000000000000000000000000005",
        AUTHOR,
        2_000,
        3,
        Vec::new(),
    );
    kernel.inject_contacts(cleared);

    // The event is PRESENT but derived empty — an empty `p`-tag set yields
    // `Some(vec![])`, NOT `None` (a cleared follow set is distinct from
    // "no kind:3 stored").
    let follows = crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR)
        .expect("an empty kind:3 must still leave a stored contact-list event");
    assert!(
        follows.is_empty(),
        "an empty kind:3 must store an empty follow vector (cleared follow set), \
         got {follows:?}",
    );
}

/// When the kind:3 author IS the active account, `ingest_contacts` emits the
/// active-source recompile trigger. The reduced author-set expansion itself is
/// owned by the feed-source compiler, not by a bespoke core follow-feed path.
#[test]
fn ingest_contacts_for_active_account_enqueues_source_recompile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(AUTHOR.to_string());

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000006",
        AUTHOR,
        1_000,
        3,
        vec![p_tag(FOLLOW_A), p_tag(FOLLOW_B)],
    );
    kernel.inject_contacts(event);

    assert!(
        crate::slots::latest_kind3_follows_from_arc(&kernel.store, AUTHOR)
            .expect("active kind:3 must be stored")
            .contains(&FOLLOW_A.to_string()),
        "active-account kind:3 must update the stored latest contact list",
    );
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "active-account kind:3 must enqueue one source recompile trigger"
    );
    assert!(
        kernel.timeline_authors_for_test().is_empty(),
        "core must not project active follows into timeline_authors directly; \
         reduced feed sources own author-set expansion",
    );
}

// ─── ingest_profile (kind:0) ─────────────────────────────────────────────────

// ─── ingest_timeline_event (kind:1) ──────────────────────────────────────────

/// Build one real Schnorr-signed kind:1 event in the `NostrEvent` shape the
/// kernel ingest path consumes after JSON decoding.
///
/// `ingest_timeline_event` routes through `store.insert` →
/// `VerifiedEvent::try_from_raw`, which performs real signature verification —
/// the unsigned `make_event` fixture would be dropped at that gate, so timeline
/// tests must sign. Mirrors `clock_injection_tests.rs::signed_note`; the
/// `expect` cannot fail with a freshly-generated keypair.
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

// F-CR-00: `ingest_timeline_event_queues_missing_author_profile_request` and
// `ingest_timeline_event_skips_author_profile_when_cached` were deleted when
// the proactive kind:0 fetch at timeline.rs:172 was removed. The replacement
// invariants live in `proactive_profile_fetch_tests.rs`:
//   - `kind1_ingest_does_not_queue_profile_fetch` (no proactive fetch)
//   - `resolve_profile_after_ingest_queues_fetch` (resolve path works)

/// A signed kind:1 from an author present in `timeline_authors` passes the
/// timeline gate: it is persisted to the `events` read-cache AND appended to
/// the `timeline` ordering projection.
///
/// The sub_id (`follow-feed-default`) is a plain id with none of the
/// gate-bypass prefixes (`diag-firehose-`, `author-notes-`, `thread-*`), so
/// the author membership in `timeline_authors` is the only thing that opens
/// both the `should_store_event` gate and the `timeline.push_back` gate.
#[test]
fn ingest_timeline_event_from_subscribed_author_stores_event() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "hello from a followed author", 1_700_000_000);
    let event_id = event.id.clone();

    // Subscribe the author: child-module test access to the kernel-private
    // `timeline_authors` projection (the `*_for_test` accessor is read-only).
    kernel.timeline_authors.insert(event.pubkey.clone());

    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    assert!(
        kernel.events.contains_key(&event_id),
        "a signed kind:1 from a subscribed author must be cached in `events`",
    );
    assert!(
        kernel.timeline.iter().any(|id| id == &event_id),
        "a subscribed author's event must also be appended to the `timeline` \
         ordering projection",
    );
}

/// ADR-0057 oracle — a signed kind:1 from an author NOT in `timeline_authors`
/// (and not matched by any `should_store_event` read-time bypass) PERSISTS to
/// the authoritative store (admission = valid signature; persistence is no
/// longer relevance-gated — #1442) but does NOT enter the timeline VIEW: the
/// read-cache (`self.events`) and the ordering projection (`self.timeline`)
/// stay empty. Persistence ≠ projection.
#[test]
fn non_subscribed_author_event_persists_but_does_not_timeline_project() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No active account — no implicit gate openings.

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "note from a stranger", 1_700_000_100);
    let event_id = event.id.clone();

    // Author is deliberately NOT inserted into `timeline_authors`.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    // ADR-0057 — the event IS now in the authoritative store (kind-agnostic,
    // valid-sig admission). This closes the #1442 relevance-shaped hole.
    let id_bytes = crate::kernel::hex_to_pubkey_bytes(&event_id).expect("event id is 64-char hex");
    assert!(
        kernel
            .store
            .get_by_id(&id_bytes)
            .expect("store get_by_id must not error")
            .is_some(),
        "ADR-0057: a validly-signed non-followed-author event must be PERSISTED",
    );

    // …but it is NOT projected into the timeline VIEW.
    assert!(
        kernel.events.is_empty(),
        "a non-followed author's event must NOT enter the timeline read-cache",
    );
    assert!(
        kernel.timeline.is_empty(),
        "a non-followed author's event must NOT enter the timeline ordering",
    );
}

/// A duplicate ingest of the same signed event (same id, same relay) is not
/// double-stored: the second delivery hits `InsertOutcome::Duplicate` and
/// returns before the `events.insert` / `timeline.push_back`, so both
/// projections still hold exactly one entry.
#[test]
fn ingest_timeline_event_duplicate_is_not_double_stored() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "ingested twice", 1_700_000_200);
    let event_id = event.id.clone();
    kernel.timeline_authors.insert(event.pubkey.clone());

    // First delivery → Inserted.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event.clone(),
    );
    // Second delivery, identical event from the same relay → Duplicate.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    assert_eq!(
        kernel.events.len(),
        1,
        "a duplicate ingest must not add a second `events` cache entry",
    );
    assert_eq!(
        kernel.timeline.len(),
        1,
        "a duplicate ingest must not append a second `timeline` entry",
    );
    assert!(
        kernel.events.contains_key(&event_id),
        "the single cached event is the one that was ingested",
    );
}

// ─── ADR-0042 §5.1 — generic `open_interest` store admission ─────────────────
//
// `should_store_event` must admit an inbound event when it matches the
// `InterestShape` of ANY active registered interest — not only the bespoke
// follow-set / sub-id-prefix clauses (V-112: `author_view` deleted). This makes a generic
// `open_interest` REQ functional end-to-end: a non-followed author's notes (or
// an arbitrary `#t` hashtag feed) reach `self.events` and the
// `notify_event_observers` fan-out (so `nmp-feed` can expose them) WITHOUT
// polluting the follow-only home `timeline` ordering projection.

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
/// ordering projection (ADR-0042 §5.1 exposure point 2).
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
