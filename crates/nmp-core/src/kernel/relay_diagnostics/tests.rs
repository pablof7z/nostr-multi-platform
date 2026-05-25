use super::*;

#[test]
fn format_ago_buckets() {
    assert_eq!(format_ago_ms(10_000, 9_500), "0s ago");
    assert_eq!(format_ago_ms(60_000, 0), "now"); // then==0 means never observed
    assert_eq!(format_ago_ms(120_000, 60_000), "1m ago");
    assert_eq!(format_ago_ms(3_700_000, 100_000), "1h ago");
    assert_eq!(format_ago_ms(90_000_000, 0_001), "1d ago");
}

#[test]
fn compact_count_buckets() {
    assert_eq!(compact_count(0), "0");
    assert_eq!(compact_count(42), "42");
    assert_eq!(compact_count(999), "999");
    assert_eq!(compact_count(1_000), "1K");
    assert_eq!(compact_count(1_234), "1.2K");
    assert_eq!(compact_count(1_000_000), "1M");
    assert_eq!(compact_count(2_500_000), "2.5M");
}

#[test]
fn short_relay_strips_scheme_and_trailing_slash() {
    assert_eq!(short_relay_url("wss://relay.example/"), "relay.example");
    assert_eq!(short_relay_url("ws://relay.example/path"), "relay.example/path");
    assert_eq!(short_relay_url("relay.example"), "relay.example");
}

#[test]
fn connection_tone_classifies_states() {
    assert_eq!(connection_tone("connected"), "ok");
    assert_eq!(connection_tone("Reconnecting"), "warn");
    assert_eq!(connection_tone("Disconnected"), "error");
    assert_eq!(connection_tone("unknown"), "muted");
}

#[test]
fn snapshot_emits_one_row_per_known_relay() {
    use crate::relay::DEFAULT_VISIBLE_LIMIT;
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let snap = kernel.relay_diagnostics_snapshot();
    // Bootstrap roles (Content + Indexer) are always present.
    let roles: Vec<_> = snap.relays.iter().map(|r| r.role_label.as_str()).collect();
    assert!(
        roles.iter().any(|r| *r == "Content"),
        "expected Content lane in roles {:?}",
        roles
    );
    assert!(
        roles.iter().any(|r| *r == "Indexer"),
        "expected Indexer lane in roles {:?}",
        roles
    );
    // Every relay row has roll-up counters zeroed (no subs yet).
    for row in &snap.relays {
        assert_eq!(row.total_sub_count, 0);
        assert_eq!(row.active_sub_count, 0);
        assert_eq!(row.eosed_sub_count, 0);
        assert_eq!(row.total_events_rx, 0);
        assert_eq!(row.total_events_display, "0");
    }
    // The interest snapshot includes the always-on lanes.
    assert!(snap.interests.iter().any(|i| i.key == "Timeline"));
    // Every interest carries a non-empty semantic tone.
    for interest in &snap.interests {
        assert!(!interest.state_tone.is_empty());
    }
}

#[test]
fn snapshot_emits_every_transport_url_for_same_role() {
    use crate::relay::{DEFAULT_VISIBLE_LIMIT, RelayRole};

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.relay_connecting_url(RelayRole::Content, "wss://relay-a.test/");
    kernel.relay_connected_url(RelayRole::Content, "wss://relay-a.test/");
    kernel.relay_connecting_url(RelayRole::Content, "wss://relay-b.test/");
    kernel.relay_connected_url(RelayRole::Content, "wss://relay-b.test/");
    kernel.record_tx_to(RelayRole::Content, "wss://relay-b.test/", 128);

    let snap = kernel.relay_diagnostics_snapshot();
    let relay_a = snap
        .relays
        .iter()
        .find(|row| row.relay_url == "wss://relay-a.test")
        .expect("diagnostics must include the first content socket URL");
    let relay_b = snap
        .relays
        .iter()
        .find(|row| row.relay_url == "wss://relay-b.test")
        .expect("diagnostics must include the second content socket URL");

    assert_eq!(relay_a.role_label, "Content");
    assert_eq!(relay_a.connection_label, "Connected");
    assert_eq!(relay_b.role_label, "Content");
    assert_eq!(relay_b.connection_label, "Connected");
    assert_eq!(relay_b.bytes_tx_display.as_deref(), Some("128 B"));
}
