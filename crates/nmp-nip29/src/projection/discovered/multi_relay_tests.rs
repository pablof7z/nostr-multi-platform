//! Multi-relay aggregation tests for `DiscoveredGroupsProjection` (#93 —
//! chirp#93 cross-relay group browse).
//!
//! Split out from `discovered/tests.rs` (AGENTS.md file-size cap): this is a
//! `mod` of that file (via `#[path]`), so it shares its helpers
//! (`event`/`event_from`/`d_tag`/`p_tag`/`HOST`) and imports through
//! `use super::*;` rather than re-declaring them.

use super::*;

const OTHER_HOST: &str = "wss://other-groups.example.com";
const THIRD_HOST: &str = "wss://third-groups.example.com";

#[test]
fn projection_opened_with_n_relays_surfaces_groups_from_all_of_them() {
    let proj = DiscoveredGroupsProjection::new([HOST, OTHER_HOST, THIRD_HOST]);
    for (relay, group_id) in [
        (HOST, "a-room"),
        (OTHER_HOST, "b-room"),
        (THIRD_HOST, "c-room"),
    ] {
        proj.on_kernel_event(&event_from(
            relay,
            &format!("meta-{group_id}"),
            KIND_GROUP_METADATA,
            100,
            vec![d_tag(group_id), vec!["name".into(), group_id.into()]],
        ));
    }

    let snap = proj.snapshot();
    assert_eq!(snap.groups.len(), 3, "one row per relay's group");
    for (relay, group_id) in [
        (HOST, "a-room"),
        (OTHER_HOST, "b-room"),
        (THIRD_HOST, "c-room"),
    ] {
        let row = snap
            .groups
            .iter()
            .find(|g| g.group_id == group_id)
            .unwrap_or_else(|| panic!("missing row for {group_id}"));
        assert_eq!(
            row.host_relay_url, relay,
            "row must be tagged with its host relay"
        );
    }
}

#[test]
fn two_relays_sharing_a_local_id_are_two_distinct_rows() {
    // Per NIP-29, `(host_relay_url, local_id)` is the group identity — two
    // relays independently hosting a `d=room` group are two different groups,
    // not one that gets clobbered.
    let proj = DiscoveredGroupsProjection::new([HOST, OTHER_HOST]);
    proj.on_kernel_event(&event_from(
        HOST,
        "meta-a",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Room on A".into()]],
    ));
    proj.on_kernel_event(&event_from(
        OTHER_HOST,
        "meta-b",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Room on B".into()]],
    ));

    let snap = proj.snapshot();
    assert_eq!(
        snap.groups.len(),
        2,
        "same local_id, different relay -> two rows"
    );
    let names: Vec<_> = snap.groups.iter().map(|g| g.name.clone()).collect();
    assert!(names.contains(&Some("Room on A".to_string())));
    assert!(names.contains(&Some("Room on B".to_string())));
}

#[test]
fn event_whose_provenance_names_no_tracked_relay_is_dropped() {
    let proj = DiscoveredGroupsProjection::new([HOST]);
    proj.on_kernel_event(&event_from(
        "wss://untracked.example.com",
        "meta",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Room".into()]],
    ));
    assert!(
        proj.snapshot().groups.is_empty(),
        "an event from a relay this projection isn't tracking must not be attributed anywhere"
    );
}

#[test]
fn event_with_empty_provenance_is_dropped_fail_closed() {
    let proj = DiscoveredGroupsProjection::new([HOST]);
    proj.on_kernel_event(&KernelEvent {
        id: "meta".into(),
        author: "author".into(),
        kind: KIND_GROUP_METADATA,
        created_at: 100,
        tags: vec![d_tag("room"), vec!["name".into(), "Room".into()]],
        content: String::new(),
        relay_provenance: Vec::new(),
    });
    assert!(
        proj.snapshot().groups.is_empty(),
        "no provenance means no relay to attribute the event to (D6 fail-closed)"
    );
}

#[test]
fn add_relay_lets_a_previously_untracked_relays_events_be_folded_in() {
    let proj = DiscoveredGroupsProjection::new([HOST]);
    // Arrives before the relay is tracked -> dropped.
    proj.on_kernel_event(&event_from(
        OTHER_HOST,
        "meta-early",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("room"), vec!["name".into(), "Too early".into()]],
    ));
    assert!(proj.snapshot().groups.is_empty());

    proj.add_relay(OTHER_HOST);
    proj.on_kernel_event(&event_from(
        OTHER_HOST,
        "meta-late",
        KIND_GROUP_METADATA,
        200,
        vec![d_tag("room"), vec!["name".into(), "On time".into()]],
    ));

    let snap = proj.snapshot();
    assert_eq!(snap.groups.len(), 1);
    assert_eq!(snap.groups[0].host_relay_url, OTHER_HOST);
    assert_eq!(snap.groups[0].name.as_deref(), Some("On time"));
}

#[test]
fn adding_a_relay_to_a_live_projection_does_not_drop_the_original_relays_groups() {
    // The exact singleton-kill regression (#93): opening discovery for relay A
    // then growing the set to {A, B} must NOT lose A's already-discovered
    // groups.
    let proj = DiscoveredGroupsProjection::new([HOST]);
    proj.on_kernel_event(&event_from(
        HOST,
        "meta-a",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("a-room"), vec!["name".into(), "A Room".into()]],
    ));
    assert_eq!(proj.snapshot().groups.len(), 1, "A's group is discovered");

    // Grow the tracked set to {A, B} — mirrors
    // `open_nip29_group_discovery_session` reconciling a live session.
    proj.add_relay(OTHER_HOST);
    proj.on_kernel_event(&event_from(
        OTHER_HOST,
        "meta-b",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("b-room"), vec!["name".into(), "B Room".into()]],
    ));

    let snap = proj.snapshot();
    assert_eq!(
        snap.groups.len(),
        2,
        "A's group must still be present after B is added: {snap:?}"
    );
    assert!(snap
        .groups
        .iter()
        .any(|g| g.group_id == "a-room" && g.host_relay_url == HOST));
    assert!(snap
        .groups
        .iter()
        .any(|g| g.group_id == "b-room" && g.host_relay_url == OTHER_HOST));
}

#[test]
fn remove_relay_untracks_it_and_purges_its_rows() {
    let proj = DiscoveredGroupsProjection::new([HOST, OTHER_HOST]);
    proj.on_kernel_event(&event_from(
        HOST,
        "meta-a",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("a-room"), vec!["name".into(), "A Room".into()]],
    ));
    proj.on_kernel_event(&event_from(
        OTHER_HOST,
        "meta-b",
        KIND_GROUP_METADATA,
        100,
        vec![d_tag("b-room"), vec!["name".into(), "B Room".into()]],
    ));
    assert_eq!(proj.snapshot().groups.len(), 2);

    proj.remove_relay(OTHER_HOST);

    let snap = proj.snapshot();
    assert_eq!(snap.groups.len(), 1, "B's rows are purged once untracked");
    assert_eq!(snap.groups[0].group_id, "a-room");
    assert_eq!(snap.host_relay_urls, vec![HOST.to_string()]);

    // Even a late-arriving event for the untracked relay is dropped.
    proj.on_kernel_event(&event_from(
        OTHER_HOST,
        "meta-b-late",
        KIND_GROUP_METADATA,
        200,
        vec![d_tag("b-room-2"), vec!["name".into(), "Still B".into()]],
    ));
    assert_eq!(proj.snapshot().groups.len(), 1);
}
