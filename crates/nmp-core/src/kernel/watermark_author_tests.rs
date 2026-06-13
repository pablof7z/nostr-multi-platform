//! V-118 regression tests — author-aware watermark rewrite.
//!
//! The T129 watermark_fn must compute `min(per-author newest)` across all
//! authors in a shape, and return `None` (no floor) if any author has no
//! stored events. The previous `KindTime` path returned `newest-from-anyone`,
//! which could floor a newly-followed author above all their historical events
//! — their past notes would never be backfilled.
//!
//! These tests exercise the *kernel-level* watermark_fn (the closure installed
//! in `Kernel::with_optional_publish_store_and_path` that captures the live
//! `EventStore`) by inserting raw events into the kernel's store and then
//! recompiling against a multi-author shape. They must FAIL on the pre-fix
//! master (which uses the author-blind `KindTime` branch) and pass after the
//! fix lands.

use std::sync::Arc;

use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use crate::store::{RawEvent, VerifiedEvent};

// ─── helpers ────────────────────────────────────────────────────────────────

fn pubkey(seed: &str) -> String {
    format!("{seed:0>64}").chars().take(64).collect()
}

fn make_relay(n: u8) -> String {
    format!("wss://r{n}.example")
}

/// Build a mailbox cache where each author declares exactly one write relay.
fn mailboxes_for(pairs: &[(&str, u8)]) -> InMemoryMailboxCache {
    let mut mc = InMemoryMailboxCache::new();
    for (author, relay_n) in pairs {
        mc.put(
            pubkey(author),
            MailboxSnapshot {
                write_relays: vec![make_relay(*relay_n)],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
    }
    mc
}

/// Insert a single kind:1 event from `author_seed` at unix timestamp `ts`.
/// `id_byte` must be unique per test so the store does not de-dup.
fn insert_event(
    store: &Arc<dyn crate::store::EventStore>,
    author_seed: &str,
    ts: u64,
    id_byte: u8,
) {
    let raw = RawEvent {
        id: format!("{id_byte:02x}{}", "00".repeat(31)),
        pubkey: pubkey(author_seed),
        created_at: ts,
        kind: 1,
        tags: vec![],
        content: String::new(),
        sig: "aa".repeat(64),
    };
    store
        .insert(VerifiedEvent::from_raw_unchecked(raw), &"wss://r0/".to_string(), 0)
        .expect("insert must succeed");
}

/// Build a two-author `LogicalInterest` (kind:1) with the given interest id.
fn two_author_interest(id: u64, author_a: &str, author_b: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author_a), pubkey(author_b)]
                .into_iter()
                .collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Build a single-author `LogicalInterest` (kind:1).
fn one_author_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
        is_indexer_discovery: false,
    }
}

/// Build a single-author `LogicalInterest` with an explicit `since` floor.
/// Used by tests that need to verify the watermark-raise path (owner decision
/// #1281: since=None interests are exempt, only Some(t) floors are raised).
fn one_author_interest_with_since(id: u64, author: &str, since: u64) -> LogicalInterest {
    let mut i = one_author_interest(id, author);
    i.shape.since = Some(since);
    i
}

/// Build a two-author `LogicalInterest` with an explicit `since` floor.
fn two_author_interest_with_since(
    id: u64,
    author_a: &str,
    author_b: &str,
    since: u64,
) -> LogicalInterest {
    let mut i = two_author_interest(id, author_a, author_b);
    i.shape.since = Some(since);
    i
}

/// Extract every `since` value from REQ frames in a filter JSON string.
fn since_from_filter(filter_json: &str) -> Option<u64> {
    // filter_json looks like `{"kinds":[1],"authors":[...],"since":1234}`.
    // Parse minimally: find `"since":` and read the integer.
    let needle = "\"since\":";
    let start = filter_json.find(needle)? + needle.len();
    let rest = &filter_json[start..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn req_filters_from_frames(frames: &[crate::subs::wire::WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            crate::subs::wire::WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect()
}

// ─── test 1: multi-author shape with A having events, B having none ──────────
//
// The planner groups authors per relay — a multi-author `SubShape` only arises
// when two authors declare the SAME write relay. We put A and B on the same
// relay so the compiled sub-shape has `authors = {A, B}`.
//
// Expected behaviour (Option B):
//   Author A newest = 100 → per-author watermark = 100.
//   Author B newest = None → floor is UNSAFE for B → return None.
//   Result: NO `since` in the REQ.
//
// On pre-fix master (KindTime branch):
//   The branch queries KindTime over kind:1 from *anyone*, finds A's t=100,
//   returns 100, and the REQ carries `"since":101` — incorrectly excluding
//   all of B's past events.
#[test]
fn multi_author_no_floor_when_any_author_has_no_events() {
    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);
    let store = kernel.event_store_handle();

    // Only author A has stored events; B is a newly-followed author with none.
    insert_event(&store, "a", 100, 0x01);

    // Both A and B declare the same write relay (r1) so the planner produces
    // a single merged sub-shape {authors:[A,B], kinds:[1]} on that relay.
    kernel
        .lifecycle_mut()
        .set_selection_budget(usize::MAX, usize::MAX);
    let mailboxes = mailboxes_for(&[("a", 1), ("b", 1)]);
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(two_author_interest(1, "a", "b"));

    let frames = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("compile");
    let filters = req_filters_from_frames(&frames);

    assert!(
        !filters.is_empty(),
        "expected REQ frames; got {frames:?}"
    );
    // The merged shape has authors [A, B] — since B has no events the floor
    // is unsafe and the watermark_fn must return None.
    let multi_author_filters: Vec<&String> = filters
        .iter()
        .filter(|f| {
            // A multi-author sub-shape carries both pubkeys in `authors`.
            f.contains(&pubkey("a")[..8]) && f.contains(&pubkey("b")[..8])
        })
        .collect();
    assert!(
        !multi_author_filters.is_empty(),
        "expected at least one REQ with both A and B in authors; got {filters:?}"
    );
    for f in &multi_author_filters {
        assert!(
            since_from_filter(f).is_none(),
            "multi-author shape with a newcomer (B) must NOT carry `since`; got {f}"
        );
    }
}

// ─── test 2: multi-author shape where both authors have events ────────────────
//
// Expected behaviour (owner decision #1281: since=None exempt from rewrite):
//   Interest carries since=1 (an explicit lower bound below both watermarks).
//   Author A newest = 100, Author B newest = 50.
//   min(100, 50) = 50 → floor = 51.
//   The interest's existing since=1 is below the floor → raised to 51.
//   Result: REQ carries `"since":51`.
//
// This validates that the kernel watermark_fn computes min(per-author) correctly
// AND that the rewrite raises an existing Some(t) floor to the watermark.
//
// On pre-fix master (KindTime branch):
//   KindTime finds the global max which is A's t=100 → floor = 101 (wrong).
#[test]
fn multi_author_since_is_min_of_per_author_watermarks() {
    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);
    let store = kernel.event_store_handle();

    // A has events at t=100 and B at t=50.
    insert_event(&store, "a", 100, 0x02);
    insert_event(&store, "b", 50, 0x03);

    // Both authors share relay r1 so the planner produces a single merged
    // sub-shape rather than per-relay shapes; raise the budget so no relay
    // is dropped.
    kernel.lifecycle_mut().set_selection_budget(usize::MAX, usize::MAX);

    let mailboxes = mailboxes_for(&[("a", 1), ("b", 1)]);
    // since=1 provides an explicit floor so the watermark rewrite applies
    // (#1281: since=None interests are exempt; only Some(t) floors are raised).
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(two_author_interest_with_since(2, "a", "b", 1));

    let frames = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("compile");
    let filters = req_filters_from_frames(&frames);

    assert!(!filters.is_empty(), "expected REQ frames; got {frames:?}");
    for f in &filters {
        let since = since_from_filter(f);
        assert_eq!(
            since,
            Some(51),
            "since must be min(50,100)+1 = 51; got {f}"
        );
    }
}

// ─── test 3: single-author watermark is unchanged (regression guard) ──────────
//
// The single-author `AuthorKind` path must continue to produce the correct
// watermark (newest stored event + 1) after the fix.
//
// Owner decision #1281: since=None interests are exempt from the rewrite.
// We supply since=1 so the watermark can be applied and the raising behaviour
// is exercised on the single-author code path.
#[test]
fn single_author_watermark_unchanged_after_fix() {
    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);
    let store = kernel.event_store_handle();

    insert_event(&store, "a", 200, 0x04);

    let mailboxes = mailboxes_for(&[("a", 3)]);
    // since=1 so the watermark rewrite applies (#1281 exempts since=None).
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(one_author_interest_with_since(3, "a", 1));

    let frames = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("compile");
    let filters = req_filters_from_frames(&frames);

    assert!(!filters.is_empty(), "expected REQ frames; got {frames:?}");
    for f in &filters {
        let since = since_from_filter(f);
        assert_eq!(
            since,
            Some(201),
            "single-author watermark must be newest+1 = 201; got {f}"
        );
    }
}

// ─── test 4: single-author with no events returns None (no since) ─────────────
//
// If the store is empty the first REQ must have no `since` — otherwise a brand
// new user could never backfill.
#[test]
fn single_author_no_floor_when_store_is_empty() {
    let mut kernel = Kernel::new(crate::relay::DEFAULT_VISIBLE_LIMIT);

    let mailboxes = mailboxes_for(&[("a", 4)]);
    kernel
        .lifecycle_mut()
        .registry_mut()
        .push(one_author_interest(4, "a"));

    let frames = kernel
        .lifecycle_mut()
        .recompile_and_diff(&mailboxes)
        .expect("compile");
    let filters = req_filters_from_frames(&frames);

    assert!(!filters.is_empty(), "expected REQ frames; got {frames:?}");
    for f in &filters {
        assert!(
            since_from_filter(f).is_none(),
            "empty store → no since; got {f}"
        );
    }
}
