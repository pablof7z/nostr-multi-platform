use super::*;
use nmp_core::nip21::NostrUri;
use nmp_core::substrate::{KernelEvent, ViewContext};

fn ev(id: &str, kind: u32) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "deadbeef".to_string(),
        kind,
        created_at: 1_700_000_000,
        tags: Vec::new(),
        content: "body".to_string(),
    }
}

fn article(id: &str, d: &str) -> KernelEvent {
    let mut e = ev(id, 30023);
    e.tags.push(vec!["d".to_string(), d.to_string()]);
    e
}

#[test]
fn three_claims_for_same_event_share_one_entry() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("abc".into());
    let (h1, r1) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (h2, r2) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (h3, r3) = EmbedClaimRegistry::claim(&mut state, target.clone());
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 1);
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 3);
    assert!(r1.is_none());
    assert!(r2.is_none());
    assert!(r3.is_none());
    assert_ne!(h1.handle_id(), h2.handle_id());
    assert_ne!(h2.handle_id(), h3.handle_id());
}

#[test]
fn last_release_returns_true_and_removes_entry() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("abc".into());
    let (h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    assert!(!EmbedClaimRegistry::release(&mut state, &h1));
    assert!(EmbedClaimRegistry::release(&mut state, &h2));
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
    assert!(!EmbedClaimRegistry::is_claimed(&state, &target));
}

#[test]
fn release_unknown_target_returns_false() {
    let mut state = EmbedClaimRegistry::state();
    let phantom = ClaimHandle {
        target: EmbedTarget::Event("never-claimed".into()),
        handle_id: 99,
    };
    assert!(!EmbedClaimRegistry::release(&mut state, &phantom));
}

/// Finding #4 — a phantom handle for a *live* target (target exists, but
/// the handle id was never issued for it) must NOT decrement the live
/// claim's refcount.
#[test]
fn phantom_handle_for_live_target_does_not_corrupt_refcount() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("live".into());
    let (h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 1);

    let phantom = ClaimHandle {
        target: target.clone(),
        handle_id: 9_999_999,
    };
    // Pre-fix, this decremented the live claim's refcount to 0 and GC'd
    // the entry. It must be a no-op.
    assert!(!EmbedClaimRegistry::release(&mut state, &phantom));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 1);
    assert!(EmbedClaimRegistry::is_claimed(&state, &target));

    // The real handle still releases cleanly to zero.
    assert!(EmbedClaimRegistry::release(&mut state, &h1));
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
}

/// Finding #4 — double-release of the same handle is idempotent: the
/// second release does not decrement a *different* live claim.
#[test]
fn double_release_of_same_handle_is_noop() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("dup".into());
    let (h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (_h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 2);

    // First release of h1: 2 -> 1, not last.
    assert!(!EmbedClaimRegistry::release(&mut state, &h1));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 1);

    // Second release of the SAME handle must not steal h2's refcount.
    assert!(!EmbedClaimRegistry::release(&mut state, &h1));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 1);
    assert!(EmbedClaimRegistry::is_claimed(&state, &target));
}

/// Refcount GCs the entry exactly at zero, not before.
#[test]
fn refcount_gcs_at_zero_only() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("gc".into());
    let (h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (h3, _) = EmbedClaimRegistry::claim(&mut state, target.clone());

    assert!(!EmbedClaimRegistry::release(&mut state, &h2));
    assert!(!EmbedClaimRegistry::release(&mut state, &h1));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 1);
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 1);

    assert!(EmbedClaimRegistry::release(&mut state, &h3));
    assert_eq!(EmbedClaimRegistry::refcount(&state, &target), 0);
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
}

#[test]
fn event_insert_updates_resolution_for_claimed_event() {
    let mut state = EmbedClaimRegistry::state();
    let id = "feedface".to_string();
    let target = EmbedTarget::Event(id.clone());
    let (_h, before) = EmbedClaimRegistry::claim(&mut state, target.clone());
    assert!(before.is_none());

    let ctx = ViewContext::default();
    let delta = EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &ev(&id, 1));
    assert!(delta.is_some());
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());
}

#[test]
fn event_insert_for_unclaimed_target_is_noop() {
    let mut state = EmbedClaimRegistry::state();
    let ctx = ViewContext::default();
    let delta = EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &ev("uninterested", 1));
    assert!(delta.is_none());
    assert_eq!(EmbedClaimRegistry::claim_count(&state), 0);
}

#[test]
fn address_coordinated_embed_resolves_via_d_tag() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Address {
        kind: 30023,
        pubkey: "deadbeef".to_string(),
        identifier: "my-article".to_string(),
    };
    let (_h, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let event = article("art-id", "my-article");

    let ctx = ViewContext::default();
    let delta = EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &event);
    assert!(delta.is_some());
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());
}

/// Finding #6 — removing the underlying event of a claimed `naddr` embed
/// must clear the stale `Address` resolution (the bare-removal path).
#[test]
fn address_target_resolution_cleared_on_bare_event_removal() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Address {
        kind: 30023,
        pubkey: "deadbeef".to_string(),
        identifier: "my-article".to_string(),
    };
    let (_h, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let ctx = ViewContext::default();
    let _ =
        EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &article("art-v1", "my-article"));
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_some());

    // Pre-fix: removing "art-v1" only cleared Event targets, leaving the
    // Address resolution stale. It must now clear.
    let delta = EmbedClaimRegistry::on_event_removed(&ctx, &mut state, &"art-v1".to_string());
    assert!(delta.is_some());
    assert!(EmbedClaimRegistry::resolved(&state, &target).is_none());
    // Entry still claimed — only the resolution was cleared.
    assert!(EmbedClaimRegistry::is_claimed(&state, &target));
}

/// Finding #6 — replace path: a newer article version with the same `d`
/// re-resolves the `Address` target to the new id (no stale `id`).
#[test]
fn address_target_re_resolves_on_event_replace() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Address {
        kind: 30023,
        pubkey: "deadbeef".to_string(),
        identifier: "my-article".to_string(),
    };
    let (_h, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let ctx = ViewContext::default();
    let _ =
        EmbedClaimRegistry::on_event_inserted(&ctx, &mut state, &article("art-v1", "my-article"));
    assert_eq!(
        EmbedClaimRegistry::resolved(&state, &target).unwrap().id,
        "art-v1"
    );

    let delta = EmbedClaimRegistry::on_event_replaced(
        &ctx,
        &mut state,
        &"art-v1".to_string(),
        &article("art-v2", "my-article"),
    );
    assert!(delta.is_some());
    assert_eq!(
        EmbedClaimRegistry::resolved(&state, &target).unwrap().id,
        "art-v2"
    );
}

#[test]
fn from_uri_skips_profile_returns_event_or_address() {
    let profile = NostrUri::Profile {
        pubkey: "p".into(),
        relays: vec![],
    };
    assert!(EmbedTarget::from_uri(&profile).is_none());

    let event = NostrUri::Event {
        event_id: "e".into(),
        relays: vec![],
        author: None,
        kind: None,
    };
    assert!(matches!(
        EmbedTarget::from_uri(&event),
        Some(EmbedTarget::Event(_))
    ));

    let addr = NostrUri::Address {
        identifier: "d".into(),
        pubkey: "p".into(),
        kind: 30023,
        relays: vec![],
    };
    assert!(matches!(
        EmbedTarget::from_uri(&addr),
        Some(EmbedTarget::Address { .. })
    ));
}

#[test]
fn snapshot_includes_refcount_and_resolution() {
    let mut state = EmbedClaimRegistry::state();
    let target = EmbedTarget::Event("xyz".into());
    let (_h1, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let (_h2, _) = EmbedClaimRegistry::claim(&mut state, target.clone());
    let ctx = ViewContext::default();
    let snap = EmbedClaimRegistry::snapshot(&ctx, &state);
    assert_eq!(snap.entries.len(), 1);
    assert_eq!(snap.entries[0].1, 2);
    assert!(snap.entries[0].2.is_none());
}
