//! Registry-level trip tests for the blossom upload typed FlatBuffers payload
//! doorway (ADR-0071 / S9 #1747).
//!
//! These tests prove the fail-closed `schema_version` gate in
//! `ActionRegistry::start_bytes` rejects bad payloads BEFORE `start()` runs,
//! for the `nmp.blossom.upload` namespace.
//!
//! Codec round-trip tests (positive + per-field negative) live in
//! `src/wire/tests.rs`. These tests sit one level up — at the registry boundary
//! — so they exercise the same path the byte transport (S2 `DispatchEnvelope`)
//! drives in production.

// ---- S9 gap tests: bad-version trip for nmp.blossom.upload ------------------

/// ADR-0071 / S9 (#1747) — `nmp.blossom.upload` with a bad `schema_version`
/// MUST be rejected BEFORE `start()` runs, proving the fail-closed gate covers
/// the upload namespace at the registry level.
#[test]
fn start_bytes_rejects_wrong_schema_version_for_upload() {
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionRejection};

    let mut registry = ActionRegistry::new();
    nmp_blossom::register_actions(&mut registry);

    let bad_version = build_bad_version_upload_payload();
    let err = registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.blossom.upload",
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

/// ADR-0071 / S9 (#1747) — round-trip encode → `start_bytes` accepts a
/// well-formed, correct-version payload (positive path through the registry).
#[test]
fn start_bytes_accepts_well_formed_upload_payload() {
    use nmp_blossom::UploadInput;
    use nmp_core::__ffi_internal::ActionRegistry;
    use nmp_core::substrate::{ActionContext, ActionPayload};

    let mut registry = ActionRegistry::new();
    nmp_blossom::register_actions(&mut registry);

    let action = UploadInput {
        file_path: "/tmp/avatar.png".to_string(),
        content_type: Some("image/png".to_string()),
        servers: vec!["https://blossom.example".to_string()],
        signer_pubkey: None,
    };
    let bytes = action.encode();
    registry
        .start_bytes(
            &mut ActionContext::default(),
            1_700_000_000_000,
            "nmp.blossom.upload",
            &bytes,
        )
        .expect("a well-formed payload with correct schema_version must be accepted");
}

// ---- helpers: build bad-version FlatBuffers buffer --------------------------

/// A finished `UploadPayload` (file identifier `BUPL`) with `schema_version = 999`.
/// The fail-closed gate must reject it before `start` runs.
fn build_bad_version_upload_payload() -> Vec<u8> {
    // Inline the generated FlatBuffers API so this integration test does not
    // need to expose the private `wire` module from nmp-blossom. The wire layout
    // (identifier, vtable offsets) is fixed by the committed .fbs schema.
    use flatbuffers::FlatBufferBuilder;

    // Mirror the UploadPayload vtable slots from upload_generated.rs:
    //   VT_SCHEMA_VERSION = 4, VT_FILE_PATH = 6
    const UPLOAD_IDENTIFIER: &str = "BUPL";
    const VT_SCHEMA_VERSION: flatbuffers::VOffsetT = 4;
    const VT_FILE_PATH: flatbuffers::VOffsetT = 6;

    let mut fbb = FlatBufferBuilder::new();
    let file_path = fbb.create_string("/tmp/test.png");

    let start = fbb.start_table();
    // schema_version = 999 (the tripwire value)
    fbb.push_slot::<u32>(VT_SCHEMA_VERSION, 999, 0);
    fbb.push_slot_always::<flatbuffers::WIPOffset<&str>>(VT_FILE_PATH, file_path);
    let root = fbb.end_table(start);
    fbb.finish(root, Some(UPLOAD_IDENTIFIER));
    fbb.finished_data().to_vec()
}
