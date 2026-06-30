use super::*;

use nmp_nip29::GroupId;

#[test]
fn group_feed_events_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = app.open_nip29_group_events_session(Nip29GroupEventsSession::new(
        GroupId::new("wss://groups.example", "first"),
        vec![9],
    ));
    assert_eq!(session_count(&app), 1);

    let second = app.open_nip29_group_events_session(Nip29GroupEventsSession::new(
        GroupId::new("wss://groups.example", "second"),
        vec![11],
    ));
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    app.close_nip29_group_events_session(first);
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    app.close_nip29_group_events_session(second.clone());
    assert_eq!(session_count(&app), 0);
    app.close_nip29_group_events_session(second);
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_feed_discovery_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = app.open_nip29_group_discovery_session(Nip29GroupDiscoverySession::new(
        "wss://groups.example".to_string(),
    ));
    assert_eq!(session_count(&app), 1);

    let second = app.open_nip29_group_discovery_session(Nip29GroupDiscoverySession::new(
        "wss://other-groups.example".to_string(),
    ));
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    app.close_nip29_group_discovery_session(first);
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    app.close_nip29_group_discovery_session(second.clone());
    assert_eq!(session_count(&app), 0);
    app.close_nip29_group_discovery_session(second);
    assert_eq!(session_count(&app), 0);
}

#[test]
fn joined_groups_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = app
        .open_nip29_joined_groups_session(Nip29JoinedGroupsSession::new(
            "a".repeat(64),
            "wss://groups.example".to_string(),
        ))
        .expect("non-empty active pubkey opens a joined-groups session");
    assert_eq!(session_count(&app), 1);

    let second = app
        .open_nip29_joined_groups_session(Nip29JoinedGroupsSession::new(
            "b".repeat(64),
            "wss://other-groups.example".to_string(),
        ))
        .expect("non-empty active pubkey opens a replacement joined-groups session");
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    app.close_nip29_joined_groups_session(first);
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    app.close_nip29_joined_groups_session(second.clone());
    assert_eq!(session_count(&app), 0);
    app.close_nip29_joined_groups_session(second);
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_feed_roster_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = app.open_nip29_group_roster_session(Nip29GroupRosterSession::new(GroupId::new(
        "wss://groups.example",
        "first",
    )));
    assert_eq!(session_count(&app), 1);

    let second = app.open_nip29_group_roster_session(Nip29GroupRosterSession::new(GroupId::new(
        "wss://groups.example",
        "second",
    )));
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    app.close_nip29_group_roster_session(first);
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    app.close_nip29_group_roster_session(second.clone());
    assert_eq!(session_count(&app), 0);
    app.close_nip29_group_roster_session(second);
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_feed_roster_reader_reflects_ingested_membership() {
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;

    let app = crate::new_app();
    let (_handle, reader) = app.open_nip29_group_roster_session_with_reader(
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
fn group_reactions_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = app.open_nip25_group_reactions_session(Nip25GroupReactionsSession::new(
        GroupId::new("wss://groups.example", "first"),
    ));
    assert_eq!(session_count(&app), 1);

    let second = app.open_nip25_group_reactions_session(Nip25GroupReactionsSession::new(
        GroupId::new("wss://groups.example", "second"),
    ));
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    app.close_nip25_group_reactions_session(first);
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    app.close_nip25_group_reactions_session(second.clone());
    assert_eq!(session_count(&app), 0);
    app.close_nip25_group_reactions_session(second);
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_reactions_reader_aggregates_ingested_kind7() {
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;

    let app = crate::new_app();
    let (_handle, reader) = app.open_nip25_group_reactions_session_with_reader(
        Nip25GroupReactionsSession::new(GroupId::new("wss://groups.example", "room")),
    );
    // Inject two in-group reactions on the same target, from distinct reactors,
    // directly into the reader — the same Arc registered as the observed
    // projection — and confirm the aggregate counts them (the seam 29er needs
    // to render reaction chips + counts).
    let target = "a".repeat(64);
    for (id, author, emoji) in [
        ("r1", "1".repeat(64), "+"),
        ("r2", "2".repeat(64), "🔥"),
    ] {
        reader.on_kernel_event(&KernelEvent {
            id: id.into(),
            author,
            kind: 7,
            created_at: 100,
            tags: vec![
                vec!["e".into(), target.clone()],
                vec!["h".into(), "room".into()],
            ],
            content: emoji.into(),
            relay_provenance: Vec::new(),
        });
    }
    let agg = reader.aggregate_for(&target).expect("target aggregated");
    assert_eq!(agg.total, 2);
    assert_eq!(agg.reactors.len(), 2);
    assert_eq!(agg.by_emoji.len(), 2);
}

#[test]
fn joined_groups_empty_active_pubkey_is_noop() {
    let app = crate::new_app();
    let handle = app.open_nip29_joined_groups_session(Nip29JoinedGroupsSession::new(
        String::new(),
        "wss://groups.example".to_string(),
    ));

    assert!(handle.is_none());
    assert_eq!(session_count(&app), 0);
}

fn session_count(app: &NmpApp) -> usize {
    app.group_feed_sessions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .len()
}
