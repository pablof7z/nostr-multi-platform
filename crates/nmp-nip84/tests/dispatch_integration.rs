//! Registry-level dispatch tests for the NIP-84 highlight typed payload.
//!
//! These tests exercise the same byte doorway used by `DispatchEnvelope`:
//! `ActionRegistry::start_bytes` decodes [`PublishHighlightAction`] before
//! running the module's validation.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection, ProtocolDescriptor};
use nmp_nip84::{Nip84Descriptor, PublishHighlightAction};

const NAMESPACE: &str = "nmp.nip84.publish_highlight";

fn registry_with_nip84() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    Nip84Descriptor.register_actions(&mut registry);
    registry
}

#[test]
fn start_bytes_accepts_podcast_external_id_with_kind_tag() {
    let registry = registry_with_nip84();
    let action = PublishHighlightAction {
        content: "quoted clip".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec!["podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f".to_string()],
        external_kinds: vec!["podcast:item:guid".to_string()],
    };
    let payload = action.encode();
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            NAMESPACE,
            &payload,
        )
        .expect("podcast external identifier with NIP-73 kind must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

#[test]
fn start_bytes_rejects_external_id_without_kind_tag() {
    let registry = registry_with_nip84();
    let action = PublishHighlightAction {
        content: "quoted clip".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec!["podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f".to_string()],
        external_kinds: Vec::new(),
    };
    let payload = action.encode();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            NAMESPACE,
            &payload,
        )
        .expect_err("external identifiers require matching NIP-73 kind tags");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("external_ids require"),
            "rejection must name the missing NIP-73 kind: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}
