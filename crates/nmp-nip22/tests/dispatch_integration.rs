//! Registry-level trip tests for the nip22 typed FlatBuffers payload doorway
//! (ADR-0064 / S9 #1747).
//!
//! These are NEGATIVE tests only: they prove the fail-closed `schema_version`
//! gate in `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()`
//! runs, for the `nmp.nip22.post_comment` namespace migrated in S9.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/tests.rs`. These tests sit one level up — at the registry boundary
//! — so they exercise the same path the byte transport (S2 `DispatchEnvelope`)
//! drives in production.

// ---- S9 gap test: bad-version trip for post_comment namespace ---------------

/// ADR-0064 / S9 (#1747) — `nmp.nip22.post_comment` with a bad `schema_version`
/// MUST be rejected BEFORE `start()` runs, proving the fail-closed gate covers
/// the post_comment namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_post_comment() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_nip22::register_actions(&mut registry);

    let bad_version = build_bad_version_post_comment_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.nip22.post_comment",
            &bad_version,
        )
        .expect_err("a wrong schema_version must be rejected before start() (fail closed)");
    match err {
        ActionRejection::Invalid(msg) => assert!(
            msg.contains("schema_version mismatch"),
            "rejection must name the version trip: {msg}"
        ),
        other => panic!("expected Invalid rejection, got {other:?}"),
    }
}

// ---- helper: build a bad-version FlatBuffers buffer --------------------------

/// A finished `PostComment` (file identifier `N22C`) with `schema_version = 999`.
/// The fail-closed gate must reject it before `start` runs.
fn build_bad_version_post_comment_payload() -> Vec<u8> {
    // Inline the generated FlatBuffers API so this integration test does not
    // need to expose the private `wire` module from nmp-nip22. The wire layout
    // (identifier, vtable offsets) is fixed by the committed .fbs schema.
    use flatbuffers::FlatBufferBuilder;

    // Mirror the PostComment vtable slots from post_comment_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_ROOT_TAG_NAME = 6, VT_ROOT_TAG_VALUE = 8,
    //   VT_CONTENT = 18
    const POST_COMMENT_IDENTIFIER: &str = "N22C";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_ROOT_TAG_NAME: flatbuffers::VOffsetT = 6;
    const VT_ROOT_TAG_VALUE: flatbuffers::VOffsetT = 8;
    const VT_CONTENT: flatbuffers::VOffsetT = 18;

    let mut fbb = FlatBufferBuilder::new();
    let root_tag_name = fbb.create_string("E");
    let root_tag_value = fbb.create_string(&"a".repeat(64));
    let content = fbb.create_string("hello");

    let start = fbb.start_table();
    // schema_version = 999 (the tripwire value)
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_ROOT_TAG_NAME, root_tag_name);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_ROOT_TAG_VALUE, root_tag_value);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_CONTENT, content);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(POST_COMMENT_IDENTIFIER));
    fbb.finished_data().to_vec()
}
