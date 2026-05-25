use super::*;

fn fresh_slot() -> BunkerHandshakeSlot {
    new_bunker_handshake_slot()
}

fn set_stage(slot: &BunkerHandshakeSlot, stage: &str, message: Option<&str>) {
    *slot.lock().unwrap() = Some(BunkerHandshakeDto::new(
        stage.to_string(),
        message.map(str::to_string),
    ));
}

#[test]
fn idle_slot_yields_table_only_dto() {
    let slot = fresh_slot();
    let dto = build_nip46_onboarding_dto(&slot);
    assert!(!dto.signer_apps.is_empty(), "signer-app table is static");
    let schemes: Vec<&str> = dto.signer_apps.iter().map(|a| a.scheme.as_str()).collect();
    assert!(schemes.contains(&"nostrsigner://"));
    assert!(schemes.contains(&"primal://"));
    assert_eq!(schemes.last(), Some(&"nostrconnect://"));
    assert!(dto.signer_apps.iter().any(|a| {
        a.scheme == "nostrconnect://" && a.display_label == "Signer App" && a.signer_kind == "nip46"
    }));
    assert_eq!(dto.stage_kind, None);
    assert_eq!(dto.progress_message, None);
    assert!(!dto.is_in_flight);
    assert!(!dto.is_failed);
    assert!(!dto.is_terminal_success);
    assert!(!dto.can_cancel);
}

#[test]
fn connecting_stage_flips_in_flight_and_cancel() {
    let slot = fresh_slot();
    set_stage(&slot, "connecting", Some("connecting to relay wss://r"));
    let dto = build_nip46_onboarding_dto(&slot);
    assert_eq!(dto.stage_kind, Some(BunkerStageKind::Connecting));
    assert!(dto.is_in_flight);
    assert!(dto.can_cancel);
    assert!(!dto.is_failed);
    assert!(!dto.is_terminal_success);
    assert_eq!(
        dto.progress_message.as_deref(),
        Some("connecting to relay wss://r")
    );
}

#[test]
fn awaiting_pubkey_is_in_flight() {
    let slot = fresh_slot();
    set_stage(&slot, "awaiting_pubkey", None);
    let dto = build_nip46_onboarding_dto(&slot);
    assert_eq!(dto.stage_kind, Some(BunkerStageKind::AwaitingPubkey));
    assert!(dto.is_in_flight);
    assert!(dto.can_cancel);
}

#[test]
fn ready_is_terminal_success_only() {
    let slot = fresh_slot();
    set_stage(&slot, "ready", None);
    let dto = build_nip46_onboarding_dto(&slot);
    assert_eq!(dto.stage_kind, Some(BunkerStageKind::Ready));
    assert!(dto.is_terminal_success);
    assert!(!dto.is_in_flight);
    assert!(!dto.can_cancel);
    assert!(!dto.is_failed);
}

#[test]
fn failed_flips_only_is_failed() {
    let slot = fresh_slot();
    set_stage(&slot, "failed", Some("relay connect failed"));
    let dto = build_nip46_onboarding_dto(&slot);
    assert_eq!(dto.stage_kind, Some(BunkerStageKind::Failed));
    assert!(dto.is_failed);
    assert!(!dto.is_in_flight);
    assert!(!dto.can_cancel);
    assert!(!dto.is_terminal_success);
    assert_eq!(
        dto.progress_message.as_deref(),
        Some("relay connect failed")
    );
}

#[test]
fn unknown_stage_maps_to_unknown_variant() {
    let slot = fresh_slot();
    set_stage(&slot, "some_future_stage", None);
    let dto = build_nip46_onboarding_dto(&slot);
    assert_eq!(dto.stage_kind, Some(BunkerStageKind::Unknown));
    assert!(!dto.is_in_flight);
    assert!(!dto.is_failed);
    assert!(!dto.is_terminal_success);
}

#[test]
fn serialized_dto_uses_snake_case_stage_kind() {
    let slot = fresh_slot();
    set_stage(&slot, "awaiting_pubkey", None);
    let dto = build_nip46_onboarding_dto(&slot);
    let v = serde_json::to_value(&dto).unwrap();
    assert_eq!(v["stage_kind"], "awaiting_pubkey");
    assert!(v["signer_apps"].is_array());
    assert_eq!(v["is_in_flight"], true);
    assert_eq!(v["can_cancel"], true);
    assert_eq!(v["is_failed"], false);
    assert_eq!(v["is_terminal_success"], false);
}
