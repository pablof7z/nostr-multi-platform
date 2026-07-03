//! Registry-level dispatch tests for the NIP-84 highlight typed payload.
//!
//! These tests exercise the same byte doorway used by `DispatchEnvelope`:
//! `ActionRegistry::start_bytes` decodes [`PublishHighlightAction`] before
//! running the module's validation.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionRegistrar, ActionRejection,
    ExternalIdValidator, ExternalIdValidatorRegistrar, RegistrationError,
};
use nmp_nip84::PublishHighlightAction;
use std::sync::{Arc, Mutex};

const NAMESPACE: &str = "nmp.nip84.publish_highlight";

fn registry_with_nip84() -> ActionRegistry {
    let mut host = TestHost::default();
    nmp_nip84::register(&mut host, nmp_nip84::Config::default())
        .expect("nmp-nip84 registration must not collide");
    assert!(
        host.external_id_validator
            .lock()
            .expect("validator slot")
            .is_some(),
        "nmp-nip84 must register the protocol external-id validator"
    );
    host.registry
}

#[derive(Default)]
struct TestHost {
    registry: ActionRegistry,
    external_id_validator: Mutex<Option<Arc<dyn ExternalIdValidator>>>,
}

impl ActionRegistrar for TestHost {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), RegistrationError> {
        self.registry.register_action(module)
    }
}

impl ExternalIdValidatorRegistrar for TestHost {
    fn set_external_id_validator(&self, validator: Arc<dyn ExternalIdValidator>) {
        *self.external_id_validator.lock().expect("validator slot") = Some(validator);
    }
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
