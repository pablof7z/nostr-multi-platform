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
            role: "content".to_string(),
            role_tone: "accent".to_string(),
            connection: "connected".to_string(),
            connection_tone: "ok".to_string(),
            auth: "ok".to_string(),
            auth_tone: "ok".to_string(),
            total_sub_count: 3,
            active_sub_count: 2,
            eosed_sub_count: 1,
            total_events_rx: 1234,
            reconnect_count: 1,
            bytes_rx: 4096,
            bytes_tx: 0,
            last_connected_ms: 1_700_000_003_000,
            last_event_ms: 0,
            last_notice: Some("rate limited".to_string()),
            notice_count: 3,
            notices: vec![
                super::NoticeRow { at_ms: 1_700_000_010_000, text: "newest".to_string() },
                super::NoticeRow { at_ms: 1_700_000_001_000, text: "oldest".to_string() },
            ],
            last_error: None,
            wire_subs: vec![WireSubRow {
                wire_id: "ff".repeat(32),
                relay_url: "wss://relay.one".to_string(),
                filter_summary: "kinds:[1]".to_string(),
                state: "open".to_string(),
                state_tone: "ok".to_string(),
                consumer_count: 1,
                events_rx: 42,
                eose_observed: true,
                opened_ms: 1_700_000_000_000,
                last_event_ms: 1_700_000_005_000,
                eose_ms: 1_700_000_008_000,
                close_reason: None,
            }],
            discovery_kinds: vec![0, 3, 10002],
            reasons: vec![],
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

/// bytes_rx/bytes_tx round-trip as u64 zeros when absent.
#[test]
fn bytes_raw_counters_round_trip() {
    let mut model = sample();
    model.relays[0].bytes_rx = 0;
    model.relays[0].bytes_tx = 128;
    let decoded =
        decode_relay_diagnostics(&encode_relay_diagnostics(&model)).expect("decode succeeds");
    assert_eq!(decoded.relays[0].bytes_rx, 0);
    assert_eq!(decoded.relays[0].bytes_tx, 128);
}

/// discovery_kinds round-trips as a Vec<u64>.
#[test]
fn discovery_kinds_round_trip() {
    let mut model = sample();
    model.relays[0].discovery_kinds = vec![0, 3, 10002, 10003];
    let decoded =
        decode_relay_diagnostics(&encode_relay_diagnostics(&model)).expect("decode succeeds");
    assert_eq!(decoded.relays[0].discovery_kinds, vec![0u64, 3, 10002, 10003]);
}

/// Empty discovery_kinds round-trips correctly.
#[test]
fn empty_discovery_kinds_round_trip() {
    let mut model = sample();
    model.relays[0].discovery_kinds = vec![];
    let decoded =
        decode_relay_diagnostics(&encode_relay_diagnostics(&model)).expect("decode succeeds");
    assert!(decoded.relays[0].discovery_kinds.is_empty());
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

/// `reasons` round-trips: a non-empty list with multiple entries — including
/// the "blocked" sentinel — must survive encode/decode with all fields intact.
#[test]
fn reasons_round_trip() {
    let mut model = sample();
    model.relays[0].reasons = vec![
        ConnectionReasonRow {
            kind: "blocked".to_string(),
            label: "Blocked".to_string(),
            tone: "muted".to_string(),
            author_pubkeys: vec![],
            author_total: 0,
            kinds_label: String::new(),
            source_event_id: None,
        },
        ConnectionReasonRow {
            kind: "nip65".to_string(),
            label: "Outbox of 2 people".to_string(),
            tone: "accent".to_string(),
            author_pubkeys: vec![
                "aabbcc".to_string(),
                "ddeeff".to_string(),
            ],
            author_total: 2,
            kinds_label: String::new(),
            source_event_id: None,
        },
        ConnectionReasonRow {
            kind: "hint".to_string(),
            label: "Relay hint".to_string(),
            tone: "warn".to_string(),
            author_pubkeys: vec![],
            author_total: 0,
            kinds_label: String::new(),
            source_event_id: Some("deadbeef".to_string()),
        },
    ];
    let decoded =
        decode_relay_diagnostics(&encode_relay_diagnostics(&model)).expect("decode succeeds");
    assert_eq!(decoded.relays[0].reasons, model.relays[0].reasons);
}
