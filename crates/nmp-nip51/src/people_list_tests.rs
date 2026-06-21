//! Tests for the kind:30000 follow-set member projection (#1740 step 3).

use super::PeopleListProjection;
use nmp_core::substrate::{EventId, KernelEvent};
use nmp_core::KernelEventObserver;
use std::sync::{Arc, Mutex};

fn projection_for(active: Option<&str>) -> PeopleListProjection {
    let slot = Arc::new(Mutex::new(active.map(|s| s.to_string())));
    PeopleListProjection::new(slot)
}

fn follow_set_event(author: &str, d_tag: &str, members: &[&str]) -> KernelEvent {
    let mut tags: Vec<Vec<String>> = vec![vec!["d".to_string(), d_tag.to_string()]];
    for pk in members {
        tags.push(vec!["p".to_string(), pk.to_string()]);
    }
    KernelEvent {
        id: EventId::from(
            "0000000000000000000000000000000000000000000000000000000000000001".to_string(),
        ),
        author: author.to_string(),
        kind: 30_000,
        created_at: 100,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

const ALICE: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee";
const BOB: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const CAROL: &str = "cc11223344556677889900aabbccddeeff00112233445566778899aabbccddee";

#[test]
fn empty_when_no_active_account() {
    let proj = projection_for(None);
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB]));
    assert!(proj.members("team").is_empty());
}

#[test]
fn members_resolve_for_active_owner() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB, CAROL]));
    let members = proj.members("team");
    assert!(members.contains(BOB));
    assert!(members.contains(CAROL));
    assert_eq!(members.len(), 2);
}

#[test]
fn non_kind30000_ignored() {
    let proj = projection_for(Some(ALICE));
    let mut ev = follow_set_event(ALICE, "team", &[BOB]);
    ev.kind = 1;
    proj.on_kernel_event(&ev);
    assert!(proj.members("team").is_empty());
}

#[test]
fn other_account_list_ignored() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&follow_set_event(CAROL, "team", &[BOB]));
    assert!(proj.members("team").is_empty());
}

#[test]
fn distinct_d_tags_are_separate_lists() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB]));
    proj.on_kernel_event(&follow_set_event(ALICE, "friends", &[CAROL]));
    assert!(proj.members("team").contains(BOB));
    assert!(!proj.members("team").contains(CAROL));
    assert!(proj.members("friends").contains(CAROL));
}

#[test]
fn newer_event_replaces_members() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB, CAROL]));
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB]));
    let members = proj.members("team");
    assert!(members.contains(BOB));
    assert!(!members.contains(CAROL));
    assert_eq!(members.len(), 1);
}

#[test]
fn older_event_does_not_overwrite_newer_members() {
    // Newest-wins (addressable-replaceable): a stale (older `created_at`)
    // kind:30000 for the same `(owner, d)` must NOT replace the newer member set
    // — even when it arrives later. Matches the sibling NIP-51 projections.
    let proj = projection_for(Some(ALICE));
    let mut newer = follow_set_event(ALICE, "team", &[BOB]);
    newer.created_at = 200;
    proj.on_kernel_event(&newer);

    let mut older = follow_set_event(ALICE, "team", &[CAROL]);
    older.created_at = 100; // arrives after, but is older
    proj.on_kernel_event(&older);

    let members = proj.members("team");
    assert!(members.contains(BOB), "newer members are retained");
    assert!(!members.contains(CAROL), "older event must not overwrite");
    assert_eq!(members.len(), 1);
}

#[test]
fn older_event_does_not_fire_on_change() {
    // A stale older event is a no-op → no change notification.
    let proj = projection_for(Some(ALICE));
    let hits = Arc::new(Mutex::new(0usize));
    let hits_cb = Arc::clone(&hits);
    proj.on_change(Box::new(move || {
        *hits_cb.lock().unwrap() += 1;
    }));
    let mut newer = follow_set_event(ALICE, "team", &[BOB]);
    newer.created_at = 200;
    proj.on_kernel_event(&newer);
    assert_eq!(*hits.lock().unwrap(), 1);

    let mut older = follow_set_event(ALICE, "team", &[CAROL]);
    older.created_at = 100;
    proj.on_kernel_event(&older);
    assert_eq!(*hits.lock().unwrap(), 1, "older event fires no change");
}

#[test]
fn unknown_list_is_empty_fail_closed() {
    let proj = projection_for(Some(ALICE));
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB]));
    assert!(proj.members("does-not-exist").is_empty());
}

#[test]
fn on_change_fires_when_members_change() {
    let proj = projection_for(Some(ALICE));
    let hits = Arc::new(Mutex::new(0usize));
    let hits_cb = Arc::clone(&hits);
    proj.on_change(Box::new(move || {
        *hits_cb.lock().unwrap() += 1;
    }));
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB]));
    assert_eq!(*hits.lock().unwrap(), 1);
    // Identical re-delivery does not fire.
    proj.on_kernel_event(&follow_set_event(ALICE, "team", &[BOB]));
    assert_eq!(*hits.lock().unwrap(), 1);
}
