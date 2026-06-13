//! Round-trip proof for the `relay_diagnostics` Tier-2 typed codec. This is the
//! #1031 struct->Model path (the struct survives), so the JSON-parse deviation
//! does not apply — the cluster maps the captured struct directly. The
//! end-to-end frame test (`typed_projections_wave_c_diagnostics_tests`) proves
//! the struct->Model mapping against a real captured snapshot; here we prove the
//! codec preserves every field through encode/decode, including the many
//! `Option<String>` presence flags and the nested `wire_subs` / `interests`.

use super::*;

fn sample() -> RelayDiagnosticsModel {
    RelayDiagnosticsModel {
        relays: vec![RelayRow {
            relay_url: "wss://relay.one".to_string(),
            short_url: "relay.one".to_string(),
            role_label: "Content".to_string(),
            role_tone: "primary".to_string(),
            connection_label: "Connected".to_string(),
            connection_tone: "ok".to_string(),
            auth_label: "OK".to_string(),
            auth_tone: "ok".to_string(),
            total_sub_count: 3,
            active_sub_count: 2,
            eosed_sub_count: 1,
            total_events_rx: 1234,
            total_events_display: "1.2K".to_string(),
            reconnect_count: 1,
            bytes_rx_display: Some("4 KB".to_string()),
            bytes_tx_display: None,
            last_connected_ms: 1_700_000_003_000,
            last_event_ms: 0,
            last_notice: Some("rate limited".to_string()),
            last_error: None,
            wire_subs: vec![WireSubRow {
                wire_id: "ff".repeat(32),
                short_wire_id: "ffffffff…".to_string(),
                relay_url: "wss://relay.one".to_string(),
                filter_summary: "kinds:[1]".to_string(),
                state_label: "Open".to_string(),
                state_tone: "ok".to_string(),
                consumer_count_label: "1 consumer".to_string(),
                events_rx_display: Some("42".to_string()),
                eose_observed: true,
                opened_ms: 1_700_000_000_000,
                last_event_ms: 1_700_000_005_000,
                eose_ms: 1_700_000_008_000,
                close_reason: None,
            }],
            info: Some(InfoRow {
                name: Some("Relay One".to_string()),
                description: None,
                icon: Some("https://relay.one/icon.png".to_string()),
                pubkey: Some("abc123".to_string()),
                contact: None,
                software: Some("strfry".to_string()),
                version: Some("0.9.6".to_string()),
                supported_nips: vec![1, 11, 42],
                payment_required: Some(false),
                auth_required: Some(true),
                restricted_writes: None,
            }),
        }],
        interests: vec![InterestRow {
            key: "home".to_string(),
            state: "Live".to_string(),
            state_tone: "ok".to_string(),
            refcount: 2,
            cache_coverage: "full".to_string(),
            relay_urls: vec!["wss://relay.one".to_string(), "wss://relay.two".to_string()],
        }],
    }
}

#[test]
fn encode_decode_round_trips() {
    let model = sample();
    let decoded = decode_relay_diagnostics(&encode_relay_diagnostics(&model))
        .expect("decode must succeed");
    assert_eq!(
        decoded, model,
        "round-trip must preserve every field, nested wire_subs/interests, and \
         every Option presence flag"
    );
}

#[test]
fn empty_snapshot_round_trips() {
    let model = RelayDiagnosticsModel::default();
    let decoded =
        decode_relay_diagnostics(&encode_relay_diagnostics(&model)).expect("decode succeeds");
    assert_eq!(decoded, model);
    assert!(decoded.relays.is_empty());
    assert!(decoded.interests.is_empty());
}

/// Every `Option<String>` must round-trip None distinctly from `Some("")` — the
/// `has_*` presence flags carry the distinction the JSON `null`-vs-`""` carries.
#[test]
fn none_options_distinct_from_empty_string() {
    let mut model = sample();
    model.relays[0].bytes_tx_display = Some(String::new());
    model.relays[0].last_error = None;
    let decoded =
        decode_relay_diagnostics(&encode_relay_diagnostics(&model)).expect("decode succeeds");
    assert_eq!(decoded.relays[0].bytes_tx_display, Some(String::new()));
    assert_eq!(decoded.relays[0].last_error, None);
}

#[test]
fn buffer_carries_the_krdg_file_identifier() {
    let bytes = encode_relay_diagnostics(&sample());
    assert_eq!(&bytes[4..8], RELAY_DIAGNOSTICS_FILE_IDENTIFIER);
}

#[test]
fn decode_rejects_malformed_input() {
    assert!(decode_relay_diagnostics(&[]).is_err());
    assert!(decode_relay_diagnostics(b"NMPU0000").is_err());
}
