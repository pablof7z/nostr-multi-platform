//! Implicit kind:10002 discovery, current-plan diagnostics accessors, and
//! the T140 (D6) `drain_tick` no-silent-swallow regression.
//!
//! Relocated verbatim out of `subs/mod.rs`'s inline `mod tests` (file-size
//! gate, NMP #169). No assertion, fixture, or test body was changed — only
//! the host module moved. `use super::*;` resolves to the `subs` module just
//! as it did when this lived inside `mod.rs`'s `mod tests`.

use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};

fn pubkey(s: &str) -> String {
    format!("{s:0>64}").chars().take(64).collect()
}

/// Single-author follow interest (kind:1 timeline).
fn follow(id: u64, author: &str) -> LogicalInterest {
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

// ─── implicit kind:10002 discovery ───────────────────────────────────────

fn probe_reqs(frames: &[WireFrame]) -> Vec<&WireFrame> {
    frames
        .iter()
        .filter(
            |f| matches!(f, WireFrame::Req { sub_id, .. } if sub_id.starts_with("mailbox-probe-")),
        )
        .collect()
}

/// An author with no cached mailbox triggers exactly one kind:10002
/// discovery REQ to the indexer set, targeting that author.
#[test]
fn unknown_author_triggers_mailbox_probe() {
    let mut l = SubscriptionLifecycle::new(); // indexer = [purplepag.es]
    let empty = InMemoryMailboxCache::new(); // nothing cached
    l.registry_mut().push(follow(1, "ab01"));

    let frames = l.recompile_and_diff(&empty).expect("compile");
    let probes = probe_reqs(&frames);
    assert_eq!(probes.len(), 1, "exactly one indexer probe expected");
    if let WireFrame::Req {
        relay_url,
        filter_json,
        lifecycle,
        ..
    } = probes[0]
    {
        assert_eq!(relay_url, "wss://purplepag.es");
        assert!(filter_json.contains("10002"));
        assert!(filter_json.contains(&pubkey("ab01")));
        assert!(matches!(lifecycle, InterestLifecycle::OneShot));
    } else {
        panic!("expected a Req frame");
    }
    assert!(l.probed_mailboxes().contains(&pubkey("ab01")));
}

/// A second recompile does NOT re-probe an already-probed author, even
/// though the mailbox never arrived ("nor have tried" — insert-only).
#[test]
fn probed_author_not_reprobed() {
    let mut l = SubscriptionLifecycle::new();
    let empty = InMemoryMailboxCache::new();
    l.registry_mut().push(follow(1, "cd01"));

    let first = l.recompile_and_diff(&empty).expect("compile 1");
    assert_eq!(probe_reqs(&first).len(), 1);

    let second = l.recompile_and_diff(&empty).expect("compile 2");
    assert_eq!(
        probe_reqs(&second).len(),
        0,
        "already-probed author must not re-probe"
    );

    // refresh escape hatch re-probes.
    l.clear_probed_mailboxes();
    let third = l.recompile_and_diff(&empty).expect("compile 3");
    assert_eq!(
        probe_reqs(&third).len(),
        1,
        "clear_probed_mailboxes re-arms discovery"
    );
}

/// An author WITH a cached mailbox is never probed.
#[test]
fn cached_author_never_probed() {
    let mut l = SubscriptionLifecycle::new();
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("ef01"),
        MailboxSnapshot {
            write_relays: vec!["wss://known.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    l.registry_mut().push(follow(1, "ef01"));

    let frames = l.recompile_and_diff(&cache).expect("compile");
    assert_eq!(
        probe_reqs(&frames).len(),
        0,
        "author with cached mailbox must not be probed"
    );
    assert!(l.probed_mailboxes().is_empty());
}

/// Unknown authors split into `ceil(n / MAILBOX_PROBE_BATCH)` probe
/// REQs. Batch-size-aware so it survives tuning the constant.
#[test]
fn many_unknown_authors_batch_into_chunks() {
    let mut l = SubscriptionLifecycle::new();
    let empty = InMemoryMailboxCache::new();
    // Two full batches + a partial → exercises chunking at any batch size.
    let n = MAILBOX_PROBE_BATCH * 2 + 7;
    for i in 0..n as u32 {
        let seed = format!("z{i:05}");
        l.registry_mut().push(follow(u64::from(i) + 1, &seed));
    }
    let frames = l.recompile_and_diff(&empty).expect("compile");
    let probes = probe_reqs(&frames);
    let expected = n.div_ceil(MAILBOX_PROBE_BATCH); // 3
    assert_eq!(
        probes.len(),
        expected,
        "{n} authors / {MAILBOX_PROBE_BATCH} per batch must be {expected} probe REQs",
    );
    assert_eq!(l.probed_mailboxes().len(), n);
}

// ─── current-plan diagnostics accessors (nmp-repl seam) ──────────────────

/// `current_plan_unroutable` is empty before any compile, then reflects
/// the plan's `unroutable_authors` after a recompile.
#[test]
fn current_plan_unroutable_reflects_plan() {
    let mut l = SubscriptionLifecycle::new();
    assert!(l.current_plan_unroutable().is_empty());

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("rt01"),
        MailboxSnapshot {
            write_relays: vec!["wss://known.example".to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    l.registry_mut().push(follow(1, "rt01"));
    l.registry_mut().push(follow(2, "ur01"));

    let _ = l.recompile_and_diff(&cache).expect("compile");
    let unroutable = l.current_plan_unroutable();
    assert!(
        unroutable.contains(&pubkey("ur01")),
        "author with no mailbox + no app-relay must be unroutable; got {unroutable:?}"
    );
    assert!(
        !unroutable.contains(&pubkey("rt01")),
        "author with cached mailbox must be routable"
    );
}

/// `current_plan_frames` is empty before any compile, then materialises
/// one content REQ per `(relay, sub_shape)` — and never a probe REQ
/// (probes live outside `current_plan`).
#[test]
fn current_plan_frames_materialises_full_content_plan() {
    let mut l = SubscriptionLifecycle::new();
    l.set_selection_budget(usize::MAX, usize::MAX);
    assert!(l.current_plan_frames().is_empty());

    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        pubkey("cp01"),
        MailboxSnapshot {
            write_relays: vec![
                "wss://cp-a.example".to_string(),
                "wss://cp-b.example".to_string(),
            ],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    l.registry_mut().push(follow(1, "cp01"));

    let _ = l.recompile_and_diff(&cache).expect("compile");
    let frames = l.current_plan_frames();

    // Expected: exactly one REQ per (relay, sub_shape) in current_plan.
    let plan = l.current_plan.as_ref().expect("plan present");
    let expected: usize = plan.per_relay.values().map(|rp| rp.sub_shapes.len()).sum();
    assert_eq!(
        frames.len(),
        expected,
        "one frame per (relay, sub_shape); got {} want {expected}",
        frames.len()
    );
    // No probe REQ may appear in the materialised content plan.
    for f in &frames {
        if let WireFrame::Req { sub_id, .. } = f {
            assert!(
                !sub_id.starts_with("mailbox-probe-"),
                "current_plan_frames must be content-only; saw probe {sub_id}"
            );
        }
    }
    // Both write relays must be present (selection budget unbounded).
    let relays: std::collections::BTreeSet<String> = frames
        .iter()
        .filter_map(|f| match f {
            WireFrame::Req { relay_url, .. } => Some(relay_url.clone()),
            _ => None,
        })
        .collect();
    assert!(relays.contains("wss://cp-a.example"));
    assert!(relays.contains("wss://cp-b.example"));
}

/// With no indexer configured, discovery is silently skipped (the
/// operator opted out of indexer discovery).
#[test]
fn no_indexer_means_no_probe() {
    let mut l = SubscriptionLifecycle::new();
    l.set_indexer_relays(vec![]);
    let empty = InMemoryMailboxCache::new();
    l.registry_mut().push(follow(1, "aa99"));
    let frames = l.recompile_and_diff(&empty).expect("compile");
    assert_eq!(probe_reqs(&frames).len(), 0);
    assert!(
        l.probed_mailboxes().is_empty(),
        "no probe emitted → nothing marked probed"
    );
}

// ─── T140 (D6) — drain_tick() error path is no longer a silent swallow ───

/// T140 / codex finding #7: `drain_tick` previously did
/// `recompile_and_diff(...).unwrap_or_default()` — every `Err(_)` silently
/// became `Vec::new()` on a now-FFI-visible path (D6 violation).
///
/// This regression test pins the *classification contract*: a trigger
/// enqueued with NO interests registered must NOT panic and must NOT
/// record a `last_planner_error` (the no-interests state is the benign
/// `EmptyInterestSet` steady state, not a genuine error). The genuine
/// structural-error arm (`InvalidShape` / `HashingFailed`) is the explicit
/// `Err(e) => self.last_planner_error = Some(...)` branch in `drain_tick`.
/// Pre-fix, `last_planner_error` did not exist and ALL errors were lost;
/// the existence of the accessor + the benign-vs-genuine split is the
/// observable D6 fix.
#[test]
fn drain_tick_benign_empty_interest_set_does_not_record_planner_error() {
    let mut l = SubscriptionLifecycle::new();
    // Trigger enqueued, but registry is empty → recompile sees no
    // interests. This is the benign steady state.
    l.enqueue_trigger(CompileTrigger::FollowListChanged {
        account_id: AccountId("acct".to_string()),
        new_follows: vec![],
    });
    let mailboxes = InMemoryMailboxCache::new();
    let frames = l.drain_tick(&mailboxes);

    assert!(
        frames.is_empty(),
        "no interests → empty diff (benign), got {} frames",
        frames.len()
    );
    assert_eq!(
        l.last_planner_error(),
        None,
        "T140 D6: the benign EmptyInterestSet state must NOT be recorded \
         as a planner error (only genuine structural errors are surfaced); \
         got {:?}",
        l.last_planner_error()
    );
}
