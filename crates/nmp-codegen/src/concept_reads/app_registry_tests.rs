use super::*;

const FIXTURE: &str = r#"{
  "schema": "nmp.concept-reads/1",
  "facade": {
    "rust_type": "GalleryApp",
    "runtime_accessor": "runtime",
    "error_type": "GalleryReadError",
    "invalid_target_variant": "InvalidTarget",
    "open_failed_variant": "OpenFailed",
    "decode_failed_variant": "DecodeFailed"
  },
  "reads": [
    {
      "concept": "replies",
      "opened_record": "GalleryOpenedReplies",
      "summary": {
        "record": "GalleryReplySummary"
      }
    }
  ],
  "outputs": {
    "rust": "crates/nmp-app-gallery/src/concept_reads_replies.rs",
    "rust_test_module": "concept_reads_replies_tests.rs",
    "swift": "ios/NmpGallery/Bridge/Generated/ConceptReads.generated.swift",
    "kotlin": "android/app/src/main/kotlin/org/nmp/gallery/bridge/ConceptReads.kt",
    "kotlin_package": "org.nmp.gallery.bridge",
    "kotlin_uniffi_package": "uniffi.nmp_app_gallery"
  },
  "drift_checks": [
    "cargo run -p nmp-codegen -- gen concept-reads --registry concept-reads.json --platform rust --check"
  ]
}"#;

#[test]
fn platform_parse_accepts_native_wrappers() {
    assert_eq!(
        crate::concept_reads::Platform::parse("rust").unwrap(),
        crate::concept_reads::Platform::Rust
    );
    assert_eq!(
        crate::concept_reads::Platform::parse("swift").unwrap(),
        crate::concept_reads::Platform::Swift
    );
    assert_eq!(
        crate::concept_reads::Platform::parse("kotlin").unwrap(),
        crate::concept_reads::Platform::Kotlin
    );
    assert!(crate::concept_reads::Platform::parse("ts").is_err());
}

#[test]
fn parses_app_registry_and_resolves_default_concept_row() {
    let loaded = parse_app_concept_read_registry(FIXTURE).unwrap();
    assert_eq!(loaded.facade.rust_type, "GalleryApp");
    assert_eq!(loaded.reads.len(), 1);
    assert_eq!(loaded.reads[0].concept.id, "replies");
    assert_eq!(loaded.reads[0].concept.rust_crate, "nmp_replies");
    assert_eq!(loaded.reads[0].opened_record, "GalleryOpenedReplies");
    assert_eq!(loaded.reads[0].summary.record, "GalleryReplySummary");
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
    assert!(rendered.contains("use crate::facade::GalleryApp;"));
    assert!(rendered.contains("decode_and_validate_reply_target"));
    assert!(rendered.contains("pub struct GalleryOpenedReplies"));
    assert!(rendered.contains("pub struct GalleryReplySummary"));
    assert!(rendered.contains("pub enum GalleryReadError"));
    assert!(rendered.contains("DecodeFailed"));
    assert!(rendered.contains("#[uniffi::export]\nimpl GalleryApp"));
    assert!(rendered.contains("pub fn open_replies("));
    assert!(rendered.contains("RepliesReadHandle::from_parts"));
    assert!(rendered.contains("pub fn decode_reply_summary("));
    assert!(rendered.contains("decode_reply_summary_snapshot"));
    assert!(rendered.contains("#[path = \"concept_reads_replies_tests.rs\"]"));
}

#[test]
fn renders_rust_import_from_custom_facade_module() {
    let raw = FIXTURE.replace(
        r#""rust_type": "GalleryApp","#,
        r#""rust_type": "GalleryApp",
    "rust_module": "app","#,
    );
    let loaded = parse_app_concept_read_registry(&raw).unwrap();
    assert_eq!(loaded.facade.rust_module, "app");
    let rendered = crate::concept_reads::rust::render_registry(&loaded);
    assert!(rendered.contains("use crate::app::GalleryApp;"));
}

#[test]
fn rejects_invalid_rust_module() {
    let raw = FIXTURE.replace(
        r#""rust_type": "GalleryApp","#,
        r#""rust_type": "GalleryApp",
    "rust_module": "App","#,
    );
    let err = parse_app_concept_read_registry(&raw).unwrap_err();
    assert!(err.contains("facade.rust_module"), "{err}");
}

#[test]
fn renders_swift_wrappers_for_replies() {
    let loaded = parse_app_concept_read_registry(FIXTURE).unwrap();
    let rendered = crate::concept_reads::swift::render_registry(&loaded);
    assert!(rendered.contains("public enum GeneratedConceptReads"));
    assert!(rendered.contains("public static let replySummarySchemaId"));
    assert!(rendered.contains("try app.openReplies(targetJson: targetJson)"));
    assert!(rendered.contains("app.closeReplies(opened: opened)"));
    assert!(rendered.contains("return try app.decodeReplySummary(payload: payload)"));
}

#[test]
fn renders_kotlin_wrappers_for_replies() {
    let loaded = parse_app_concept_read_registry(FIXTURE).unwrap();
    let rendered = crate::concept_reads::kotlin::render_registry(&loaded);
    assert!(rendered.contains("package org.nmp.gallery.bridge"));
    assert!(rendered.contains("import uniffi.nmp_app_gallery.GalleryApp"));
    assert!(rendered.contains("const val REPLY_SUMMARY_SCHEMA_ID"));
    assert!(rendered.contains("app.openReplies(targetJson)"));
    assert!(rendered.contains("app.closeReplies(opened)"));
    assert!(rendered.contains("return app.decodeReplySummary(payload)"));
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
      "opened_record": "GalleryOpenedReplies",
      "summary": {
        "record": "GalleryReplySummary"
      }
    }"#,
        r#"{
      "concept": "replies",
      "opened_record": "GalleryOpenedReplies",
      "summary": {
        "record": "GalleryReplySummary"
      }
    },
    {
      "concept": "replies",
      "opened_record": "GalleryOpenedRepliesAgain",
      "summary": {
        "record": "GalleryReplySummaryAgain"
      }
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

#[test]
fn validates_nested_summary_records_for_reactions() {
    let raw = FIXTURE.replace(
        r#"{
      "concept": "replies",
      "opened_record": "GalleryOpenedReplies",
      "summary": {
        "record": "GalleryReplySummary"
      }
    }"#,
        r#"{
      "concept": "reactions",
      "opened_record": "GalleryOpenedReactions",
      "summary": {
        "record": "GalleryReactionSummary"
      }
    }"#,
    );
    let err = parse_app_concept_read_registry(&raw).unwrap_err();
    assert!(err.contains("summary.group_record"), "{err}");

    let fixed = raw.replace(
        r#""record": "GalleryReactionSummary""#,
        r#""record": "GalleryReactionSummary",
        "group_record": "GalleryReactionGroupSummary""#,
    );
    let loaded = parse_app_concept_read_registry(&fixed).unwrap();
    assert_eq!(
        loaded.reads[0].summary.group_record.as_deref(),
        Some("GalleryReactionGroupSummary")
    );
}
