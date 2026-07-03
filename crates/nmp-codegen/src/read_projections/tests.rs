use super::*;

fn registry_json(dir: &Path) -> String {
    let schema = dir.join("schemas/notes_inbox.fbs");
    std::fs::create_dir_all(schema.parent().unwrap()).expect("schema dir");
    std::fs::write(
        &schema,
        r#"
namespace app.notes;
table NotesInboxSnapshot {
  schema_version:uint = 1;
  title:string;
}
root_type NotesInboxSnapshot;
file_identifier "NINB";
"#,
    )
    .expect("write schema");
    format!(
        r#"{{
  "schema": "nmp.read-projections/1",
  "snapshot_projections": [{{
    "key": "app.notes.inbox",
    "schema": {{
      "schema_path": "schemas/notes_inbox.fbs",
      "root_type": "NotesInboxSnapshot",
      "file_identifier": "NINB",
      "schema_id": "app.notes.inbox",
      "schema_version": 1
    }},
    "swift": {{
      "field": "notesInbox",
      "domain_type": "NotesInboxSnapshot",
      "reader_type": "app_notes_NotesInboxSnapshot"
    }},
    "kotlin": {{
      "domain_type": "NotesInboxSnapshot",
      "reader_type": "AppNotesInboxSnapshot"
    }},
    "rust": {{
      "rust_crate": "notes-app",
      "module": "projection",
      "producer": "register_notes_inbox_projection"
    }}
  }}],
  "outputs": {{
    "swift_typed_decoders": "generated/TypedProjectionDecoders.generated.swift",
    "swift_projection_cache": "generated/ProjectionCache.generated.swift",
    "kotlin_projection_cache": "generated/ProjectionCache.kt"
  }},
  "drift_checks": ["cargo run -p nmp-codegen -- gen read-projections --registry {} --check"]
}}"#,
        dir.join("read-projections.json").display()
    )
}

#[test]
fn app_registry_generates_and_checks_all_outputs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let registry_path = dir.path().join("read-projections.json");
    std::fs::write(&registry_path, registry_json(dir.path())).expect("write registry");
    let loaded = load_app_read_projection_registry(&registry_path).expect("load");
    validate_app_read_projection_schema_files(&registry_path, &loaded).expect("schema");
    for platform in [
        ReadProjectionPlatform::SwiftTypedDecoders,
        ReadProjectionPlatform::SwiftProjectionCache,
        ReadProjectionPlatform::KotlinProjectionCache,
    ] {
        let out = resolve_registry_output(&registry_path, loaded.output_for(platform));
        generate_read_projections_from_registry(
            platform,
            &loaded,
            &registry_path.to_string_lossy(),
            &out,
        )
        .expect("generate");
    }
    let typed = std::fs::read_to_string(
        dir.path()
            .join("generated/TypedProjectionDecoders.generated.swift"),
    )
    .expect("read typed");
    assert!(typed.contains("enum TypedNotesInboxDecoder {"));
    assert!(typed.contains("static let schemaId = \"app.notes.inbox\""));
    let outcome = check_app_read_projection_registry(&registry_path).expect("check");
    assert!(outcome.up_to_date());
    assert_eq!(outcome.schema_count, 1);
    assert_eq!(outcome.outputs.len(), 3);
}

#[test]
fn app_registry_rejects_builtin_key_collision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw = registry_json(dir.path()).replace("app.notes.inbox", "accounts");
    let err = match parse_app_read_projection_registry(&raw) {
        Ok(_) => panic!("collision should fail"),
        Err(err) => err,
    };
    assert!(err.contains("collides with a built-in NMP projection key"));
}

#[test]
fn app_registry_rejects_unsupported_glue_type() {
    let dir = tempfile::tempdir().expect("tempdir");
    let raw = registry_json(dir.path()).replace(
        r#""reader_type": "app_notes_NotesInboxSnapshot""#,
        r#""reader_type": "app_notes_NotesInboxSnapshot", "glue_type": "CustomGlue""#,
    );
    let err = match parse_app_read_projection_registry(&raw) {
        Ok(_) => panic!("glue type should fail"),
        Err(err) => err,
    };
    assert!(err.contains("Swift glue_type must be"));
}
