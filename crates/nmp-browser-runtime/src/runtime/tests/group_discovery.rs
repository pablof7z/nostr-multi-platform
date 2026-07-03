use super::started_handle;
use nmp_nip29::{
    close_nip29_group_discovery_session, open_nip29_group_discovery_session,
    Nip29GroupDiscoverySession, DISCOVERED_GROUPS_KEY,
};

const RELAY: &str = "wss://groups.example";

#[test]
fn browser_group_discovery_replaces_same_session_and_close_is_idempotent() {
    let handle = started_handle();
    let first = open_nip29_group_discovery_session(
        &handle,
        Nip29GroupDiscoverySession::new(RELAY.to_string()),
    );
    assert_eq!(first.key(), DISCOVERED_GROUPS_KEY);
    assert_eq!(handle.feed_sessions.live_count(), 1);

    let replacement = open_nip29_group_discovery_session(
        &handle,
        Nip29GroupDiscoverySession::new("wss://other-groups.example".to_string()),
    );
    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "discovery singleton replacement must remove stale read lifecycle"
    );

    assert!(!close_nip29_group_discovery_session(&handle, first));
    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "closing a replaced handle is idempotent and must not tear down the replacement"
    );

    assert!(close_nip29_group_discovery_session(
        &handle,
        replacement.clone()
    ));
    assert_eq!(handle.feed_sessions.live_count(), 0);
    assert!(!close_nip29_group_discovery_session(&handle, replacement));
    assert_eq!(handle.feed_sessions.live_count(), 0);
}
