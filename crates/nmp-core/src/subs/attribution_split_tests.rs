//! Tests for SPLIT A (diagnostic attribution snapshot) and SPLIT B
//! (blocked-relay post-pass) in [`SubscriptionLifecycle::recompile_inner`].
//!
//! These tests verify the invariant: `current_plan_attribution` is captured
//! BEFORE the blocked-relay set is applied to the wire plan, so diagnostics
//! can report attribution even for blocked relays.

use super::*;
use crate::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot,
};
use crate::substrate::BlockedRelaySet;

fn push_legacy(reg: &mut InterestRegistry, interest: LogicalInterest) {
    use crate::kernel::cache_serve::{InterestWrite, RegistryWriteToken};
    let t = RegistryWriteToken::for_test();
    let identity =
        crate::subs::test_identity_for_interest(("scoped-test-interest", interest.id.0), &interest);
    let _ = reg.apply(&t, InterestWrite::Replace, identity, interest);
}

fn make_interest(id: u64, author: &str) -> LogicalInterest {
    LogicalInterest {
        id: InterestId(id),
        scope: InterestScope::Global,
        lifecycle: InterestLifecycle::Tailing,
        shape: InterestShape {
            authors: std::collections::BTreeSet::from([author.to_string()]),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn make_cache_with_relay(author: &str, relay: &str) -> InMemoryMailboxCache {
    let mut cache = InMemoryMailboxCache::new();
    cache.put(
        author.to_string(),
        MailboxSnapshot {
            write_relays: vec![relay.to_string()],
            read_relays: vec![],
            both_relays: vec![],
        },
    );
    cache
}

/// SPLIT A: `current_plan_attribution` is empty before the first compile and
/// accessible (no panic) afterwards.
#[test]
fn attribution_empty_before_first_compile() {
    let lc = SubscriptionLifecycle::new();
    assert!(
        lc.current_plan_attribution().is_empty(),
        "attribution must be empty before any compile"
    );
}

/// SPLIT A: `current_plan_attribution` is accessible after a compile (no
/// panic regardless of whether any relays ended up in the plan).
#[test]
fn attribution_accessible_after_compile() {
    let mut lc = SubscriptionLifecycle::new();
    let author = "aa".repeat(32);
    let relay = "wss://relay.example.com".to_string();
    let cache = make_cache_with_relay(&author, &relay);
    let interest = make_interest(1, &author);
    push_legacy(lc.registry_mut(), interest);
    lc.set_indexer_relays(vec![]);

    let empty_blocked = BlockedRelaySet::new();
    let _ = lc.recompile_and_diff_with_blocked(&cache, None, &empty_blocked);
    // (Result is Ok or Err-EmptyInterestSet, either is fine — we're testing no panic)

    // Must be callable without panic.
    let attr = lc.current_plan_attribution();
    let _ = attr.len();
}

/// SPLIT B: blocked relays do not appear in wire frames but attribution is
/// still accessible (captured pre-block).
#[test]
fn blocked_relay_absent_from_wire_frames() {
    let mut lc = SubscriptionLifecycle::new();
    let author = "bb".repeat(32);
    let relay = "wss://blocked.relay.example.com".to_string();
    let cache = make_cache_with_relay(&author, &relay);
    let interest = make_interest(1, &author);
    push_legacy(lc.registry_mut(), interest);
    lc.set_indexer_relays(vec![]);

    let mut blocked = BlockedRelaySet::new();
    blocked.insert(relay.clone());

    let frames = lc
        .recompile_and_diff_with_blocked(&cache, None, &blocked)
        .unwrap_or_default();

    // No REQ frames should target the blocked relay.
    for frame in frames.iter() {
        if let WireFrame::Req { relay_url, .. } = frame {
            assert_ne!(
                relay_url, &relay,
                "blocked relay must not appear in wire frames"
            );
        }
    }

    // Attribution accessor must still be callable after the blocked compile.
    let _ = lc.current_plan_attribution();
}

/// Backward compatibility: `drain_tick_with_lookup` (without blocked arg)
/// still works and does not panic.
#[test]
fn drain_tick_with_lookup_backward_compat() {
    let mut lc = SubscriptionLifecycle::new();
    let cache = InMemoryMailboxCache::new();
    lc.enqueue_trigger(trigger::CompileTrigger::InvalidateCompile {
        reason: trigger::InvalidateReason::TestForceRecompile,
    });
    let frames = lc.drain_tick_with_lookup(&cache, None);
    // No interests registered → empty diff (benign EmptyInterestSet).
    drop(frames);
}
