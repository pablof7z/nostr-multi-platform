use nmp_core::substrate::KernelEvent;
use nmp_nip29::{
    close_nip29_group_events_session, decode_group_events_snapshot,
    open_nip29_group_events_session, GroupId, Nip29GroupEventsSession, GROUP_EVENTS_KEY,
};

use super::started_handle;

const RELAY: &str = "wss://groups.example";
const GROUP_ID: &str = "nmp-builders";

#[test]
fn browser_group_events_emits_ngev_rows_from_h_tagged_relay_hits() {
    let mut handle = started_handle();
    // Chat view: the consumer declares kinds [9, 11] (issue #2187).
    let session = open_nip29_group_events_session(
        &handle,
        Nip29GroupEventsSession::new(GroupId::new(RELAY, GROUP_ID), vec![9, 11]),
    );
    assert_eq!(session.key(), GROUP_EVENTS_KEY);

    let opened = handle.pump();
    let outbound = opened
        .outbound
        .iter()
        .map(|frame| frame.text().to_string())
        .collect::<Vec<_>>();
    assert!(
        outbound.iter().any(|text| {
            text.contains(r#""kinds":[9,11]"#) && text.contains(r##""#h":["nmp-builders"]"##)
        }),
        "group timeline must fan out a relay-pinned NIP-29 chat REQ, outbound={outbound:?}"
    );

    handle
        .runtime
        .reducer
        .fire_event_observers_for_test(&KernelEvent {
            id: "11".repeat(32),
            author: "22".repeat(32),
            // kind:9 is the NIP-29-routed chat kind, owned by the caller (not
            // nmp-nip29, which is kind-blind about non-9xxx/3900x kinds).
            kind: 9,
            created_at: 42,
            tags: vec![vec!["h".to_string(), GROUP_ID.to_string()]],
            content: "hello from a browser group timeline".to_string(),
            relay_provenance: vec![RELAY.to_string()],
        });

    let payload = timeline_payload(&mut handle, "nmp.nip29.group_events");
    let snapshot = decode_group_events_snapshot(&payload).expect("NGEV decodes");
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(
        snapshot.events[0].content,
        "hello from a browser group timeline"
    );
    assert_eq!(snapshot.events[0].kind, 9);
}

#[test]
fn browser_group_events_replaces_prior_session_and_close_is_idempotent() {
    let handle = started_handle();
    let first = open_nip29_group_events_session(
        &handle,
        Nip29GroupEventsSession::new(GroupId::new(RELAY, "first-room"), vec![9]),
    );
    assert_eq!(handle.feed_sessions.live_count(), 1);

    let second = open_nip29_group_events_session(
        &handle,
        Nip29GroupEventsSession::new(GroupId::new(RELAY, "second-room"), vec![11]),
    );
    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "singleton replacement must remove the stale read lifecycle"
    );

    assert!(!close_nip29_group_events_session(&handle, first));
    assert_eq!(
        handle.feed_sessions.live_count(),
        1,
        "closing a replaced handle is idempotent and must not tear down the replacement"
    );

    assert!(close_nip29_group_events_session(&handle, second.clone()));
    assert_eq!(handle.feed_sessions.live_count(), 0);
    assert!(!close_nip29_group_events_session(&handle, second));
    assert_eq!(handle.feed_sessions.live_count(), 0);
}

fn timeline_payload(handle: &mut crate::BrowserRuntimeHandle, key: &str) -> Vec<u8> {
    let bytes = handle
        .produce_snapshot_bytes(true)
        .expect("snapshot frame bytes");
    nmp_core::decode_snapshot_typed_projections(&bytes)
        .expect("typed projections decode")
        .into_iter()
        .find(|row| row.key == key)
        .expect("group timeline projection row present")
        .payload
}
