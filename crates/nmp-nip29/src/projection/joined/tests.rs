use super::*;
use crate::kinds::KIND_PUT_USER;

fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.to_string(),
        author: "relay".to_string(),
        kind,
        created_at,
        tags,
        content: String::new(),
        relay_provenance: vec!["wss://groups.example.com".to_string()],
    }
}

fn d(room: &str) -> Vec<String> {
    vec!["d".to_string(), room.to_string()]
}

fn p(pubkey: &str) -> Vec<String> {
    vec!["p".to_string(), pubkey.to_string()]
}

#[test]
fn only_groups_containing_active_pubkey_are_projected() {
    let active = "a".repeat(64);
    let proj = JoinedGroupsProjection::new(&active);
    proj.on_kernel_event(&event(
        "1",
        KIND_GROUP_MEMBERS,
        100,
        vec![d("mine"), p(&active), p(&"b".repeat(64))],
    ));
    proj.on_kernel_event(&event(
        "2",
        KIND_GROUP_MEMBERS,
        100,
        vec![d("other"), p(&"c".repeat(64))],
    ));

    let snapshot = proj.snapshot();
    assert_eq!(snapshot.groups.len(), 1);
    assert_eq!(snapshot.groups[0].group_id, "mine");
    assert_eq!(snapshot.groups[0].member_count, 2);
    assert!(snapshot.groups[0].is_member);
}

#[test]
fn admin_membership_sets_admin_flag_even_without_member_snapshot() {
    let active = "a".repeat(64);
    let proj = JoinedGroupsProjection::new_for_host(&active, "wss://h");
    proj.on_kernel_event(&event(
        "1",
        KIND_GROUP_ADMINS,
        100,
        vec![d("admins"), p(&active)],
    ));

    let group = proj.snapshot().groups.pop().expect("admin group appears");
    assert_eq!(group.host_relay_url, "wss://h");
    assert!(group.is_admin);
    assert!(!group.is_member);
    assert_eq!(group.admin_count, 1);
}

#[test]
fn kind_9000_does_not_mutate_joined_status() {
    let active = "a".repeat(64);
    let proj = JoinedGroupsProjection::new_for_host(&active, "wss://h");
    proj.on_kernel_event(&event(
        "1",
        KIND_PUT_USER,
        100,
        vec![vec!["h".to_string(), "room".to_string()], p(&active)],
    ));

    assert!(proj.snapshot().groups.is_empty());
}

#[test]
fn newer_replaceable_event_supersedes_older_one() {
    let active = "a".repeat(64);
    let proj = JoinedGroupsProjection::new_for_host(&active, "wss://h");
    proj.on_kernel_event(&event(
        "1",
        KIND_GROUP_MEMBERS,
        100,
        vec![d("room"), p(&active)],
    ));
    proj.on_kernel_event(&event(
        "2",
        KIND_GROUP_MEMBERS,
        200,
        vec![d("room"), p(&"b".repeat(64))],
    ));

    assert!(
        proj.snapshot().groups.is_empty(),
        "newer 39002 without active pubkey must remove the row"
    );
}

#[test]
fn equal_timestamp_tie_breaks_by_descending_event_id() {
    let active = "a".repeat(64);
    let proj = JoinedGroupsProjection::new_for_host(&active, "wss://h");
    proj.on_kernel_event(&event(
        "1",
        KIND_GROUP_MEMBERS,
        100,
        vec![d("room"), p(&active)],
    ));
    proj.on_kernel_event(&event(
        "0",
        KIND_GROUP_MEMBERS,
        100,
        vec![d("room"), p(&"b".repeat(64))],
    ));

    assert_eq!(proj.snapshot().groups.len(), 1);
}
