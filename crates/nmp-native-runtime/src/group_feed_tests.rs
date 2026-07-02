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
        String::new(),
    ));
    assert_eq!(session_count(&app), 1);

    let second = app.open_nip25_group_reactions_session(Nip25GroupReactionsSession::new(
        GroupId::new("wss://groups.example", "second"),
        String::new(),
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
    // The viewer is reactor "1.." — the aggregate must surface their own kind:7
    // id in `mine` so the app can retract it.
    let viewer = "1".repeat(64);
    let (_handle, reader) =
        app.open_nip25_group_reactions_session_with_reader(Nip25GroupReactionsSession::new(
            GroupId::new("wss://groups.example", "room"),
            viewer.clone(),
        ));
    // Inject two in-group reactions on the same target, from distinct reactors,
    // directly into the reader — the same Arc registered as the observed
    // projection — and confirm the aggregate counts them (the seam 29er needs
    // to render reaction chips + counts).
    let target = "a".repeat(64);
    for (id, author, emoji) in [("r1", viewer.clone(), "+"), ("r2", "2".repeat(64), "🔥")] {
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
    // The viewer's own kind:7 ("r1") is surfaced as the retraction handle.
    assert_eq!(agg.mine.len(), 1, "only the viewer's own reaction");
    assert_eq!(agg.mine[0].reaction_event_id, "r1");
    assert_eq!(agg.mine[0].token, "+");
}

#[test]
fn group_threading_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = app.open_nip29_group_threading_session(Nip29GroupThreadingSession::new(
        GroupId::new("wss://groups.example", "first"),
        vec![9, 11],
    ));
    assert_eq!(session_count(&app), 1);

    let second = app.open_nip29_group_threading_session(Nip29GroupThreadingSession::new(
        GroupId::new("wss://groups.example", "second"),
        vec![9, 11],
    ));
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    app.close_nip29_group_threading_session(first);
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    app.close_nip29_group_threading_session(second.clone());
    assert_eq!(session_count(&app), 0);
    app.close_nip29_group_threading_session(second);
    assert_eq!(session_count(&app), 0);
}

#[test]
fn group_threading_reader_resolves_reply_and_root_edges_for_a_published_tree() {
    use nmp_core::substrate::KernelEvent;
    use nmp_core::ObservedProjectionSink;

    let app = crate::new_app();
    let (_handle, reader) = app.open_nip29_group_threading_session_with_reader(
        Nip29GroupThreadingSession::new(GroupId::new("wss://groups.example", "room"), vec![9]),
    );

    // A small reply tree published into the group: a root chat message, a
    // direct reply, and a reply-to-the-reply. 29er/hl need this exact shape —
    // reply chips on `msg2`/`msg3` and a thread jump back to `msg1` — with
    // zero app-side `e`-tag parsing (issue #2719 acceptance criteria).
    let root_id = "1".repeat(64);
    let reply_id = "2".repeat(64);
    let grandchild_id = "3".repeat(64);
    let author = "a".repeat(64);

    reader.on_kernel_event(&KernelEvent {
        id: root_id.clone(),
        author: author.clone(),
        kind: 9,
        created_at: 100,
        tags: vec![vec!["h".into(), "room".into()]],
        content: "root message".into(),
        relay_provenance: Vec::new(),
    });
    reader.on_kernel_event(&KernelEvent {
        id: reply_id.clone(),
        author: author.clone(),
        kind: 9,
        created_at: 200,
        tags: vec![
            vec!["h".into(), "room".into()],
            vec!["e".into(), root_id.clone(), String::new(), "root".into()],
            vec!["e".into(), root_id.clone(), String::new(), "reply".into()],
        ],
        content: "a reply".into(),
        relay_provenance: Vec::new(),
    });
    reader.on_kernel_event(&KernelEvent {
        id: grandchild_id.clone(),
        author: author.clone(),
        kind: 9,
        created_at: 300,
        tags: vec![
            vec!["h".into(), "room".into()],
            vec!["e".into(), root_id.clone(), String::new(), "root".into()],
            vec!["e".into(), reply_id.clone(), String::new(), "reply".into()],
        ],
        content: "a reply to the reply".into(),
        relay_provenance: Vec::new(),
    });

    let snapshot = reader.snapshot();
    assert_eq!(
        snapshot.edges.len(),
        3,
        "one edge row per event, kind-blind"
    );

    let root_edge = snapshot
        .edges
        .iter()
        .find(|e| e.event_id == root_id)
        .expect("root edge present");
    assert!(root_edge.parent.is_none());
    assert!(root_edge.root.is_none());

    let reply_edge = snapshot
        .edges
        .iter()
        .find(|e| e.event_id == reply_id)
        .expect("reply edge present");
    assert_eq!(
        reply_edge.parent.as_ref().and_then(|p| p.event_id()),
        Some(root_id.as_str())
    );
    assert_eq!(
        reply_edge.root.as_ref().and_then(|p| p.event_id()),
        Some(root_id.as_str())
    );

    let grandchild_edge = snapshot
        .edges
        .iter()
        .find(|e| e.event_id == grandchild_id)
        .expect("grandchild edge present");
    assert_eq!(
        grandchild_edge.parent.as_ref().and_then(|p| p.event_id()),
        Some(reply_id.as_str())
    );
    assert_eq!(
        grandchild_edge.root.as_ref().and_then(|p| p.event_id()),
        Some(root_id.as_str())
    );

    // The grouper stitches all three into one Twitter-style module block —
    // the thread-jump / module-render seam 29er needs.
    assert!(
        snapshot.blocks.iter().any(|b| matches!(
            b,
            nmp_threading::TimelineBlock::Module { events, .. } if events.len() == 3
        )),
        "expected a 3-event module block, got {:?}",
        snapshot.blocks
    );
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
