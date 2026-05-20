//! Unit tests for the kernel ingest handlers `ingest_relay_list` (kind:10002,
//! NIP-65) and `ingest_contacts` (kind:3) in `kernel/ingest/`.
//!
//! ## Scope vs. the existing `tests.rs` regression suite
//!
//! `kernel/tests.rs` already covers stale re-delivery (D4 supersession) for
//! both kinds by driving events through `inject_replaceable_event` (store +
//! ingest). These tests are orthogonal: they call the `ingest_relay_list` /
//! `ingest_contacts` methods *directly* — the kernel methods invoked AFTER
//! `verify_and_persist` confirms an `Inserted | Replaced`. No store round-trip,
//! no signing: the ingest methods consume a `NostrEvent` (the post-JSON-decode
//! shape) and the contract under test is purely the cache + lifecycle mutation
//! those methods perform.
//!
//! `NostrEvent` is `pub(super)` within `kernel`, so this file (declared as
//! `#[cfg(test)] mod ingest_tests;` in `kernel/mod.rs`) constructs it directly
//! — that is the minimal, deterministic fixture for a unit test of these
//! handlers. Real Schnorr signing is unnecessary because neither ingest method
//! re-verifies; the `sig` field is never read past `verify_and_persist`.

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
fn make_event(id: &str, pubkey: &str, created_at: u64, kind: u32, tags: Vec<Vec<String>>) -> NostrEvent {
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

// ─── ingest_relay_list (kind:10002) ──────────────────────────────────────────

/// A non-empty NIP-65 relay list is parsed into `author_relay_lists` under the
/// event author's pubkey, with the read/write/both buckets split by marker.
#[test]
fn ingest_relay_list_stores_non_empty_list() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        AUTHOR,
        1_000,
        10002,
        vec![
            r_tag("wss://read.example/", Some("read")),
            r_tag("wss://write.example/", Some("write")),
            r_tag("wss://both.example/", None),
        ],
    );
    kernel.ingest_relay_list(event);

    let stored = kernel
        .author_relay_lists
        .get(AUTHOR)
        .expect("a non-empty kind:10002 must store an entry under the author pubkey");
    assert_eq!(stored.created_at, 1_000);
    assert_eq!(stored.read_relays, vec!["wss://read.example/"]);
    assert_eq!(stored.write_relays, vec!["wss://write.example/"]);
    assert_eq!(stored.both_relays, vec!["wss://both.example/"]);

    // A1: storing a fresh mailbox fans a `Nip65Arrived` recompile trigger so
    // the M2 subscription compiler re-routes the author on the next tick.
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "storing a non-empty NIP-65 list must enqueue exactly one recompile trigger",
    );
}

/// An empty kind:10002 for an author with NO cached relay list is a true
/// no-op: no entry is created and no recompile trigger is enqueued (there is
/// no stale plan to fix).
#[test]
fn ingest_relay_list_empty_for_unknown_author_is_noop() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // No `r` tags at all → all three relay buckets parse empty.
    let event = make_event(
        "0000000000000000000000000000000000000000000000000000000000000002",
        AUTHOR,
        1_000,
        10002,
        Vec::new(),
    );
    kernel.ingest_relay_list(event);

    assert!(
        !kernel.author_relay_lists.contains_key(AUTHOR),
        "an empty NIP-65 list for an unknown author must NOT create a cache entry",
    );
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "an empty NIP-65 list for an unknown author must not enqueue a recompile trigger",
    );
}

/// An empty kind:10002 for an author who DOES have a cached relay list clears
/// the stale entry (the author explicitly emptied their NIP-65 metadata) and
/// fans a `Nip65Arrived` trigger so the now-stale M2 plan is recompiled.
#[test]
fn ingest_relay_list_empty_for_known_author_clears_entry_and_triggers_recompile() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Seed a non-empty list first.
    let seed = make_event(
        "0000000000000000000000000000000000000000000000000000000000000001",
        AUTHOR,
        1_000,
        10002,
        vec![r_tag("wss://write.example/", Some("write"))],
    );
    kernel.ingest_relay_list(seed);
    assert!(
        kernel.author_relay_lists.contains_key(AUTHOR),
        "precondition: the seed list must be cached",
    );
    // Drain the seed's trigger so the assertion below isolates the clear path.
    let _ = kernel.drain_lifecycle_tick();
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        0,
        "precondition: inbox drained before the clear",
    );

    // A newer (higher `created_at`) empty kind:10002 → author cleared NIP-65.
    let clear = make_event(
        "0000000000000000000000000000000000000000000000000000000000000003",
        AUTHOR,
        2_000,
        10002,
        Vec::new(),
    );
    kernel.ingest_relay_list(clear);

    assert!(
        !kernel.author_relay_lists.contains_key(AUTHOR),
        "an empty NIP-65 list for a known author must REMOVE the stale cache entry",
    );
    assert_eq!(
        kernel.lifecycle.pending_trigger_count(),
        1,
        "clearing a cached NIP-65 list must enqueue a recompile trigger so the \
         M2 plan is re-routed off the now-stale relays",
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
    assert!(kernel.active_account.is_none(), "precondition: no active account");

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
