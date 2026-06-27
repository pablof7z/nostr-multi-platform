//! Byte-doorway round-trip for the `nmp.nip01.publish_note` action
//! (M14-1 / PR2 #2145).
//!
//! Proves bytes shaped as the generated `publishNote` builder emits (the typed
//! `PublishNotePayload` wrapped in a `DispatchEnvelope`) route END TO END through
//! `ActionRegistry::start_bytes` — S2 envelope decode → typed decode + the
//! fail-closed `schema_version` gate → `start()`. The wrong-namespace twin proves
//! the positive is not vacuous.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::dispatch_envelope::{
    decode_dispatch_envelope, encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION,
};
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection, ProtocolDescriptor};
use nmp_nip01::{Nip01Descriptor, PublishNoteInput};

const PARENT_ID: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const AUTHOR: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn envelope_for(input: &PublishNoteInput) -> Vec<u8> {
    encode_dispatch_envelope(
        "corr-note",
        "nmp.nip01.publish_note",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &input.encode(),
    )
}

#[test]
fn publish_note_builder_bytes_dispatch_through_start_bytes() {
    let mut registry = ActionRegistry::new();
    ProtocolDescriptor::register_actions(&Nip01Descriptor, &mut registry);

    let input = PublishNoteInput {
        content: "hello world".to_string(),
        reply_event_id: Some(PARENT_ID.to_string()),
        reply_author_pubkey: Some(AUTHOR.to_string()),
        ..Default::default()
    };
    let bytes = envelope_for(&input);
    let decoded = decode_dispatch_envelope(&bytes).expect("envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip01.publish_note");
    assert_eq!(
        PublishNoteInput::decode(&decoded.payload).expect("payload must decode"),
        input,
        "publishNote builder bytes must decode field-for-field"
    );

    // POSITIVE: routed to the right namespace; payload decodes + start() OK.
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("publishNote builder bytes must dispatch + validate via start_bytes");

    // LOAD-BEARING: the same bytes under an unregistered namespace fail closed.
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip18.repost",
            &decoded.payload,
        )
        .expect_err("a PublishNotePayload routed as repost must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}

#[test]
fn root_note_builder_bytes_dispatch_through_start_bytes() {
    let mut registry = ActionRegistry::new();
    ProtocolDescriptor::register_actions(&Nip01Descriptor, &mut registry);

    let input = PublishNoteInput {
        content: "just a note".to_string(),
        ..Default::default()
    };
    let bytes = envelope_for(&input);
    let decoded = decode_dispatch_envelope(&bytes).expect("envelope must decode (S2)");
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("root-note builder bytes must dispatch + validate via start_bytes");
}
