use super::*;
use crate::action_builders::registry::ACTION_BUILDERS;
use crate::action_builders::{
    check_app_action_builder_registry, render_from_registry,
    validate_app_action_builder_schema_files, AppActionBuilderOutputCheck, Platform,
};
use crate::action_contract::ACTION_CONTRACT;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE: &str = r#"{
  "actions": [
    {
      "action_namespace": "app.notes.publish_note",
      "event_kind": 30444,
      "dispatch": "publishes_event",
      "schema": {
        "schema_path": "schemas/publish_note.fbs",
        "root_type": "PublishNotePayload",
        "file_identifier": "APPA",
        "schema_id": "app.notes.publish_note",
        "schema_version": 42
      },
      "builder": {
        "method": "publishNote",
        "doc": "Publish an app-private note event.",
        "fields": [
          { "name": "title", "kind": "string" },
          { "name": "retryCount", "kind": "uint" },
          { "name": "topics", "kind": "string_vec", "optional": true }
        ]
      },
      "rust": {
        "rust_crate": "notes-app",
        "module": "note_kind",
        "payload_type": "PublishNotePayload",
        "action_module": "PublishNoteActionModule"
      }
    }
  ],
  "outputs": {
    "swift": "ios/Generated/ActionBuilders.generated.swift",
    "kotlin": "android/app/src/main/java/app/notes/ActionBuilders.kt",
    "ts": "web/src/actionBuilders.generated.ts"
  },
  "drift_checks": [
    "cargo run -p nmp-codegen -- gen action-builders --registry action-builders.json --platform swift --check"
  ]
}"#;

#[test]
fn parses_app_local_registry_and_preserves_field_order() {
    let loaded = parse_app_action_builder_registry(FIXTURE).unwrap();
    assert_eq!(loaded.builders.len(), 1);
    let builder = &loaded.builders[0];
    assert_eq!(builder.namespace, "app.notes.publish_note");
    assert_eq!(builder.method, "publishNote");
    assert_eq!(builder.fields[0].name, "title");
    assert_eq!(builder.fields[1].name, "retryCount");
    assert_eq!(builder.fields[2].name, "topics");
    assert!(builder.fields[2].optional);

    let registry = loaded.as_registry();
    let wire = registry.wire_contract_for("app.notes.publish_note");
    assert_eq!(wire.schema_version, 42);
    assert_eq!(wire.file_identifier, "APPA");
}

#[test]
fn renders_app_local_swift_kotlin_and_ts_builders() {
    let loaded = parse_app_action_builder_registry(FIXTURE).unwrap();
    let registry = loaded.as_registry();

    let swift = render_from_registry(Platform::Swift, &registry);
    assert_app_local_header(&swift);
    assert!(swift.contains("public static func publishNote("));
    assert!(swift.contains("actionNamespace: \"app.notes.publish_note\""));
    assert!(swift.contains("UInt32(42)"));
    assert!(swift.contains("fileId: \"APPA\""));
    assert_in_order(
        &swift,
        &["slot 1: title", "slot 2: retryCount", "slot 3: topics"],
    );

    let kotlin = render_from_registry(Platform::Kotlin, &registry);
    assert_app_local_header(&kotlin);
    assert!(kotlin.contains("fun publishNote("));
    assert!(kotlin.contains("actionNamespace = \"app.notes.publish_note\""));
    assert!(kotlin.contains("fbb.addInt(0, 42, 0)"));
    assert!(kotlin.contains("fbb.finish(payloadRoot, \"APPA\")"));
    assert_in_order(
        &kotlin,
        &["slot 1: title", "slot 2: retryCount", "slot 3: topics"],
    );

    let ts = render_from_registry(Platform::Ts, &registry);
    assert_app_local_header(&ts);
    assert!(ts.contains("publishNote("));
    assert!(ts.contains("encodeDispatchEnvelope(correlationId, \"app.notes.publish_note\""));
    assert!(ts.contains("fbb.addFieldInt32(0, 42, 0)"));
    assert!(ts.contains("fbb.finish(payloadRoot, \"APPA\")"));
    assert_in_order(
        &ts,
        &["slot 1: title", "slot 2: retryCount", "slot 3: topics"],
    );
}

#[test]
fn app_local_registry_rows_are_not_builtin_action_rows() {
    let loaded = parse_app_action_builder_registry(FIXTURE).unwrap();
    let builder = &loaded.builders[0];
    let schema = &loaded.schemas[0];
    let raw: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();

    assert_eq!(builder.namespace, "app.notes.publish_note");
    assert_eq!(raw["actions"][0]["event_kind"], 30444);
    assert_eq!(schema.action_namespace, builder.namespace);
    assert_eq!(schema.file_identifier, "APPA");
    assert_eq!(schema.schema_version, 42);
    assert!(
        ACTION_CONTRACT
            .iter()
            .all(|contract| contract.namespace != builder.namespace),
        "app-private namespaces must stay out of the built-in ACTION_CONTRACT"
    );
    assert!(
        ACTION_BUILDERS
            .iter()
            .all(|builtin| builtin.namespace != builder.namespace),
        "app-private namespaces must stay out of the built-in ACTION_BUILDERS"
    );
}

fn assert_app_local_header(rendered: &str) {
    assert!(rendered.contains("app-local action-builders registry JSON"));
    assert!(rendered.contains("NOT NMP's built-in `ACTION_BUILDERS` table"));
}

#[test]
fn rejects_missing_presence_flag_for_presence_encoded_ulong() {
    let raw = FIXTURE.replace(
        r#"{ "name": "retryCount", "kind": "uint" }"#,
        r#"{ "name": "expiresAt", "kind": "ulong_with_presence_flag" }"#,
    );
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject missing presence_flag"),
        Err(err) => err,
    };
    assert!(err.contains("requires presence_flag"), "{err}");
}

#[test]
fn rejects_duplicate_action_namespace() {
    let raw = FIXTURE.replace(
        r#""actions": ["#,
        r#""actions": [
    {
      "action_namespace": "app.notes.publish_note",
      "event_kind": 30445,
      "dispatch": "app_local",
      "schema": {
        "schema_path": "schemas/publish_note_again.fbs",
        "root_type": "PublishNoteAgainPayload",
        "file_identifier": "APPB",
        "schema_id": "app.notes.publish_note_again",
        "schema_version": 43
      },
      "builder": {
        "method": "publishNoteAgain",
        "doc": "Publish another app-private note event.",
        "fields": [
          { "name": "title", "kind": "string" }
        ]
      },
      "rust": {
        "rust_crate": "notes-app",
        "module": "note_kind",
        "payload_type": "PublishNoteAgainPayload",
        "action_module": "PublishNoteAgainActionModule"
      }
    },"#,
    );
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject duplicate namespaces"),
        Err(err) => err,
    };
    assert!(err.contains("duplicate action_namespace"), "{err}");
}

#[test]
fn rejects_builtin_action_namespace_collision() {
    let raw = FIXTURE.replace("app.notes.publish_note", "nmp.publish");
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject built-in namespace collision"),
        Err(err) => err,
    };
    assert!(
        err.contains("collides with a built-in NMP action namespace"),
        "{err}"
    );
    assert!(err.contains("app-owned namespace"), "{err}");
}

#[test]
fn rejects_invalid_generated_method_identifier() {
    let raw = FIXTURE.replace(r#""method": "publishNote""#, r#""method": "publish-note""#);
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject invalid generated method"),
        Err(err) => err,
    };
    assert!(err.contains("builder.method"), "{err}");
    assert!(err.contains("[a-z][A-Za-z0-9]*"), "{err}");
}

#[test]
fn rejects_invalid_generated_field_identifier() {
    let raw = FIXTURE.replace(r#""name": "title""#, r#""name": "1title""#);
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject invalid generated field"),
        Err(err) => err,
    };
    assert!(err.contains("builder field name"), "{err}");
    assert!(err.contains("[a-z][A-Za-z0-9]*"), "{err}");
}

#[test]
fn rejects_reserved_generated_identifiers() {
    let raw = FIXTURE.replace(r#""name": "title""#, r#""name": "class""#);
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject reserved generated field"),
        Err(err) => err,
    };
    assert!(err.contains("reserved by Swift/Kotlin/TypeScript"), "{err}");
    assert!(err.contains("class"), "{err}");
}

#[test]
fn rejects_duplicate_generated_field_symbols() {
    let raw = FIXTURE.replace(
        r#"{ "name": "title", "kind": "string" },"#,
        r#"{ "name": "title", "kind": "string" },
          { "name": "title", "kind": "string" },"#,
    );
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject duplicate generated field symbols"),
        Err(err) => err,
    };
    assert!(err.contains("duplicate generated symbol"), "{err}");
    assert!(err.contains("title"), "{err}");
}

#[test]
fn rejects_presence_flag_symbol_collision() {
    let raw = FIXTURE.replace(
        r#"{ "name": "retryCount", "kind": "uint" }"#,
        r#"{ "name": "expiresAt", "kind": "ulong_with_presence_flag", "presence_flag": "title" }"#,
    );
    let err = match parse_app_action_builder_registry(&raw) {
        Ok(_) => panic!("registry should reject duplicate presence flag symbols"),
        Err(err) => err,
    };
    assert!(err.contains("duplicate generated symbol"), "{err}");
    assert!(err.contains("presence_flag"), "{err}");
}

#[test]
fn validates_fixture_schema_files() {
    let path = fixture_registry_path();
    let loaded = load_app_action_builder_registry(&path).unwrap();
    validate_app_action_builder_schema_files(&path, &loaded).unwrap();
}

#[test]
fn rejects_schema_file_identifier_drift() {
    let tmp = copy_fixture_to_temp("fid-drift");
    let schema = tmp.path().join("schemas/publish_note.fbs");
    let raw = fs::read_to_string(&schema).unwrap();
    fs::write(
        &schema,
        raw.replace(r#"file_identifier "APPA";"#, r#"file_identifier "BAD!";"#),
    )
    .unwrap();
    let err = check_app_action_builder_registry(&tmp.path().join("action-builders.json"))
        .expect_err("mismatched file_identifier should fail");
    assert!(err.contains("file_identifier"), "{err}");
}

#[test]
fn rejects_schema_version_field_drift() {
    let tmp = copy_fixture_to_temp("version-drift");
    let schema = tmp.path().join("schemas/publish_note.fbs");
    let raw = fs::read_to_string(&schema).unwrap();
    fs::write(
        &schema,
        raw.replace("schema_version:uint;", "schema_version:ulong;"),
    )
    .unwrap();
    let err = check_app_action_builder_registry(&tmp.path().join("action-builders.json"))
        .expect_err("wrong schema_version type should fail");
    assert!(err.contains("schema_version"), "{err}");
}

#[test]
fn fixture_registry_passes_full_drift_check() {
    let outcome = check_app_action_builder_registry(&fixture_registry_path()).unwrap();
    assert_eq!(outcome.schema_count, 1);
    assert_eq!(outcome.outputs.len(), 3);
    assert!(outcome.up_to_date());
}

#[test]
fn stale_swift_fixture_fails_drift_check() {
    assert_stale_fixture_fails(
        Platform::Swift,
        "stale/ActionBuilders.generated.swift",
        "generated/ActionBuilders.generated.swift",
    );
}

#[test]
fn stale_kotlin_fixture_fails_drift_check() {
    assert_stale_fixture_fails(
        Platform::Kotlin,
        "stale/ActionBuilders.kt",
        "generated/ActionBuilders.kt",
    );
}

#[test]
fn stale_ts_fixture_fails_drift_check() {
    assert_stale_fixture_fails(
        Platform::Ts,
        "stale/actionBuilders.generated.ts",
        "generated/actionBuilders.generated.ts",
    );
}

fn assert_in_order(haystack: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let next = haystack[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?}"));
        cursor += next + needle.len();
    }
}

fn assert_stale_fixture_fails(platform: Platform, stale: &str, output: &str) {
    let tmp = copy_fixture_to_temp(&format!("{platform:?}-stale"));
    fs::copy(tmp.path().join(stale), tmp.path().join(output)).unwrap();
    let outcome = check_app_action_builder_registry(&tmp.path().join("action-builders.json"))
        .expect("stale generated output should report drift, not schema failure");
    let stale = outcome
        .outputs
        .iter()
        .find(|output| output.platform == platform)
        .unwrap_or_else(|| panic!("missing platform {platform:?} in outcome"));
    assert_output_stale(stale);
}

fn assert_output_stale(output: &AppActionBuilderOutputCheck) {
    assert!(!output.outcome.up_to_date);
    assert_eq!(output.outcome.first_diff_line, Some(1));
}

fn fixture_registry_path() -> PathBuf {
    fixture_dir().join("action-builders.json")
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/app_action_builders")
}

fn copy_fixture_to_temp(name: &str) -> tempfile::TempDir {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("nmp-app-action-builders-{name}-"))
        .tempdir()
        .unwrap();
    copy_dir(&fixture_dir(), tmp.path());
    tmp
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir(&src_path, &dst_path);
        } else {
            fs::copy(&src_path, &dst_path).unwrap();
        }
    }
}
