use super::*;

use crate::NmpApp;
use nmp_nip29::{
    close_nip29_group_discovery_session, close_nip29_group_events_session,
    close_nip29_group_roster_session, close_nip29_joined_groups_session,
    open_nip29_group_discovery_session, open_nip29_group_events_session,
    open_nip29_group_roster_session, open_nip29_group_roster_session_with_reader,
    open_nip29_joined_groups_session, GroupId, Nip29GroupDiscoverySession, Nip29GroupEventsSession,
    Nip29GroupRosterSession, Nip29JoinedGroupsSession,
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
fn group_feed_discovery_replacement_makes_old_handle_idempotent() {
    let app = crate::new_app();
    let first = open_nip29_group_discovery_session(
        &app,
        Nip29GroupDiscoverySession::new("wss://groups.example".to_string()),
    );
    assert_eq!(session_count(&app), 1);

    let second = open_nip29_group_discovery_session(
        &app,
        Nip29GroupDiscoverySession::new("wss://other-groups.example".to_string()),
    );
    assert_eq!(
        session_count(&app),
        1,
        "replacement must tear down the old observer/session"
    );

    assert!(!close_nip29_group_discovery_session(&app, first));
    assert_eq!(
        session_count(&app),
        1,
        "stale handles must not close the replacement session"
    );

    assert!(close_nip29_group_discovery_session(&app, second.clone()));
    assert_eq!(session_count(&app), 0);
    assert!(!close_nip29_group_discovery_session(&app, second));
    assert_eq!(session_count(&app), 0);
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
