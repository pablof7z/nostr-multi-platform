use super::*;

#[test]
fn publish_raw_serde_default_signer_is_active_when_field_omitted() {
    // Backward-compat: dispatch JSON may omit `signer`; `#[serde(default)]`
    // must deserialize it to Active rather than failing the decode.
    let json = r#"{"PublishRaw":{"kind":1,"tags":[],"content":"hi","target":"Auto"}}"#;
    let action: PublishAction =
        serde_json::from_str(json).expect("legacy PublishRaw JSON must deserialize");
    match action {
        PublishAction::PublishRaw { signer, .. } => assert_eq!(signer, PublishSigner::Active),
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}

#[test]
fn publish_raw_serde_round_trips_registered_signer_provenance() {
    // The selector must also survive the wire when a host supplies it, so
    // a shell can address an agent key by typed provenance + hex pubkey.
    let agent_pk = "a".repeat(64);
    let json = format!(
        r#"{{"PublishRaw":{{"kind":1,"tags":[],"content":"hi","target":"Auto","signer":{{"kind":"registered","pubkey":"{agent_pk}","provenance":"app_managed"}}}}}}"#
    );
    let action: PublishAction =
        serde_json::from_str(&json).expect("PublishRaw JSON with typed signer must deserialize");
    match action {
        PublishAction::PublishRaw { signer, .. } => assert_eq!(
            signer,
            PublishSigner::registered(agent_pk, PublishSignerProvenance::AppManaged)
        ),
        other => panic!("expected PublishRaw, got {other:?}"),
    }
}
