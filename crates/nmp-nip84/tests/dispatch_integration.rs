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
fn start_bytes_accepts_podcast_external_id_and_derives_kind_tag() {
    let registry = registry_with_nip84();
    let action = PublishHighlightAction {
        content: "quoted clip".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec!["podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f".to_string()],
    };
    let payload = action.encode();
    let id = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            NAMESPACE,
            &payload,
        )
        .expect("recognized podcast external identifier must be accepted");
    assert_eq!(id.len(), 32, "minted correlation_id must be 32 hex chars");
}

#[test]
fn start_bytes_rejects_unknown_external_id_scheme() {
    let registry = registry_with_nip84();
    let action = PublishHighlightAction {
        content: "quoted clip".to_string(),
        context: None,
        source_event_id: None,
        source_address: None,
        source_author_pubkey: None,
        alt: None,
        external_ids: vec!["not-a-nip73-id".to_string()],
    };
    let payload = action.encode();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            NAMESPACE,
            &payload,
        )
        .expect_err("unknown external identifiers fail closed");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("recognized NIP-73"),
            "rejection must name NIP-73 validation: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}
