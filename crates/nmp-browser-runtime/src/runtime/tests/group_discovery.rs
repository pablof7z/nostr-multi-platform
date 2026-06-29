use super::started_handle;
use crate::runtime::BrowserGroupDiscoverySessionDescriptor;

const RELAY: &str = "wss://groups.example";

#[test]
fn browser_group_discovery_replaces_same_session_and_close_is_idempotent() {
    let mut handle = started_handle();
    let first = handle
        .open_nip29_group_discovery_session(BrowserGroupDiscoverySessionDescriptor {
            relay_url: RELAY.to_string(),
            session_id: "catalog".to_string(),
        })
        .expect("first group-discovery session");
    assert_eq!(first.projection_key(), "nmp.nip29.discovered_groups");
    assert_eq!(handle.group_discovery_sessions.len(), 1);

    let replacement = handle
        .open_nip29_group_discovery_session(BrowserGroupDiscoverySessionDescriptor {
            relay_url: "wss://other-groups.example".to_string(),
            session_id: "catalog".to_string(),
        })
        .expect("replacement group-discovery session");
    assert_eq!(
        handle.group_discovery_sessions.len(),
        1,
        "same-session replacement must remove stale observer bookkeeping"
    );

    handle.close_nip29_group_discovery_session(first);
    assert_eq!(
        handle.group_discovery_sessions.len(),
        1,
        "closing a replaced handle is idempotent and must not tear down the replacement"
    );

    handle.close_nip29_group_discovery_session(replacement.clone());
    assert!(handle.group_discovery_sessions.is_empty());
    handle.close_nip29_group_discovery_session(replacement);
    assert!(handle.group_discovery_sessions.is_empty());
}
