//! Integration tests for [`EmbedClaimRegistry`]: dedupe, refcounting, and
//! resolution updates through the registry's event-ingest methods.

use nmp_content::{
    ClaimHandle, EmbedClaimRegistry, EmbedClaimSpec, EmbedTarget,
};
use nmp_core::substrate::{KernelEvent, ViewContext};

fn ev(id: &str, kind: u32, content: &str) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "deadbeef".to_string(),
        kind,
        created_at: 1_700_000_000,
        tags: Vec::new(),
        content: content.to_string(),
    }
}

#[test]
fn three_claims_for_same_id_share_one_subscription_slot() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("abcdef".to_string());

    let h1 = EmbedClaimRegistry::claim(&mut state, target.clone()).0;
    let h2 = EmbedClaimRegistry::claim(&mut state, target.clone()).0;
    let h3 = EmbedClaimRegistry::claim(&mut state, target.clone()).0;

    // Single entry in the map — the dedupe contract.
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 1);
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 3);

    // Releasing two of three keeps the entry alive.
    assert!(!EmbedClaimRegistry::release(&mut state, &h1));
    assert!(!EmbedClaimRegistry::release(&mut state, &h2));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 1);
    assert!(EmbedClaimRegistry::is_claimed(&state, &target));

    // Last release tears down the entry.
    assert!(EmbedClaimRegistry::release(&mut state, &h3));
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
    assert!(!EmbedClaimRegistry::is_claimed(&state, &target));
}

#[test]
fn distinct_ids_get_distinct_entries() {
    let mut state = EmbedClaimRegistry::state();
    let a = EmbedTarget::Event("aaa".into());
    let b = EmbedTarget::Event("bbb".into());
    let _ha = EmbedClaimRegistry::claim(&mut state, a.clone());
    let _hb = EmbedClaimRegistry::claim(&mut state, b.clone());
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 2);
}

#[test]
fn view_module_open_returns_empty_payload() {
    let ctx = ViewContext::default();
    let (state, payload) = EmbedClaimRegistry::open(&ctx, EmbedClaimSpec);
    assert!(payload.entries.is_empty());
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
}

#[test]
fn view_module_snapshot_reflects_claims_and_resolution() {
    let ctx = ViewContext::default();
    let (mut state, _payload) = EmbedClaimRegistry::open(&ctx, EmbedClaimSpec);

    let target = EmbedTarget::Event("evt-1".to_string());
    let (_h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (_h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());

    let snapshot = EmbedClaimRegistry::snapshot(&ctx, &state);
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(snapshot.entries[0].0, target);
    assert_eq!(snapshot.entries[0].1, 2);
    assert!(snapshot.entries[0].2.is_none());

    // Resolution arrives via on_event_inserted.
    let delta = EmbedClaimRegistry::on_event_inserted(
        &ctx,
        &mut state,
        &ev("evt-1", 1, "hello"),
    );
    assert!(delta.is_some());

    let snapshot = EmbedClaimRegistry::snapshot(&ctx, &state);
    assert!(snapshot.entries[0].2.is_some());
    let resolved = snapshot.entries[0].2.as_ref().unwrap();
    assert_eq!(resolved.id, "evt-1");
    assert_eq!(resolved.content, "hello");
}

#[test]
fn unclaimed_event_does_not_produce_delta() {
    let ctx = ViewContext::default();
    let (mut state, _) = EmbedClaimRegistry::open(&ctx, EmbedClaimSpec);
    let delta = EmbedClaimRegistry::on_event_inserted(
        &ctx,
        &mut state,
        &ev("uninterested", 1, "x"),
    );
    assert!(delta.is_none());
    let snapshot = EmbedClaimRegistry::snapshot(&ctx, &state);
    assert!(snapshot.entries.is_empty());
}

#[test]
fn on_event_removed_clears_resolution_for_claimed_target() {
    let ctx = ViewContext::default();
    let (mut state, _) = EmbedClaimRegistry::open(&ctx, EmbedClaimSpec);

    let target = EmbedTarget::Event("e1".to_string());
    let (_h, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let _ = EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &ev("e1", 1, "hi"));
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());

    let delta = EmbedClaimRegistry::on_event_removed(
        &ctx,
        &mut state,
        &"e1".to_string(),
    );
    assert!(delta.is_some());
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_none());
}

#[test]
fn handle_id_is_unique_across_claims() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("dup".into());
    let h1: ClaimHandle = EmbedClaimRegistry::claim(&mut state, target.clone()).0;
    let h2: ClaimHandle = EmbedClaimRegistry::claim(&mut state, target.clone()).0;
    let h3: ClaimHandle = EmbedClaimRegistry::claim(&mut state, target).0;
    assert_ne!(h1.handle_id(), h2.handle_id());
    assert_ne!(h2.handle_id(), h3.handle_id());
    assert_ne!(h1.handle_id(), h3.handle_id());
}

#[test]
fn release_one_handle_leaves_others_resolvable() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("ee".into());
    let (h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (_h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());

    let ctx = ViewContext::default();
    let _ = EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &ev("ee", 1, "x"));

    assert!(!EmbedClaimRegistry::release(&mut state, &h1));
    // After releasing one handle, the resolved payload is still there.
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());
}

/// All read-only queries return their "absent" sentinel for a target that
/// was never claimed — `refcount` 0, `is_claimed` false, `resolved` None.
#[test]
fn queries_on_unknown_target_return_absent_sentinels() {
    let state = EmbedClaimRegistry::state();
    let unknown = EmbedTarget::Event("never-claimed".into());
    assert_eq!(EmbedClaimRegistry::refcount(&state, &unknown), 0);
    assert!(!EmbedClaimRegistry::is_claimed(&state, &unknown));
    assert!(EmbedClaimRegistry::resolved(&state, &unknown).is_none());
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
}

/// A released target is again "unknown": its entry is GC'd at refcount 0,
/// so the query sentinels apply just as for a never-seen target.
#[test]
fn queries_after_full_release_report_unknown_target() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("transient".into());
    let (h, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    assert!(EmbedClaimRegistry::release(&mut state, &h));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 0);
    assert!(!EmbedClaimRegistry::is_claimed(&state, &target));
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_none());
}

/// Interleaved claims/releases on distinct targets keep independent
/// refcounts — releasing one target's handle never touches another's.
#[test]
fn distinct_targets_keep_independent_refcounts() {
    let mut state = EmbedClaimRegistry::state();
    let a = EmbedTarget::Event("aaa".into());
    let b = EmbedTarget::Event("bbb".into());

    let (a1, _) = EmbedClaimRegistry::claim(&mut state, a.clone());
    let (_a2, _) = EmbedClaimRegistry::claim(&mut state, a.clone());
    let (_b1, _) = EmbedClaimRegistry::claim(&mut state, b.clone());

    assert_eq!(EmbedClaimRegistry::refcount(&state, &a), 2);
    assert_eq!(EmbedClaimRegistry::refcount(&state, &b), 1);

    // Releasing one of A's handles must leave B untouched.
    assert!(!EmbedClaimRegistry::release(&mut state, &a1));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &a), 1);
    assert_eq!(EmbedClaimRegistry::refcount(&state, &b), 1);
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 2);
}
