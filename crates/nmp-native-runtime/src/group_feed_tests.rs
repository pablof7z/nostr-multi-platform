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

fn session_count(app: &NmpApp) -> usize {
    app.group_feed_sessions
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .len()
}
