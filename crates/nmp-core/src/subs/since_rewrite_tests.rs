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
    }
}

fn timeline_interest_with_since(id: u64, author: &str, since: u64) -> LogicalInterest {
    let mut i = timeline_interest(id, author);
    i.shape.since = Some(since);
    i
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

// ─── 1) Basic rewrite ────────────────────────────────────────────────────────

#[test]
fn rewrites_since_to_watermark_plus_one_when_filter_has_no_since() {
    let (mut l, mailboxes) = lifecycle_with_mailbox("a", &["wss://r1"]);
    // Cache has events for this filter with newest created_at = 1700.
    l.set_watermark_fn(Arc::new(|_shape: &InterestShape| Some(1700)));
    l.registry_mut().push(timeline_interest(1, "a"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);

    assert!(!filters.is_empty(), "expected at least one REQ");
    for filter in &filters {
        assert!(
            filter.contains("\"since\":1701"),
            "since not rewritten to watermark+1 in filter {filter}",
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

    assert!(!filters.is_empty(), "expected at least one REQ for ephemeral");
    for filter in &filters {
        assert!(
            !filter.contains("\"since\""),
            "ephemeral filter must not be since-rewritten; got {filter}",
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
    l.registry_mut().push(timeline_interest(1, "a"));

    let frames = l.recompile_and_diff(&mailboxes).expect("compile");
    let filters = req_filters(&frames);
    let relays = relays_with_req(&frames);

    assert_eq!(relays.len(), 3, "expected REQs to all 3 author write relays");
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
