//! T129 — `addSinceFromCache` semantics.
//!
//! When a subscription is (re)opened the kernel rewrites each filter's
//! `since` to `max(filter.since, watermark + 1)` so the relay REQ does NOT
//! re-fetch events already on disk. Mirrors NDK
//! `subscription/index.ts:537 opts.addSinceFromCache` but defaults to enabled
//! here — NMP always has a cache.
//!
//! The rewrite happens inside [`SubscriptionLifecycle::recompile_and_diff`]
//! between the M2 compiler and the wire-emitter, AFTER `coverage_hook`
//! but BEFORE `plan_diff`. The rewrite is gated by
//! [`SubscriptionLifecycle::set_watermark_fn`] — without a watermark fn
//! installed, behaviour is unchanged (legacy lifecycle tests stay green).
//!
//! Ephemeral kinds (20000-29999) are SKIPPED — the event store does not
//! persist them so the watermark is meaningless (matches NDK 5afbd245).

use std::sync::Arc;

use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use crate::subs::wire::WireFrame;
use crate::subs::SubscriptionLifecycle;

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

fn timeline_interest(id: u64, author: &str) -> LogicalInterest {
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

fn timeline_interest_with_since(id: u64, author: &str, since: u64) -> LogicalInterest {
    let mut i = timeline_interest(id, author);
    i.shape.since = Some(since);
    i
}

/// A OneShot (backfill) interest with no since — models an all-time history
/// fetch where T129 narrowing must NOT be applied (#1281).
fn backfill_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            kinds: [1u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

fn ephemeral_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: [pubkey(author)].into_iter().collect(),
            // 22242 — NIP-42 AUTH ephemeral kind.
            kinds: [22242u32].into_iter().collect(),
            ..Default::default()
        },
        hints: Vec::new(),
        lifecycle: InterestLifecycle::OneShot,
        is_indexer_discovery: false,
    }
}

/// Construct a lifecycle plus an `InMemoryMailboxCache` carrying one author's
/// write-relay set. T132 moved mailbox ownership out of the lifecycle, so the
/// cache is now caller-owned and passed into `recompile_and_diff`.
fn lifecycle_with_mailbox(
    author: &str,
    relays: &[&str],
) -> (SubscriptionLifecycle, InMemoryMailboxCache) {
    let lifecycle = SubscriptionLifecycle::new();
    let mut mailboxes = InMemoryMailboxCache::new();
    mailboxes.put(
        pubkey(author),
        MailboxSnapshot {
            write_relays: relays.iter().map(|r| (*r).to_string()).collect(),
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    (lifecycle, mailboxes)
}

/// Extract every `WireFrame::Req`'s `filter_json` (newest-first ordering not
/// required — the assertions inspect substrings).
fn req_filters(frames: &[WireFrame]) -> Vec<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { filter_json, .. } => Some(filter_json.clone()),
            _ => None,
        })
        .collect()
}

/// Number of distinct relay URLs that received a REQ.
fn relays_with_req(frames: &[WireFrame]) -> std::collections::BTreeSet<String> {
    frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect()
}

// ─── 1) since=None exemption (#1281) ─────────────────────────────────────────
//
// Owner decision #1281 (2026-06-13, refined): the backfill exemption applies
// ONLY to non-Tailing (OneShot/backfill) interests. A non-Tailing since=None
// interest requests all-time history; raising it to watermark+1 would
// silently prevent the relay from returning events older than the local store.
//
// A TAILING since=None interest is a live feed that has not yet expressed a
// lower bound. T129 applies normally — we narrow it to watermark+1 so the
// relay does not re-send already-cached events (the core T129 optimisation).

/// Non-Tailing (backfill/OneShot) + since=None: watermark rewrite must NOT
/// introduce a lower bound (full history requested).
#[test]
fn since_none_stays_none_for_backfill_interest_after_watermark_rewrite() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Cache has events up to ts=1700 for this filter, but the interest is a
    // backfill (OneShot, lifecycle!=Tailing) with no since. The watermark
    // rewrite must NOT introduce a lower bound.
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    l.registry_mut().push(backfill_interest(1, "a"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected at least one REQ");
    for filter in &filters {
        assert!(
            !filter.contains("\"since\""),
            "backfill (OneShot) since=None must remain absent after watermark rewrite (#1281); got {filter}",
        );
    }
}

/// Tailing + since=None: T129 applies — rewrite to watermark+1 so the live
/// feed does not re-request already-cached events on reconnect/recompile.
#[test]
fn tailing_since_none_is_narrowed_to_watermark_plus_one() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Watermark = 1700; Tailing interest with no explicit since.
    // T129 must narrow it to since=1701 so the relay skips cached events.
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    l.registry_mut().push(timeline_interest(1, "a")); // lifecycle: Tailing

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected at least one REQ");
    for filter in &filters {
        assert!(
            filter.contains("\"since\":1701"),
            "Tailing since=None must be narrowed to watermark+1 by T129; got {filter}",
        );
    }
}

#[test]
fn some_since_is_raised_to_watermark_plus_one() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Interest has an explicit since=500 (older than the watermark).
    // Watermark = 1700, so the floor is 1701.  The rewrite must raise it.
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    l.registry_mut()
        .push(timeline_interest_with_since(1, "a", 500));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected at least one REQ");
    for filter in &filters {
        assert!(
            filter.contains("\"since\":1701"),
            "existing since below watermark must be raised to watermark+1; got {filter}",
        );
    }
}

// ─── 2) No regression on first open (empty store) ────────────────────────────

#[test]
fn does_not_rewrite_when_watermark_is_none() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Empty cache: watermark fn returns None for every shape.
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| None));
    l.registry_mut().push(timeline_interest(1, "a"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected at least one REQ");
    for filter in &filters {
        assert!(
            !filter.contains("\"since\""),
            "no since should appear when watermark is None; got {filter}",
        );
    }
}

// ─── 3) User-set since wins if newer than watermark ──────────────────────────

#[test]
fn user_since_wins_when_newer_than_watermark() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Watermark = 1500, user explicit since = 1800 (newer).
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1500)));
    l.registry_mut()
        .push(timeline_interest_with_since(1, "a", 1800));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected at least one REQ");
    for filter in &filters {
        assert!(
            filter.contains("\"since\":1800"),
            "user-set since (newer) must win; got {filter}",
        );
        assert!(
            !filter.contains("\"since\":1501"),
            "must not downgrade from user since to watermark+1; got {filter}",
        );
    }
}

// ─── 4) Ephemeral kinds skip the rewrite ─────────────────────────────────────

#[test]
fn ephemeral_kinds_skip_since_rewrite() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Even though watermark fn would return Some(1700), ephemeral kinds
    // (20000-29999) must SKIP the rewrite — the event store doesn't persist
    // them so the watermark is meaningless.
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    l.registry_mut().push(ephemeral_interest(1, "a"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(
        !filters.is_empty(),
        "expected at least one REQ for ephemeral"
    );
    for filter in &filters {
        assert!(
            !filter.contains("\"since\""),
            "ephemeral filter must not be since-rewritten; got {filter}",
        );
    }
}

// ─── V-118: author-blind KindTime regression guard ───────────────────────────
//
// These tests use mock watermark fns that mimic what the corrected kernel
// watermark_fn produces for multi-author shapes. They pin `apply_watermark_rewrite`
// behaviour (not the kernel-level fn — those live in watermark_author_tests.rs).

/// A two-author timeline interest for testing.
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

/// Build a mailbox cache with two authors BOTH on the same relay.
///
/// This is required for a multi-author `SubShape` to appear in the compiled
/// plan — the planner groups authors per relay, so two authors with different
/// write relays produce separate single-author shapes, never a merged one.
fn lifecycle_with_two_authors_shared_relay(
    author_a: &str,
    author_b: &str,
) -> (SubscriptionLifecycle, InMemoryMailboxCache) {
    let mut lifecycle = SubscriptionLifecycle::new();
    // Disable selection budget so neither author is pruned.
    lifecycle.set_selection_budget(usize::MAX, usize::MAX);
    let mut mailboxes = InMemoryMailboxCache::new();
    for author in [author_a, author_b] {
        mailboxes.put(
            pubkey(author),
            MailboxSnapshot {
                write_relays: vec!["wss://shared1".to_string()],
                read_relays: vec![],
                both_relays: vec![],
            },
        );
    }
    (lifecycle, mailboxes)
}

/// When the watermark fn returns None for a multi-author shape (because one
/// author has no stored events), the rewrite must NOT add a `since`.
///
/// Uses a shared-relay mailbox so the planner emits a merged `{authors:[A,B]}`
/// sub-shape. With separate relays the planner produces two single-author
/// shapes and this test would exercise the single-author path instead.
#[test]
fn multi_author_no_since_when_watermark_fn_returns_none() {
    let (mut l, mailboxes) = lifecycle_with_two_authors_shared_relay("a", "b");
    // Watermark fn returns None for any multi-author shape (simulating the
    // Option-B behaviour when B has no stored events).
    l.set_watermark_fn(Arc::new(|shape: &InterestShape| {
        if shape.authors.len() > 1 {
            None
        } else {
            Some(1700)
        }
    }));
    l.registry_mut().push(two_author_interest(10, "a", "b"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected REQ frames");
    // Find the merged shape (has both pubkeys in the authors array).
    let multi_author: Vec<&String> = filters
        .iter()
        .filter(|f| f.contains(&pubkey("a")[..8]) && f.contains(&pubkey("b")[..8]))
        .collect();
    assert!(
        !multi_author.is_empty(),
        "expected a merged 2-author REQ; got {filters:?}"
    );
    for f in &multi_author {
        assert!(
            !f.contains("\"since\""),
            "multi-author shape with watermark=None must not carry `since`; got {f}"
        );
    }
}

/// When the watermark fn returns the minimum of per-author watermarks, the
/// rewrite must use that minimum (not a higher value).
///
/// Owner decision #1281: since=None interests are exempt from the T129 rewrite.
/// The interest must carry an explicit since so the watermark can be applied.
#[test]
fn multi_author_since_is_min_watermark_plus_one() {
    let (mut l, mailboxes) = lifecycle_with_two_authors_shared_relay("a", "b");
    // Watermark fn returns the min(100, 50) = 50 for multi-author shapes.
    l.set_watermark_fn(Arc::new(|shape: &InterestShape| {
        if shape.authors.len() > 1 {
            Some(50) // min of per-author watermarks
        } else {
            Some(1700) // single-author path unchanged
        }
    }));
    // since=1 so the watermark rewrite applies (#1281: since=None is exempt).
    let mut interest = two_author_interest(11, "a", "b");
    interest.shape.since = Some(1);
    l.registry_mut().push(interest);

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected REQ frames");
    let multi_author: Vec<&String> = filters
        .iter()
        .filter(|f| f.contains(&pubkey("a")[..8]) && f.contains(&pubkey("b")[..8]))
        .collect();
    assert!(
        !multi_author.is_empty(),
        "expected a merged 2-author REQ; got {filters:?}"
    );
    for f in &multi_author {
        assert!(
            f.contains("\"since\":51"),
            "multi-author watermark+1 must be 51; got {f}"
        );
    }
}

// ─── 5) Multi-relay consistency — all REQs share the rewritten since ─────────

#[test]
fn multi_relay_emits_identical_rewritten_since() {
    // Three relays carry the same author's events; all three REQs must use
    // the same rewritten since (1701).
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1", "wss://r2", "wss://r3"]);
    // The greedy selector caps coverage at `max_per_user` relays per author;
    // raise it above the test fanout (3) so this watermark assertion is not
    // confounded by selection-induced relay dropping.
    l.set_selection_budget(usize::MAX, usize::MAX);
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    // since=500 so the watermark rewrite applies (#1281: since=None is exempt).
    l.registry_mut()
        .push(timeline_interest_with_since(1, "a", 500));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);
    let relays = relays_with_req(&frames);

    assert_eq!(
        relays.len(),
        3,
        "expected REQs to all 3 author write relays"
    );
    assert_eq!(
        filters.len(),
        3,
        "expected one REQ per write relay; got {filters:?}",
    );
    for filter in &filters {
        assert!(
            filter.contains("\"since\":1701"),
            "every relay's REQ must carry watermark+1; got {filter}",
        );
    }
}
