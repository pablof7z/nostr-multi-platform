//! Registry-level typed payload gates for `nmp.nip84.publish_highlight`.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection, ProtocolDescriptor};
use nmp_nip84::{HighlightSource, Nip84Descriptor, PublishHighlightInput};

const NAMESPACE: &str = "nmp.nip84.publish_highlight";

fn registry_with_nip84() -> ActionRegistry {
    let mut registry = ActionRegistry::new();
    ProtocolDescriptor::register_actions(&Nip84Descriptor, &mut registry);
    registry
}

#[test]
fn start_bytes_accepts_podcast_external_ref_highlight() {
    let registry = registry_with_nip84();
    let action = PublishHighlightInput {
        highlighted_text: String::new(),
        source_refs: vec![HighlightSource::External {
            external_id: "podcast:item:guid:d98d189b-dc7b-45b1-8720-d4b98690f31f".to_string(),
            external_kind: "podcast:item:guid".to_string(),
            hint_url: Some("https://fountain.fm/episode/z1y9TMQRuqXl2awyrQxg".to_string()),
        }],
        ..Default::default()
    };

    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            NAMESPACE,
            &action.encode(),
        )
        .expect("well-formed NIP-84 payload must be accepted");
}

#[test]
fn start_bytes_rejects_wrong_schema_version() {
    let registry = registry_with_nip84();
    let bad = build_bad_version_highlight_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            NAMESPACE,
            &bad,
        )
        .expect_err("wrong schema_version must fail before start()");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

fn build_bad_version_highlight_payload() -> Vec<u8> {
    use flatbuffers::FlatBufferBuilder;

    const IDENTIFIER: &str = "N84H";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_HIGHLIGHTED_TEXT: flatbuffers::VOffsetT = 6;

    let mut fbb = FlatBufferBuilder::new();
    let highlighted_text = fbb.create_string("quoted text");
    let start = fbb.start_table();
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_HIGHLIGHTED_TEXT, highlighted_text);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(IDENTIFIER));
    fbb.finished_data().to_vec()
}
