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
//! consumes a `NostrEvent` (the post-JSON-decode shape) and the contract
//! under test is purely the cache + lifecycle mutation that method performs.
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

/*
The pre-V-40 unit tests below exercised `ingest_dm_relay_list` directly.
After V-40 the kernel no longer has that method; the equivalent coverage
lives in `crates/nmp-nip17/src/kind10050_parser.rs` (`parse_event` /
`IngestParser::parse`). Kept commented out to make the migration visible:

/// A non-empty kind:10050 DM-relay list is parsed into `dm_relay_lists` under
/// the event author's pubkey. kind:10050 has no read/write/both markers —
/// every `relay` tag is a DM-inbox relay.
#[test]
fn ingest_dm_relay_list_stores_non_empty_list() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000010",
        AUTHOR,
        1_000,
        10050,
        vec![
            relay_tag("wss://dm-a.example/"),
            relay_tag("wss://dm-b.example/"),
        ],
    );
    kernel.ingest_dm_relay_list(event);

    let resolved = kernel
        .recipient_dm_relays(AUTHOR)
        .expect("a non-empty kind:10050 must resolve to a DM-relay list");
    // URLs are returned in canonical form — `CanonicalRelayUrl` strips the
    // empty-path trailing slash (`wss://host/` → `wss://host`).
    assert_eq!(
        resolved,
        vec!["wss://dm-a.example", "wss://dm-b.example"],
        "every `relay` tag is a DM-inbox relay, in tag order",
    );

    // kind:10050 also feeds NIP-17 receive routing, so it must enqueue a
    // recompile trigger for active gift-wrap inbox interests.
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "a kind:10050 ingest must enqueue a recompile trigger",
    );
}

/// kind:10050 URLs are canonicalized (lowercase scheme+host) and duplicate
/// `relay` tags are deduped, preserving first-seen order.
#[test]
fn ingest_dm_relay_list_canonicalizes_and_dedupes() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000011",
        AUTHOR,
        1_000,
        10050,
        vec![
            relay_tag("wss://DM-Relay.Example/"),
            // A mixed-case duplicate of the first tag — canonicalizes to the
            // same URL and must be deduped.
            relay_tag("wss://dm-relay.example/"),
            relay_tag("wss://other.example/"),
        ],
    );
    kernel.ingest_dm_relay_list(event);

    let resolved = kernel
        .recipient_dm_relays(AUTHOR)
        .expect("kind:10050 must resolve");
    assert_eq!(
        resolved,
        vec!["wss://dm-relay.example", "wss://other.example"],
        "canonicalization lowercases host and strips the empty-path slash; \
         the duplicate is dropped",
    );
}

/// Non-`relay` tags and non-`wss://` URLs are skipped — mirroring the
/// defensive scheme gate `parse_relay_list` applies to kind:10002.
#[test]
fn ingest_dm_relay_list_ignores_non_relay_and_non_wss_tags() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000012",
        AUTHOR,
        1_000,
        10050,
        vec![
            // The NIP-65 `r` marker must NOT be read as a DM relay.
            r_tag("wss://nip65.example/", Some("write")),
            // A non-wss scheme is rejected.
            relay_tag("ws://insecure.example/"),
            relay_tag("https://not-a-relay.example/"),
            // A `relay` tag with no URL value.
            vec!["relay".to_string()],
            // The one well-formed DM relay.
            relay_tag("wss://valid.example/"),
        ],
    );
    kernel.ingest_dm_relay_list(event);

    let resolved = kernel
        .recipient_dm_relays(AUTHOR)
        .expect("the one well-formed `relay` tag must resolve");
    assert_eq!(
        resolved,
        vec!["wss://valid.example"],
        "only well-formed wss `relay` tags are kept",
    );
}

/// An empty kind:10050 for an author with NO cached DM-relay list is a true
/// no-op: no entry is created.
#[test]
fn ingest_dm_relay_list_empty_for_unknown_author_is_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000013",
        AUTHOR,
        1_000,
        10050,
        Vec::new(),
    );
    kernel.ingest_dm_relay_list(event);

    assert!(
        kernel.recipient_dm_relays(AUTHOR).is_none(),
        "an empty kind:10050 for an unknown author must NOT create a cache entry",
    );
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "an empty kind:10050 for an unknown author has no stale DM inbox plan to re-route",
    );
}

/// An empty kind:10050 for an author who DOES have a cached DM-relay list
/// clears the stale entry — the author explicitly emptied their kind:10050,
/// and the send path must fail closed rather than route to a stale DM-relay
/// list or generic Content relays.
#[test]
fn ingest_dm_relay_list_empty_for_known_author_clears_entry() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let seed = make_event(
        "0000000000000000000000000000000000000000000000000000000000000010",
        AUTHOR,
        1_000,
        10050,
        vec![relay_tag("wss://dm.example/")],
    );
    kernel.ingest_dm_relay_list(seed);
    assert!(
        kernel.recipient_dm_relays(AUTHOR).is_some(),
        "precondition: the seed DM-relay list must be cached",
    );

    let clear = make_event(
        "0000000000000000000000000000000000000000000000000000000000000014",
        AUTHOR,
        2_000,
        10050,
        Vec::new(),
    );
    kernel.ingest_dm_relay_list(clear);

    assert!(
        kernel.recipient_dm_relays(AUTHOR).is_none(),
        "an empty kind:10050 for a known author must REMOVE the stale entry",
    );
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        2,
        "seed and clear must each enqueue a DM-relay recompile trigger",
    );
}

/// A later kind:10050 replaces the cached list — the cache always reflects the
/// most-recently-ingested DM-relay list (the store gates supersession by
/// `created_at`; this handler only runs on the winning event).
#[test]
fn ingest_dm_relay_list_replaces_cached_list() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let first = make_event(
        "0000000000000000000000000000000000000000000000000000000000000010",
        AUTHOR,
        1_000,
        10050,
        vec![relay_tag("wss://old-dm.example/")],
    );
    kernel.ingest_dm_relay_list(first);

    let second = make_event(
        "0000000000000000000000000000000000000000000000000000000000000015",
        AUTHOR,
        2_000,
        10050,
        vec![relay_tag("wss://new-dm.example/")],
    );
    kernel.ingest_dm_relay_list(second);

    assert_eq!(
        kernel.recipient_dm_relays(AUTHOR),
        Some(vec!["wss://new-dm.example".to_string()]),
        "the newer kind:10050 must replace the cached DM-relay list",
    );
}
*/

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

// ─── ingest_contacts (kind:3) ────────────────────────────────────────────────

/// A kind:3 contact list with `p` tags updates the `seed_contacts` follow
/// graph: the followed hex pubkeys are stored under the author's key.
///
/// The author here is NOT the active account, so this isolates the
/// `seed_contacts` insert from the active-account-only
/// `sync_follow_feed_interests` side-effects (registry + `timeline_authors`).
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
    kernel.ingest_contacts(event);

    let follows = kernel
        .seed_contacts
        .get(AUTHOR)
        .expect("a kind:3 must store a follow-graph entry under the author pubkey");
    assert_eq!(
        follows,
        &vec![FOLLOW_A.to_string(), FOLLOW_B.to_string()],
        "only well-formed hex `p`-tag values are kept, in tag order",
    );

    // A11: every kind:3 fans a `FollowListChanged` recompile trigger.
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "a kind:3 ingest must enqueue exactly one FollowListChanged trigger",
    );

    // Non-active author: the active-only follow-feed registry sync is skipped,
    // so `timeline_authors` stays empty.
    assert!(
        kernel.timeline_authors_for_test().is_empty(),
        "a non-active author's kind:3 must NOT mutate the timeline_authors projection",
    );
}

/// An empty kind:3 (no `p` tags) does NOT remove the `seed_contacts` entry —
/// `ingest_contacts` has no empty-list early return (unlike `ingest_relay_list`).
/// It unconditionally stores an empty follow vector, which is the correct
/// "cleared follow set" representation.
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
    kernel.ingest_contacts(seed);
    assert_eq!(
        kernel.seed_contacts.get(AUTHOR).map(Vec::len),
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
    kernel.ingest_contacts(cleared);

    // The entry is PRESENT but empty — `ingest_contacts` always inserts; an
    // empty `p`-tag set yields `Some(&vec![])`, not `None`.
    let follows = kernel
        .seed_contacts
        .get(AUTHOR)
        .expect("an empty kind:3 must still leave a (now-empty) follow-graph entry");
    assert!(
        follows.is_empty(),
        "an empty kind:3 must store an empty follow vector (cleared follow set), \
         got {follows:?}",
    );
}

/// When the kind:3 author IS the active account, `ingest_contacts` additionally
/// runs `sync_follow_feed_interests`, which rebuilds the `timeline_authors`
/// projection and registers M2 follow-feed interests. This asserts that
/// active-account-only branch fires.
#[test]
fn ingest_contacts_for_active_account_syncs_follow_feed_projection() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Declare the host kinds {1, 6} the contact-list-authors subscription REQs
    // for (D0: the substrate no longer hardcodes a kind set).
    kernel.follow_feed_kinds = std::collections::BTreeSet::from([1u32, 6u32]);
    kernel.active_account = Some(AUTHOR.to_string());

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000006",
        AUTHOR,
        1_000,
        3,
        vec![p_tag(FOLLOW_A), p_tag(FOLLOW_B)],
    );
    kernel.ingest_contacts(event);

    // `timeline_authors` is rebuilt from the new follow set plus the active
    // account itself (so the user's own notes appear in the timeline).
    let authors = kernel.timeline_authors_for_test();
    assert!(
        authors.contains(FOLLOW_A) && authors.contains(FOLLOW_B),
        "active-account kind:3 must project followed authors into timeline_authors",
    );
    assert!(
        authors.contains(AUTHOR),
        "timeline_authors must also include the active account itself",
    );

    // One M2 follow-feed interest per follow plus one for the active account.
    assert_eq!(
        kernel.follow_feed_interest_ids_for_test().len(),
        3,
        "active-account kind:3 must register one follow-feed interest per follow \
         plus one for the active account itself",
    );
}

// ─── ingest_profile (kind:0) ─────────────────────────────────────────────────

/// A kind:0 metadata event is parsed by `parse_profile` and stored in the
/// `profiles` read-cache keyed by the event author's pubkey.
///
/// `ingest_profile` writes directly to `self.profiles` with no signature
/// re-verification (it runs post-`verify_and_persist`, D4), so the unsigned
/// `make_event` fixture is sufficient — `parse_profile` only JSON-decodes the
/// `content` field as `ProfileContent`. The `display_name` JSON key wins the
/// `display_name → displayName → name` precedence chain in `parse_profile`.
#[test]
fn ingest_profile_stores_metadata_under_pubkey() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let profile_id = "0000000000000000000000000000000000000000000000000000000000000010";
    let event = make_event(profile_id, AUTHOR, 1_000, 0, Vec::new());
    // `parse_profile` reads only `content`; tags are irrelevant for kind:0.
    let event = NostrEvent {
        content: r#"{"name":"test","display_name":"Test User","nip05":"test@example.com","about":"hi","picture":"https://example.com/a.png"}"#
            .to_string(),
        ..event
    };
    kernel.ingest_profile(event);

    let stored = kernel
        .profiles
        .get(AUTHOR)
        .expect("a kind:0 must store a profile entry under the author pubkey");
    assert_eq!(
        stored.display, "Test User",
        "`display_name` wins the parse_profile precedence chain over `name`",
    );
    assert_eq!(stored.event_id, profile_id);
    assert_eq!(stored.created_at, 1_000);
    assert_eq!(stored.nip05, "test@example.com");
    assert_eq!(stored.about, "hi");
    assert_eq!(
        stored.picture_url.as_deref(),
        Some("https://example.com/a.png"),
        "an http(s) picture URL is retained on the cached Profile",
    );
}

/// A newer kind:0 (higher `created_at`) supersedes an already-cached profile;
/// `ingest_profile` uses strict `>` on `created_at` mirroring the store's D4
/// supersession rule.
#[test]
fn ingest_profile_newer_event_supersedes_older() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let old = NostrEvent {
        content: r#"{"display_name":"Old Name"}"#.to_string(),
        ..make_event(
            "0000000000000000000000000000000000000000000000000000000000000011",
            AUTHOR,
            1_000,
            0,
            Vec::new(),
        )
    };
    kernel.ingest_profile(old);
    assert_eq!(
        kernel.profiles.get(AUTHOR).map(|p| p.display.as_str()),
        Some("Old Name"),
        "precondition: the older kind:0 is cached",
    );

    let new = NostrEvent {
        content: r#"{"display_name":"New Name"}"#.to_string(),
        ..make_event(
            "0000000000000000000000000000000000000000000000000000000000000012",
            AUTHOR,
            2_000,
            0,
            Vec::new(),
        )
    };
    kernel.ingest_profile(new);

    assert_eq!(
        kernel.profiles.get(AUTHOR).map(|p| p.display.as_str()),
        Some("New Name"),
        "a kind:0 with a newer created_at must replace the cached profile",
    );

    // A stale (older) re-delivery must NOT clobber the newer cached profile.
    let stale = NostrEvent {
        content: r#"{"display_name":"Stale Name"}"#.to_string(),
        ..make_event(
            "0000000000000000000000000000000000000000000000000000000000000013",
            AUTHOR,
            500,
            0,
            Vec::new(),
        )
    };
    kernel.ingest_profile(stale);
    assert_eq!(
        kernel.profiles.get(AUTHOR).map(|p| p.display.as_str()),
        Some("New Name"),
        "an older kind:0 re-delivery must not supersede the newer cached profile",
    );
}

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
//   - `claim_profile_after_ingest_queues_fetch` (claim path works)

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

/// A signed kind:1 from an author NOT in `timeline_authors` (and not matched
/// by any `should_store_event` bypass) is dropped before reaching the store:
/// neither the `events` cache nor the `timeline` projection is mutated.
#[test]
fn ingest_timeline_event_from_non_subscribed_author_is_dropped() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No active account — no implicit gate openings.
    // V-112 (ADR-0042): selected_author assertion removed with AuthorViewState.

    let keys = ::nostr::Keys::generate();
    let event = signed_note(&keys, "note from a stranger", 1_700_000_100);

    // Author is deliberately NOT inserted into `timeline_authors`.
    kernel.ingest_timeline_event(
        RelayRole::Content,
        "wss://relay.example/",
        "follow-feed-default",
        event,
    );

    assert!(
        kernel.events.is_empty(),
        "an event from a non-subscribed author must NOT be stored (timeline gate)",
    );
    assert!(
        kernel.timeline.is_empty(),
        "an event from a non-subscribed author must NOT enter the timeline",
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
fn signed_note_with_hashtag(keys: &::nostr::Keys, content: &str, ts: u64, hashtag: &str) -> NostrEvent {
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
    shape
        .authors
        .insert(FOLLOW_A.to_string());
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
