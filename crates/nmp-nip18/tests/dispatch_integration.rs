//! Byte-doorway round-trip for the `nmp.nip18.repost` action
//! (M14-1 / PR2 #2145).
//!
//! Proves bytes shaped as the generated `repost` builder emits route END TO END
//! through `ActionRegistry::start_bytes`. The wrong-namespace twin proves the
//! positive is not vacuous.

use nmp_core::__ffi_internal::ActionRegistry;
use nmp_core::dispatch_envelope::{
    decode_dispatch_envelope, encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION,
};
use nmp_core::substrate::{ActionContext, ActionPayload, ActionRejection, ProtocolDescriptor};
use nmp_nip18::{Nip18Descriptor, RepostInput};

const EVENT_ID: &str = "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddff";
const AUTHOR: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

#[test]
fn repost_builder_bytes_dispatch_through_start_bytes() {
    let mut registry = ActionRegistry::new();
    ProtocolDescriptor::register_actions(&Nip18Descriptor, &mut registry);

    let input = RepostInput {
        event_id: EVENT_ID.to_string(),
        author_pubkey: AUTHOR.to_string(),
    };
    let bytes = encode_dispatch_envelope(
        "corr-repost",
        "nmp.nip18.repost",
        DISPATCH_ENVELOPE_SCHEMA_VERSION,
        &input.encode(),
    );
    let decoded = decode_dispatch_envelope(&bytes).expect("envelope must decode (S2)");
    assert_eq!(decoded.action_namespace, "nmp.nip18.repost");
    assert_eq!(
        RepostInput::decode(&decoded.payload).expect("payload must decode"),
        input,
        "repost builder bytes must decode field-for-field"
    );

    // POSITIVE: routed to the right namespace; payload decodes + start() OK.
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            &decoded.action_namespace,
            &decoded.payload,
        )
        .expect("repost builder bytes must dispatch + validate via start_bytes");

    // LOAD-BEARING: the same bytes under an unregistered namespace fail closed.
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip01.publish_note",
            &decoded.payload,
        )
        .expect_err("a RepostPayload routed as publish_note must be rejected");
    assert!(
        matches!(err, ActionRejection::Invalid(_)),
        "wrong-namespace dispatch must fail closed as Invalid, got {err:?}"
    );
}
