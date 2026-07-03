use super::*;

const FIXTURE: &str = r#"{
  "schema": "nmp.concept-reads/1",
  "facade": {
    "rust_type": "GalleryApp",
    "runtime_accessor": "runtime",
    "error_type": "GalleryReadError",
    "invalid_target_variant": "InvalidTarget",
    "open_failed_variant": "OpenFailed"
  },
  "reads": [
    {
      "concept": "replies",
      "opened_record": "GalleryOpenedReplies"
    }
  ],
  "outputs": {
    "rust": "crates/nmp-app-gallery/src/concept_reads_replies.rs",
    "rust_test_module": "concept_reads_replies_tests.rs"
  },
  "drift_checks": [
    "cargo run -p nmp-codegen -- gen concept-reads --registry concept-reads.json --platform rust --check"
  ]
}"#;

#[test]
fn parses_app_registry_and_resolves_default_concept_row() {
    let loaded = parse_app_concept_read_registry(FIXTURE).unwrap();
    assert_eq!(loaded.facade.rust_type, "GalleryApp");
    assert_eq!(loaded.reads.len(), 1);
    assert_eq!(loaded.reads[0].concept.id, "replies");
    assert_eq!(loaded.reads[0].concept.rust_crate, "nmp_replies");
    assert_eq!(loaded.reads[0].opened_record, "GalleryOpenedReplies");
    assert_eq!(
        loaded.outputs.rust_test_module.as_deref(),
        Some("concept_reads_replies_tests.rs")
    );
}

#[test]
fn renders_rust_facade_slice_for_replies() {
    let loaded = parse_app_concept_read_registry(FIXTURE).unwrap();
    let rendered = crate::concept_reads::rust::render_registry(&loaded);
    assert!(rendered.contains("use nmp_replies::{"));
    assert!(rendered.contains("decode_and_validate_reply_target"));
    assert!(rendered.contains("pub struct GalleryOpenedReplies"));
    assert!(rendered.contains("pub enum GalleryReadError"));
    assert!(rendered.contains("#[uniffi::export]\nimpl GalleryApp"));
    assert!(rendered.contains("pub fn open_replies("));
    assert!(rendered.contains("RepliesReadHandle::from_parts"));
    assert!(rendered.contains("#[path = \"concept_reads_replies_tests.rs\"]"));
}

#[test]
fn rejects_unknown_concept() {
    let raw = FIXTURE.replace("\"replies\"", "\"likes\"");
    let err = parse_app_concept_read_registry(&raw).unwrap_err();
    assert!(err.contains("unknown concept read"), "{err}");
}

#[test]
fn rejects_duplicate_concept() {
    let raw = FIXTURE.replace(
        r#"{
      "concept": "replies",
      "opened_record": "GalleryOpenedReplies"
    }"#,
        r#"{
      "concept": "replies",
      "opened_record": "GalleryOpenedReplies"
    },
    {
      "concept": "replies",
      "opened_record": "GalleryOpenedRepliesAgain"
    }"#,
    );
    let err = parse_app_concept_read_registry(&raw).unwrap_err();
    assert!(err.contains("duplicate concept read"), "{err}");
}

#[test]
fn rejects_invalid_rust_identifier() {
    let raw = FIXTURE.replace("GalleryOpenedReplies", "opened-replies");
    let err = parse_app_concept_read_registry(&raw).unwrap_err();
    assert!(err.contains("reads[].opened_record"), "{err}");
    assert!(err.contains("Rust identifier"), "{err}");
}
