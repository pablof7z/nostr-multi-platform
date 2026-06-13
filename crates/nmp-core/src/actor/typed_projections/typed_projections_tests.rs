//! Proof tests for the actor-owned Tier-1 typed projections.
//!
//! These call the SAME builders (`bunker_handshake_typed` /
//! `nip46_onboarding_typed` / `signer_state_typed`) the actor registers via
//! `register_typed` — not a hand-duplicated closure — so they prove the wired
//! behaviour: each typed entry lands (and decodes back) exactly when its JSON
//! counterpart is present, and is absent otherwise.

use super::{
    bunker_handshake_typed, decode_bunker_handshake, decode_nip46_onboarding,
    decode_signer_state, nip46_onboarding_typed, signer_state_typed,
};
use crate::actor::commands::{new_bunker_handshake_slot, BunkerHandshakeDto, BunkerHandshakeSlot};

fn set_stage(slot: &BunkerHandshakeSlot, stage: &str, message: Option<&str>) {
    *slot.lock().unwrap() = Some(BunkerHandshakeDto::new(
        stage.to_string(),
        message.map(str::to_string),
    ));
}

// --- bunker_handshake: conditionally present -------------------------------

#[test]
fn bunker_handshake_absent_when_slot_idle() {
    let slot = new_bunker_handshake_slot();
    // Mirrors the generic closure's JSON `null` while no handshake is in flight:
    // no typed sidecar entry is emitted.
    assert!(
        bunker_handshake_typed(&slot).is_none(),
        "no bunker_handshake sidecar entry while the slot is None"
    );
}

#[test]
fn bunker_handshake_present_and_decodes_when_slot_some() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "connecting", Some("wss://relay.example"));

    let entry = bunker_handshake_typed(&slot).expect("entry present when slot is Some");
    assert_eq!(entry.key, "bunker_handshake");
    assert_eq!(entry.schema_id, "bunker_handshake");
    assert_eq!(entry.schema_version, 1);
    assert_eq!(entry.file_identifier, "KBHS");

    let decoded = decode_bunker_handshake(&entry.payload).expect("round-trips");
    assert_eq!(decoded.stage, "connecting");
    assert_eq!(decoded.message.as_deref(), Some("wss://relay.example"));
    assert!(decoded.is_in_flight);
    assert!(decoded.can_cancel);
    assert!(!decoded.is_idle);
    assert!(!decoded.is_failed);
    assert!(!decoded.is_terminal_success);
    // `stage_label` is pre-formatted server-side (D1: never empty).
    assert!(!decoded.stage_label.is_empty());
}

#[test]
fn bunker_handshake_typed_mirrors_json_message_none() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "failed", None);

    let entry = bunker_handshake_typed(&slot).expect("present");
    let decoded = decode_bunker_handshake(&entry.payload).expect("round-trips");
    // `message: None` → `has_message == false` → decodes back to `None`,
    // mirroring the JSON projection's `null`.
    assert_eq!(decoded.message, None);
    assert!(decoded.is_failed);
}

// --- nip46_onboarding: always present --------------------------------------

#[test]
fn nip46_onboarding_present_even_when_idle() {
    let slot = new_bunker_handshake_slot();
    // The JSON projection is NEVER `null` (static signer-app table), so the
    // typed builder always returns `Some` — even on an idle slot.
    let entry = nip46_onboarding_typed(&slot).expect("always present");
    assert_eq!(entry.key, "nip46_onboarding");
    assert_eq!(entry.schema_id, "nip46_onboarding");
    assert_eq!(entry.schema_version, 1);
    assert_eq!(entry.file_identifier, "KN46");

    let decoded = decode_nip46_onboarding(&entry.payload).expect("round-trips");
    assert!(
        !decoded.signer_apps.is_empty(),
        "static signer-app table is always present"
    );
    let schemes: Vec<&str> = decoded
        .signer_apps
        .iter()
        .map(|a| a.scheme.as_str())
        .collect();
    assert!(schemes.contains(&"nostrsigner://"));
    assert!(schemes.contains(&"primal://"));
    assert!(schemes.contains(&"nostrconnect://"));
    // Idle: no stage, no progress, every flag false.
    assert_eq!(decoded.stage_kind, None);
    assert_eq!(decoded.progress_message, None);
    assert!(!decoded.is_in_flight);
    assert!(!decoded.is_failed);
    assert!(!decoded.is_terminal_success);
    assert!(!decoded.can_cancel);
}

#[test]
fn nip46_onboarding_stage_kind_wire_token_matches_serde() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "awaiting_pubkey", Some("approve on bunker"));

    let entry = nip46_onboarding_typed(&slot).expect("present");
    let decoded = decode_nip46_onboarding(&entry.payload).expect("round-trips");
    // The wire token is derived through serde, so it matches the exact
    // snake_case string the JSON projection emits.
    assert_eq!(decoded.stage_kind.as_deref(), Some("awaiting_pubkey"));
    assert_eq!(decoded.progress_message.as_deref(), Some("approve on bunker"));
    assert!(decoded.is_in_flight);
    assert!(decoded.can_cancel);
}

#[test]
fn nip46_onboarding_failed_stage_sets_is_failed() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "failed", Some("relay unreachable"));

    let entry = nip46_onboarding_typed(&slot).expect("present");
    let decoded = decode_nip46_onboarding(&entry.payload).expect("round-trips");
    assert_eq!(decoded.stage_kind.as_deref(), Some("failed"));
    assert!(decoded.is_failed);
    assert!(!decoded.is_in_flight);
    assert!(!decoded.can_cancel);
}

#[test]
fn nip46_onboarding_ready_stage_is_terminal_success() {
    let slot = new_bunker_handshake_slot();
    set_stage(&slot, "ready", None);

    let entry = nip46_onboarding_typed(&slot).expect("present");
    let decoded = decode_nip46_onboarding(&entry.payload).expect("round-trips");
    assert_eq!(decoded.stage_kind.as_deref(), Some("ready"));
    assert!(decoded.is_terminal_success);
    assert!(!decoded.is_in_flight);
}

// --- signer_state: conditionally present (ADR-0048 D6) ----------------------

#[test]
fn signer_state_absent_when_slot_idle() {
    let slot = crate::actor::commands::new_signer_state_slot();
    // Mirrors the generic closure's JSON `null` while no remote-signer session
    // is active: no typed sidecar entry is emitted.
    assert!(
        signer_state_typed(&slot).is_none(),
        "no signer_state sidecar entry while the slot is None"
    );
}

#[test]
fn signer_state_present_and_decodes_when_slot_some() {
    use crate::actor::commands::{new_signer_state_slot, SignerStateDto};

    let slot = new_signer_state_slot();
    *slot.lock().unwrap() = Some(SignerStateDto::new(
        "nip55".to_string(),
        "awaiting_approval".to_string(),
        None,
    ));

    let entry = signer_state_typed(&slot).expect("entry present when slot is Some");
    assert_eq!(entry.key, "signer_state");
    assert_eq!(entry.schema_id, "signer_state");
    assert_eq!(entry.schema_version, 1);
    assert_eq!(entry.file_identifier, "KSST");

    let decoded = decode_signer_state(&entry.payload).expect("round-trips");
    assert_eq!(decoded.signer_kind, "nip55");
    assert_eq!(decoded.state, "awaiting_approval");
    assert!(decoded.is_awaiting_approval);
    assert!(!decoded.is_ready);
    assert!(!decoded.is_reconnecting);
    assert!(!decoded.is_unavailable);
    assert!(!decoded.is_failed);
    assert_eq!(decoded.reason, None);
    // ADR-0032 / #1099: Rust-precomputed label/tone flow through the typed wire.
    assert_eq!(decoded.status_label, "Waiting for approval…");
    assert_eq!(decoded.status_tone, "warning");
}

#[test]
fn signer_state_typed_mirrors_json_for_nip46_degraded_state() {
    use crate::actor::commands::{new_signer_state_slot, SignerStateDto};

    let slot = new_signer_state_slot();
    *slot.lock().unwrap() = Some(SignerStateDto::from_nip46_connection_state(
        "reconnecting",
        Some("connection reset by peer".to_string()),
    ));

    let entry = signer_state_typed(&slot).expect("present");
    let decoded = decode_signer_state(&entry.payload).expect("round-trips");
    assert_eq!(decoded.signer_kind, "nip46");
    assert_eq!(decoded.state, "reconnecting");
    assert!(decoded.is_reconnecting);
    assert_eq!(decoded.reason.as_deref(), Some("connection reset by peer"));
    assert_eq!(decoded.status_label, "Reconnecting…");
    assert_eq!(decoded.status_tone, "warning");
}
