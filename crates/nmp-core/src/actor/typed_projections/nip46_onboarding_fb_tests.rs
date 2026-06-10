use super::{
    decode_nip46_onboarding, encode_nip46_onboarding, Nip46OnboardingModel, SignerAppRow,
};

fn sample_apps() -> Vec<SignerAppRow> {
    vec![
        SignerAppRow {
            scheme: "nostrsigner://".to_string(),
            display_label: "Nostr Signer".to_string(),
            signer_kind: "nip46".to_string(),
        },
        SignerAppRow {
            scheme: "primal://".to_string(),
            display_label: "Primal".to_string(),
            signer_kind: "nip46".to_string(),
        },
    ]
}

#[test]
fn round_trips_in_flight_model() {
    let model = Nip46OnboardingModel {
        signer_apps: sample_apps(),
        stage_kind: Some("awaiting_pubkey".to_string()),
        progress_message: Some("approve on bunker".to_string()),
        is_in_flight: true,
        is_failed: false,
        is_terminal_success: false,
        can_cancel: true,
    };
    let bytes = encode_nip46_onboarding(&model);
    let decoded = decode_nip46_onboarding(&bytes).expect("decodes");
    assert_eq!(decoded, model);
}

#[test]
fn round_trips_idle_model_options_none() {
    let model = Nip46OnboardingModel {
        signer_apps: sample_apps(),
        stage_kind: None,
        progress_message: None,
        is_in_flight: false,
        is_failed: false,
        is_terminal_success: false,
        can_cancel: false,
    };
    let bytes = encode_nip46_onboarding(&model);
    let decoded = decode_nip46_onboarding(&bytes).expect("decodes");
    assert_eq!(decoded.stage_kind, None);
    assert_eq!(decoded.progress_message, None);
    assert_eq!(decoded, model);
}

#[test]
fn rejects_foreign_identifier() {
    assert!(decode_nip46_onboarding(b"not a flatbuffer").is_err());
    assert!(decode_nip46_onboarding(&[]).is_err());
}
