use super::*;
use crate::kernel::RelayFrame;
use nmp_network::role::RelayRole;

const RELAY: &str = "wss://relay.example";

// These tests cover the contracts the wasm32 `BrowserRelayDriver` depends
// on. They are intentionally narrow: deeper replay, AUTH partition, and
// wire-sub eviction behaviour is covered by kernel-side tests.

#[test]
fn handle_relay_frame_text_does_not_panic_on_garbage() {
    let mut r = KernelReducer::new();
    let out = r.handle_relay_frame(
        RelayRole::Content,
        RELAY,
        RelayFrame::Text("garbage that is not NIP-01".to_string()),
    );

    assert!(
        out.is_empty(),
        "garbage text must drop, not produce outbound"
    );
}

#[test]
fn handle_relay_frame_close_does_not_panic() {
    let mut r = KernelReducer::new();
    let out = r.handle_relay_frame(
        RelayRole::Content,
        RELAY,
        RelayFrame::Close(Some("server going away".to_string())),
    );

    assert!(out.is_empty());
}

#[test]
fn handle_relay_frame_binary_and_ping_pong_are_counted_no_outbound() {
    let mut r = KernelReducer::new();

    for frame in [
        RelayFrame::Binary(b"opaque".to_vec()),
        RelayFrame::Ping,
        RelayFrame::Pong,
    ] {
        let out = r.handle_relay_frame(RelayRole::Indexer, RELAY, frame);
        assert!(out.is_empty(), "non-text frames must produce no outbound");
    }
}

#[test]
fn handle_relay_connected_first_dial_emits_startup_or_empty() {
    let mut r = KernelReducer::new();
    let out = r.handle_relay_connected(RelayRole::Content, RELAY, false);

    assert!(out.is_empty(), "fresh kernel has no startup REQs");
}

#[test]
fn handle_relay_connected_is_reconnect_does_not_panic() {
    let mut r = KernelReducer::new();

    r.handle_relay_closed(RelayRole::Content, RELAY);
    let _ = r.handle_relay_connected(RelayRole::Content, RELAY, true);
}

#[test]
fn handle_relay_failed_and_closed_are_total() {
    let mut r = KernelReducer::new();

    r.handle_relay_failed(
        RelayRole::Content,
        RELAY,
        "connection reset by peer".to_string(),
    );
    r.handle_relay_closed(RelayRole::Content, RELAY);
}

#[test]
fn handle_relay_outbound_dropped_does_not_mark_transport_failed() {
    let mut r = KernelReducer::new();
    r.set_configured_relays(vec![(RELAY.to_string(), "both".to_string())]);
    r.kernel.relay_connecting_url(RelayRole::Content, RELAY);

    r.handle_relay_outbound_dropped(RelayRole::Content, RELAY);

    let frame = r.make_update_frame(true);
    let envelope = crate::decode_snapshot_envelope(&frame).expect("frame must decode");
    let status = envelope
        .relay_statuses
        .iter()
        .find(|row| row.relay_url == RELAY)
        .expect("content relay status row must be present");
    assert_eq!(
        status.connection, "connecting",
        "pre-connect buffer overflow must not masquerade as a transport failure"
    );
}
