//! Unit tests for `DiscoveredGroupsProjection`.
//!
//! Lives in a sibling file (not `#[cfg(test)] mod tests` inline) so the
//! production `discovered.rs` stays under the AGENTS.md 500-LoC ceiling.
//! The test surface mirrors `GroupChatProjection`'s test idiom: direct
//! `on_kernel_event` injection — no mock relay infrastructure needed.

use std::sync::Arc;

use nmp_core::substrate::KernelEvent;
use nmp_core::KernelEventObserver;

use super::{DiscoveredGroupsProjection, DiscoveredGroupsSnapshot};
use crate::kinds::{KIND_GROUP_ADMINS, KIND_GROUP_MEMBERS, KIND_GROUP_METADATA};

const HOST: &str = "wss://groups.example.com";

fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: format!("relay-of-{id}"),
        kind,
        created_at,
        tags,
        content: String::new(),
    }
}

fn d_tag(local_id: &str) -> Vec<String> {
    vec!["d".into(), local_id.into()]
}

fn p_tag(pubkey: &str) -> Vec<String> {
    vec!["p".into(), pubkey.into()]
}

#[test]
fn fresh_projection_yields_empty_snapshot() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    let snap = proj.snapshot();
    assert_eq!(snap.host_relay_url, HOST);
    assert!(snap.groups.is_empty());

    let json = proj.snapshot_json();
    assert_eq!(
        json.get("host_relay_url").and_then(|v| v.as_str()),
        Some(HOST)
    );
    assert_eq!(
        json.get("groups")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(0)
    );
}

#[test]
fn kind39000_populates_name_picture_about() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "meta1",
        KIND_GROUP_METADATA,
        100,
        vec![
            d_tag("rust-nostr"),
            vec!["name".into(), "Rust Nostr".into()],
            vec!["picture".into(), "https://x.test/p.png".into()],
            vec!["about".into(), "We build NMP.".into()],
        ],
    ));

    let snap = proj.snapshot();
    assert_eq!(snap.groups.len(), 1);
    let g = &snap.groups[0];
    assert_eq!(g.group_id, "rust-nostr");
    assert_eq!(g.host_relay_url, HOST);
    assert_eq!(g.name.as_deref(), Some("Rust Nostr"));
    assert_eq!(g.picture.as_deref(), Some("https://x.test/p.png"));
    assert_eq!(g.about.as_deref(), Some("We build NMP."));
    // Defaults: no `private`/`closed` markers → public + open.
    assert!(g.public);
    assert!(g.open);
    // No 39001/39002 yet.
    assert_eq!(g.member_count, 0);
    assert_eq!(g.admin_count, 0);
}

#[test]
fn private_marker_flips_public_flag() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "meta",
        KIND_GROUP_METADATA,
        100,
        vec![
            d_tag("secret"),
            vec!["name".into(), "Hidden".into()],
            vec!["private".into()],
            vec!["closed".into()],
        ],
    ));

    let g = &proj.snapshot().groups[0];
    assert!(!g.public, "private marker must flip public to false");
    assert!(!g.open, "closed marker must flip open to false");
}

#[test]
fn kind39002_member_count_is_p_tag_cardinality() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "members",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag("room"), p_tag("a"), p_tag("b"), p_tag("c")],
    ));

    let g = &proj.snapshot().groups[0];
    assert_eq!(g.member_count, 3);
}

#[test]
fn kind39001_admin_count_is_p_tag_cardinality() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "admins",
        KIND_GROUP_ADMINS,
        100,
        vec![d_tag("room"), p_tag("a"), p_tag("b")],
    ));

    let g = &proj.snapshot().groups[0];
    assert_eq!(g.admin_count, 2);
}

#[test]
fn three_kinds_for_same_d_roll_into_one_row() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "meta",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Room".into()]],
    ));
    proj.on_kernel_event(&event(
        "admins",
        KIND_GROUP_ADMINS,
        100,
        vec![d_tag("room"), p_tag("admin1")],
    ));
    proj.on_kernel_event(&event(
        "members",
        KIND_GROUP_MEMBERS,
        100,
        vec![d_tag("room"), p_tag("a"), p_tag("b"), p_tag("c")],
    ));

    let snap = proj.snapshot();
    assert_eq!(snap.groups.len(), 1, "all 3 kinds for one d → one row");
    let g = &snap.groups[0];
    assert_eq!(g.name.as_deref(), Some("Room"));
    assert_eq!(g.admin_count, 1);
    assert_eq!(g.member_count, 3);
}

#[test]
fn replaceable_semantics_newer_metadata_wins() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "older",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Old name".into()]],
    ));
    proj.on_kernel_event(&event(
        "newer",
        KIND_GROUP_METADATA,
        200,
        vec![d_tag("room"), vec!["name".into(), "New name".into()]],
    ));

    let g = &proj.snapshot().groups[0];
    assert_eq!(g.name.as_deref(), Some("New name"));
}

#[test]
fn replaceable_semantics_older_event_does_not_overwrite() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "newer",
        KIND_GROUP_METADATA,
        200,
        vec![d_tag("room"), vec!["name".into(), "New name".into()]],
    ));
    // Older event arrives second (out-of-order delivery).
    proj.on_kernel_event(&event(
        "older",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Old name".into()]],
    ));

    let g = &proj.snapshot().groups[0];
    assert_eq!(
        g.name.as_deref(),
        Some("New name"),
        "older event must not overwrite a newer replaceable"
    );
}

#[test]
fn off_kind_with_d_tag_is_excluded() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    // Wrong kind (a long-form 30023), even with a `d` tag, isn't metadata.
    proj.on_kernel_event(&event(
        "long",
        30023,
        100,
        vec![d_tag("room"), vec!["name".into(), "Not group meta".into()]],
    ));
    assert!(proj.snapshot().groups.is_empty());
}

#[test]
fn metadata_event_without_d_tag_is_excluded() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    // 39000 without a `d` tag is malformed — drop it.
    proj.on_kernel_event(&event(
        "noisy",
        KIND_GROUP_METADATA,
        100,
        vec![vec!["name".into(), "Orphan".into()]],
    ));
    assert!(proj.snapshot().groups.is_empty());
}

#[test]
fn groups_are_ordered_alphabetically_by_id() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    for d in ["charlie", "alpha", "bravo"] {
        proj.on_kernel_event(&event(
            d,
            KIND_GROUP_METADATA,
            100,
            vec![d_tag(d), vec!["name".into(), d.into()]],
        ));
    }
    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.groups.iter().map(|g| g.group_id.as_str()).collect();
    assert_eq!(ids, vec!["alpha", "bravo", "charlie"]);
}

#[test]
fn snapshot_json_round_trips_through_serde() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    proj.on_kernel_event(&event(
        "meta",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Room".into()]],
    ));
    let snap = proj.snapshot();
    let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
    let decoded: DiscoveredGroupsSnapshot =
        serde_json::from_str(&encoded).expect("snapshot deserialises");
    assert_eq!(snap, decoded);
}

#[test]
fn drives_through_observer_trait_object() {
    // Same trait-object usage the host registers with
    // `register_event_observer`.
    let proj = Arc::new(DiscoveredGroupsProjection::new(HOST));
    let observer: Arc<dyn KernelEventObserver> = Arc::clone(&proj) as _;
    observer.on_kernel_event(&event(
        "meta",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Room".into()]],
    ));
    assert_eq!(proj.snapshot().groups.len(), 1);
}

#[test]
fn host_relay_url_accessor_returns_construction_value() {
    let proj = DiscoveredGroupsProjection::new(HOST);
    assert_eq!(proj.host_relay_url(), HOST);
}

#[test]
fn outer_map_is_bounded_against_adversarial_d_tag_spam() {
    // V7 — without an outer bound, a relay spamming distinct `d` tags grows
    // the projection's internal map for the lifetime of the session. The
    // `BoundedMessageMap` cap (MAX_PROJECTION_MESSAGES = 10_000) defends the
    // resident set and the per-tick snapshot cost: at saturation we keep
    // only the most-recent rows. Pushing well beyond the cap and asserting
    // the snapshot len does not exceed it is the structural invariant.
    use nmp_core::substrate::MAX_PROJECTION_MESSAGES;
    let proj = DiscoveredGroupsProjection::new(HOST);
    // Push 2× the cap distinct `d` values via kind:39000 — each creates a
    // new `(kind, d)` slot in the outer map.
    let overflow = MAX_PROJECTION_MESSAGES * 2;
    for i in 0..overflow {
        let d = format!("group-{i:07}");
        proj.on_kernel_event(&event(
            &format!("meta-{i}"),
            KIND_GROUP_METADATA,
            100 + i as u64,
            vec![d_tag(&d), vec!["name".into(), d]],
        ));
    }
    let snap = proj.snapshot();
    assert!(
        snap.groups.len() <= MAX_PROJECTION_MESSAGES,
        "discovered-groups outer map must be bounded; got {} (cap {})",
        snap.groups.len(),
        MAX_PROJECTION_MESSAGES,
    );
}
