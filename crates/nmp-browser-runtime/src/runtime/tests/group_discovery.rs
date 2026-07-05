use super::started_handle;
use nmp_nip29::{
    close_nip29_group_discovery_session, open_nip29_group_discovery_session,
    Nip29GroupDiscoverySession, DISCOVERED_GROUPS_KEY,
};

const RELAY: &str = "wss://groups.example";

#[test]
fn browser_group_discovery_reconciles_relay_set_into_one_session() {
    // #93 — re-opening discovery with a different desired relay set
    // reconciles the ONE singleton read-lifecycle session rather than
    // minting a second one; `live_count` never exceeds 1 either way.
    let handle = started_handle();
    let first = open_nip29_group_discovery_session(
        &handle,
        Nip29GroupDiscoverySession::new(vec![RELAY.to_string()]),
    );
    assert_eq!(first.key(), DISCOVERED_GROUPS_KEY);
    assert_eq!(handle.feed_sessions.live_count(), 1);

    let second = open_nip29_group_discovery_session(
        &handle,
        Nip29GroupDiscoverySession::new(vec!["wss://other-groups.example".to_string()]),
    );
    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "reconciling to a disjoint relay set stays ONE session"
    );

    assert!(close_nip29_group_discovery_session(&handle, second));
    assert_eq!(handle.feed_sessions.live_count(), 0);
    assert!(!close_nip29_group_discovery_session(&handle, first));
    assert_eq!(handle.feed_sessions.live_count(), 0);
}
