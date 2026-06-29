//! Unit tests for `GroupRosterProjection`.
//!
//! Lives in a sibling file so the production `roster.rs` stays under the
//! AGENTS.md 500-LoC ceiling. Direct `on_kernel_event` injection — no mock
//! relay infrastructure needed.

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;

use super::{GroupRosterProjection, GroupRosterSnapshot};
use crate::kinds::{KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_ROLES};

const HOST: &str = "wss://groups.example.com";
const GROUP: &str = "rust-nostr";

fn pk(n: u8) -> String {
    std::iter::repeat(char::from(b'a' + n % 6))
        .take(64)
        .collect()
}

fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: format!("relay-of-{id}"),
        kind,
        created_at,
        tags,
        content: String::new(),
        relay_provenance: Vec::new(),
    }
}

fn d_tag(local_id: &str) -> Vec<String> {
    vec!["d".into(), local_id.into()]
}

fn p_tag(pubkey: &str, roles: &[&str]) -> Vec<String> {
    let mut t = vec!["p".into(), pubkey.into()];
    t.extend(roles.iter().map(|r| r.to_string()));
    t
}

#[test]
fn fresh_projection_yields_empty_snapshot() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    let snap = proj.snapshot();
    assert_eq!(snap.host_relay_url, HOST);
    assert_eq!(snap.group_id, GROUP);
    assert!(snap.members.is_empty());
    assert!(snap.roles.is_empty());
}

#[test]
fn members_39002_pubkeys_are_retained() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    let (a, b) = (pk(0), pk(1));
    proj.on_kernel_event(&event(
        "m1",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag(GROUP), p_tag(&a, &[]), p_tag(&b, &[])],
    ));
    let snap = proj.snapshot();
    assert_eq!(snap.members.len(), 2);
    assert!(snap.members.iter().all(|m| m.is_member && !m.is_admin));
    let pubkeys: Vec<&str> = snap.members.iter().map(|m| m.pubkey.as_str()).collect();
    assert!(pubkeys.contains(&a.as_str()) && pubkeys.contains(&b.as_str()));
}

#[test]
fn admins_39001_pubkeys_and_roles_round_trip() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    let admin = pk(2);
    proj.on_kernel_event(&event(
        "a1",
        KIND_GROUP_ADMINS,
        100,
        vec![d_tag(GROUP), p_tag(&admin, &["king", "moderator"])],
    ));
    let snap = proj.snapshot();
    assert_eq!(snap.members.len(), 1);
    let row = &snap.members[0];
    assert_eq!(row.pubkey, admin);
    assert!(row.is_admin && !row.is_member);
    assert_eq!(row.roles, vec!["king".to_string(), "moderator".to_string()]);
}

#[test]
fn member_and_admin_merge_into_one_row_without_duplicate_roles() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    let p = pk(3);
    proj.on_kernel_event(&event(
        "m1",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag(GROUP), p_tag(&p, &["king"])],
    ));
    proj.on_kernel_event(&event(
        "a1",
        KIND_GROUP_ADMINS,
        100,
        vec![d_tag(GROUP), p_tag(&p, &["king"])],
    ));
    let snap = proj.snapshot();
    assert_eq!(snap.members.len(), 1, "same pubkey must collapse to one row");
    let row = &snap.members[0];
    assert!(row.is_admin && row.is_member);
    assert_eq!(row.roles, vec!["king".to_string()], "roles de-duplicated");
}

#[test]
fn roles_catalog_39003_is_exposed() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    proj.on_kernel_event(&event(
        "r1",
        KIND_GROUP_ROLES,
        100,
        vec![
            d_tag(GROUP),
            vec!["role".into(), "king".into(), "the founder".into()],
            vec!["role".into(), "moderator".into()],
        ],
    ));
    let snap = proj.snapshot();
    assert_eq!(snap.roles.len(), 2);
    assert_eq!(snap.roles[0].name, "king");
    assert_eq!(snap.roles[0].description.as_deref(), Some("the founder"));
    assert_eq!(snap.roles[1].name, "moderator");
    assert_eq!(snap.roles[1].description, None);
}

#[test]
fn newer_39002_supersedes_older() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    let (a, b, c) = (pk(0), pk(1), pk(2));
    proj.on_kernel_event(&event(
        "old",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag(GROUP), p_tag(&a, &[]), p_tag(&b, &[])],
    ));
    proj.on_kernel_event(&event(
        "new",
        KIND_GROUP_MEMBERS,
        200,
        vec![d_tag(GROUP), p_tag(&c, &[])],
    ));
    let snap = proj.snapshot();
    assert_eq!(snap.members.len(), 1, "latest 39002 replaces the old set");
    assert_eq!(snap.members[0].pubkey, c);
}

#[test]
fn events_for_other_groups_are_ignored() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    proj.on_kernel_event(&event(
        "other",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag("some-other-group"), p_tag(&pk(0), &[])],
    ));
    assert!(proj.snapshot().members.is_empty());
}

#[test]
fn snapshot_json_shape_is_stable() {
    let proj = GroupRosterProjection::new(HOST, GROUP);
    proj.on_kernel_event(&event(
        "m1",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag(GROUP), p_tag(&pk(0), &[])],
    ));
    let json = proj.snapshot_json();
    assert_eq!(
        json.get("group_id").and_then(|v| v.as_str()),
        Some(GROUP)
    );
    assert_eq!(
        json.get("members")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(1)
    );
}

#[test]
fn empty_helper_matches_fresh_snapshot() {
    let fresh = GroupRosterProjection::new(HOST, GROUP).snapshot();
    assert_eq!(fresh, GroupRosterSnapshot::empty(HOST, GROUP));
}
