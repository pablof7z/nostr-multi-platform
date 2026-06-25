use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn parse_event_frame_extracts_inner_event_json() {
    let frame = r#"["EVENT","nmp-bunker",{"id":"abc","kind":24133,"content":"x"}]"#;
    let v = parse_event_frame(frame).expect("event frame parses");
    assert_eq!(v.get("id").and_then(|x| x.as_str()), Some("abc"));
}

#[test]
fn parse_event_frame_rejects_non_event_frames() {
    assert!(parse_event_frame(r#"["EOSE","subA"]"#).is_none());
    assert!(parse_event_frame(r#"["NOTICE","go away"]"#).is_none());
    assert!(parse_event_frame(r#"not json"#).is_none());
    assert!(parse_event_frame(r#"["EVENT"]"#).is_none());
}

#[test]
fn parse_event_frame_rejects_other_subscriptions_and_kinds() {
    assert!(
        parse_event_frame(r#"["EVENT","other-sub",{"id":"abc","kind":24133,"content":"x"}]"#)
            .is_none()
    );
    assert!(
        parse_event_frame(r#"["EVENT","nmp-bunker",{"id":"abc","kind":1,"content":"x"}]"#)
            .is_none()
    );
}

#[test]
fn parse_event_frame_rejects_non_array_json() {
    // A bare object or scalar must not panic — D6.
    assert!(parse_event_frame(r#"{"id":"abc"}"#).is_none());
    assert!(parse_event_frame(r#"42"#).is_none());
    assert!(parse_event_frame(r#""just-a-string""#).is_none());
}

#[test]
fn relay_error_display_strings_are_descriptive() {
    // Display strings flow into `BunkerHandshakeProgress` failure text;
    // they must carry the cause without panicking.
    assert_eq!(
        RelayError::Connect("tls handshake".to_string()).to_string(),
        "connect failed: tls handshake"
    );
    assert_eq!(
        RelayError::Write("broken pipe".to_string()).to_string(),
        "write failed: broken pipe"
    );
    assert_eq!(
        RelayError::Disconnected.to_string(),
        "relay client disconnected"
    );
}

#[test]
fn default_subscribe_forwards_to_send() {
    // Stub impls that don't override `subscribe` should still receive
    // the frame via `send`, so they keep working without changes.
    struct CountingStub {
        send_count: AtomicUsize,
    }
    impl RelayClient for CountingStub {
        fn send(&self, _frame: String) -> Result<(), RelayError> {
            self.send_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
        fn shutdown(&self) {}
    }
    let stub = CountingStub {
        send_count: AtomicUsize::new(0),
    };
    stub.subscribe("[\"REQ\",\"x\",{}]".to_string()).unwrap();
    assert_eq!(stub.send_count.load(Ordering::Relaxed), 1);
}

// These tests exercise the pure classifier functions that map relay lifecycle
// signals to host-visible connection-state tokens.

#[test]
fn permanent_close_reason_maps_to_failed() {
    use nmp_network::pool::ClosedReason;
    assert_eq!(closed_reason_to_state(&ClosedReason::Permanent), Some("failed"));
}

#[test]
fn requested_close_reason_maps_to_reconnecting() {
    use nmp_network::pool::ClosedReason;
    assert_eq!(
        closed_reason_to_state(&ClosedReason::Requested),
        Some("reconnecting"),
        "Requested must be reconnecting so session-replace doesn't alarm the host"
    );
}

#[test]
fn shutdown_close_reason_maps_to_none() {
    use nmp_network::pool::ClosedReason;
    assert!(
        closed_reason_to_state(&ClosedReason::Shutdown).is_none(),
        "Shutdown is intentional teardown; must not emit a state change"
    );
}

#[test]
fn transient_transport_error_maps_to_reconnecting() {
    use nmp_network::pool::TransportError;
    let err = TransportError {
        message: "connection reset by peer".to_string(),
        permanent: false,
    };
    let (state, reason) = transport_error_to_state(&err);
    assert_eq!(state, "reconnecting");
    assert_eq!(reason.as_deref(), Some("connection reset by peer"));
}

#[test]
fn permanent_transport_error_maps_to_failed() {
    use nmp_network::pool::TransportError;
    let err = TransportError {
        message: "403 Forbidden".to_string(),
        permanent: true,
    };
    let (state, reason) = transport_error_to_state(&err);
    assert_eq!(state, "failed");
    assert_eq!(reason.as_deref(), Some("403 Forbidden"));
}

#[test]
fn relay_client_uses_pool_not_polling() {
    let full = include_str!("../relay_client.rs");
    let production = full
        .split("#[cfg(test)]")
        .next()
        .expect("source has a production half");
    for forbidden in [
        "set_read_timeout",
        "Duration::from_millis(100)",
        "tungstenite::WebSocket",
        "MaybeTlsStream",
        "mio::Poll",
    ] {
        assert!(
            !production
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .filter(|l| !l.trim_start().starts_with("//!"))
                .any(|l| l.contains(forbidden)),
            "relay client regressed to in-broker socket pattern: {forbidden}"
        );
    }
    assert!(
        production.contains("nmp_network::pool::Pool")
            || production.contains("use nmp_network::pool"),
        "relay client must route through nmp_network::Pool (V-13 Stage 2)"
    );
}
