use crate::NmpApp;
use nmp_nip29::{
    close_nip29_group_discovery_session, close_nip29_group_events_session,
    close_nip29_group_roster_session, close_nip29_joined_groups_session,
    open_nip29_group_discovery_session, open_nip29_group_discovery_session_with_reader,
    open_nip29_group_events_session, open_nip29_group_roster_session,
    open_nip29_group_roster_session_with_reader, open_nip29_joined_groups_session, GroupId,
    Nip29GroupDiscoverySession, Nip29GroupEventsSession, Nip29GroupRosterSession,
    Nip29JoinedGroupsSession,
};

#[test]
fn group_feed_events_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = open_nip29_group_events_session(
        &app,
        Nip29GroupEventsSession::new(GroupId::new("wss://groups.example", "first"), vec![9]),
    );
    assert_eq!(session_count(&app), 1);

    let second = open_nip29_group_events_session(
        &app,
        Nip29GroupEventsSession::new(GroupId::new("wss://groups.example", "second"), vec![11]),
    );
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    assert!(!close_nip29_group_events_session(&app, first));
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    assert!(close_nip29_group_events_session(&app, second.clone()));
    assert_eq!(session_count(&app), 0);
    assert!(!close_nip29_group_events_session(&app, second));
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_feed_discovery_reconciles_relay_set_without_a_second_session() {
    // Re-opening discovery with an entirely different relay set still
    // reconciles the ONE singleton session (#93) rather than minting a
    // second one — `session_count` never exceeds 1 either way.
    let app = crate::new_app();
    let first = open_nip29_group_discovery_session(
        &app,
        Nip29GroupDiscoverySession::new(vec!["wss://groups.example".to_string()]),
    );
    assert_eq!(session_count(&app), 1);

    let second = open_nip29_group_discovery_session(
        &app,
        Nip29GroupDiscoverySession::new(vec!["wss://other-groups.example".to_string()]),
    );
    assert_eq!(
        session_count(&app),
        1,
        "reconciling to a disjoint relay set stays ONE session"
    );

    assert!(close_nip29_group_discovery_session(&app, second));
    assert_eq!(session_count(&app), 0);
    assert!(!close_nip29_group_discovery_session(&app, first));
    assert_eq!(session_count(&app), 0);
}

#[test]
fn discovery_session_aggregates_groups_from_all_relays_in_one_projection() {
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;

    let app = crate::new_app();
    const RELAY_A: &str = "wss://a.groups.example";
    const RELAY_B: &str = "wss://b.groups.example";
    const RELAY_C: &str = "wss://c.groups.example";

    let (handle, reader) = open_nip29_group_discovery_session_with_reader(
        &app,
        Nip29GroupDiscoverySession::new(vec![
            RELAY_A.to_string(),
            RELAY_B.to_string(),
            RELAY_C.to_string(),
        ]),
    );

    for (relay, group_id) in [
        (RELAY_A, "room-a"),
        (RELAY_B, "room-b"),
        (RELAY_C, "room-c"),
    ] {
        reader.on_kernel_event(&KernelEvent {
            id: format!("meta-{group_id}"),
            author: "relay".into(),
            kind: 39_000,
            created_at: 100,
            tags: vec![vec!["d".into(), group_id.into()]],
            content: String::new(),
            relay_provenance: vec![relay.to_string()],
        });
    }

    let snap = reader.snapshot();
    assert_eq!(
        snap.groups.len(),
        3,
        "one discovery session, all 3 relays' groups"
    );
    for (relay, group_id) in [
        (RELAY_A, "room-a"),
        (RELAY_B, "room-b"),
        (RELAY_C, "room-c"),
    ] {
        assert!(
            snap.groups
                .iter()
                .any(|g| g.group_id == group_id && g.host_relay_url == relay),
            "missing {group_id}@{relay} in aggregated snapshot: {:?}",
            snap.groups
        );
    }

    assert!(close_nip29_group_discovery_session(&app, handle));
    assert_eq!(session_count(&app), 0);
}

#[test]
fn adding_a_relay_to_a_live_discovery_session_does_not_drop_the_original_relays_groups() {
    // The exact singleton-kill regression (#93): open discovery for relay A,
    // then re-open (growing the desired set to {A, B}) — A's already-discovered
    // groups must still be present, and the read-lifecycle stays ONE session.
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;

    let app = crate::new_app();
    const RELAY_A: &str = "wss://a.groups.example";
    const RELAY_B: &str = "wss://b.groups.example";

    let (first, reader) = open_nip29_group_discovery_session_with_reader(
        &app,
        Nip29GroupDiscoverySession::new(vec![RELAY_A.to_string()]),
    );
    reader.on_kernel_event(&KernelEvent {
        id: "meta-a".into(),
        author: "relay".into(),
        kind: 39_000,
        created_at: 100,
        tags: vec![vec!["d".into(), "room-a".into()]],
        content: String::new(),
        relay_provenance: vec![RELAY_A.to_string()],
    });
    assert_eq!(reader.snapshot().groups.len(), 1, "A's group is discovered");
    assert_eq!(session_count(&app), 1);

    let (second, reader) = open_nip29_group_discovery_session_with_reader(
        &app,
        Nip29GroupDiscoverySession::new(vec![RELAY_A.to_string(), RELAY_B.to_string()]),
    );
    assert_eq!(
        session_count(&app),
        1,
        "growing the relay set reconciles the ONE session, no second one opens"
    );

    reader.on_kernel_event(&KernelEvent {
        id: "meta-b".into(),
        author: "relay".into(),
        kind: 39_000,
        created_at: 100,
        tags: vec![vec!["d".into(), "room-b".into()]],
        content: String::new(),
        relay_provenance: vec![RELAY_B.to_string()],
    });

    let snap = reader.snapshot();
    assert_eq!(
        snap.groups.len(),
        2,
        "A's group must still be present after B is added: {:?}",
        snap.groups
    );
    assert!(snap
        .groups
        .iter()
        .any(|g| g.group_id == "room-a" && g.host_relay_url == RELAY_A));
    assert!(snap
        .groups
        .iter()
        .any(|g| g.group_id == "room-b" && g.host_relay_url == RELAY_B));

    // Both handles address the same underlying session — closing either one
    // via `second` (the current handle) tears it down; a stale-looking
    // `first` obtained before the reconcile addresses the very same session.
    assert!(close_nip29_group_discovery_session(&app, second));
    assert_eq!(session_count(&app), 0);
    assert!(!close_nip29_group_discovery_session(&app, first));
}

#[test]
fn joined_groups_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = open_nip29_joined_groups_session(
        &app,
        Nip29JoinedGroupsSession::new("a".repeat(64), "wss://groups.example".to_string()),
    )
    .expect("non-empty active pubkey opens a joined-groups session");
    assert_eq!(session_count(&app), 1);

    let second = open_nip29_joined_groups_session(
        &app,
        Nip29JoinedGroupsSession::new("b".repeat(64), "wss://other-groups.example".to_string()),
    )
    .expect("non-empty active pubkey opens a replacement joined-groups session");
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    assert!(!close_nip29_joined_groups_session(&app, first));
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    assert!(close_nip29_joined_groups_session(&app, second.clone()));
    assert_eq!(session_count(&app), 0);
    assert!(!close_nip29_joined_groups_session(&app, second));
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_feed_roster_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = open_nip29_group_roster_session(
        &app,
        Nip29GroupRosterSession::new(GroupId::new("wss://groups.example", "first")),
    );
    assert_eq!(session_count(&app), 1);

    let second = open_nip29_group_roster_session(
        &app,
        Nip29GroupRosterSession::new(GroupId::new("wss://groups.example", "second")),
    );
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    assert!(!close_nip29_group_roster_session(&app, first));
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    assert!(close_nip29_group_roster_session(&app, second.clone()));
    assert_eq!(session_count(&app), 0);
    assert!(!close_nip29_group_roster_session(&app, second));
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_feed_roster_reader_reflects_ingested_membership() {
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;

    let app = crate::new_app();
    let (_handle, reader) = open_nip29_group_roster_session_with_reader(
        &app,
        Nip29GroupRosterSession::new(GroupId::new("wss://groups.example", "room")),
    );
    // Inject a relay-signed 39002 members snapshot directly into the reader —
    // the same Arc registered as the observed projection — and confirm the
    // roster retains the pubkeys (the seam 29er needs to render a roster).
    reader.on_kernel_event(&KernelEvent {
        id: "m1".into(),
        author: "relay".into(),
        kind: 39002,
        created_at: 100,
        tags: vec![
            vec!["d".into(), "room".into()],
            vec!["p".into(), "a".repeat(64)],
            vec!["p".into(), "b".repeat(64)],
        ],
        content: String::new(),
        relay_provenance: Vec::new(),
    });
    let snap = reader.snapshot();
    assert_eq!(snap.members.len(), 2);
    assert!(snap.members.iter().all(|m| m.is_member));
}

#[test]
fn joined_groups_empty_active_pubkey_is_noop() {
    let app = crate::new_app();
    let handle = open_nip29_joined_groups_session(
        &app,
        Nip29JoinedGroupsSession::new(String::new(), "wss://groups.example".to_string()),
    );

    assert!(handle.is_none());
    assert_eq!(session_count(&app), 0);
}

fn session_count(app: &NmpApp) -> usize {
    app.live_feed_session_count()
}
