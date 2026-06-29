use super::*;
use crate::action_builders::{render_from_registry, Platform};

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
    assert!(swift.contains("public static func publishNote("));
    assert!(swift.contains("actionNamespace: \"app.notes.publish_note\""));
    assert!(swift.contains("UInt32(42)"));
    assert!(swift.contains("fileId: \"APPA\""));
    assert_in_order(
        &swift,
        &["slot 1: title", "slot 2: retryCount", "slot 3: topics"],
    );

    let kotlin = render_from_registry(Platform::Kotlin, &registry);
    assert!(kotlin.contains("fun publishNote("));
    assert!(kotlin.contains("actionNamespace = \"app.notes.publish_note\""));
    assert!(kotlin.contains("fbb.addInt(0, 42, 0)"));
    assert!(kotlin.contains("fbb.finish(payloadRoot, \"APPA\")"));
    assert_in_order(
        &kotlin,
        &["slot 1: title", "slot 2: retryCount", "slot 3: topics"],
    );

    let ts = render_from_registry(Platform::Ts, &registry);
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

fn assert_in_order(haystack: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let next = haystack[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle:?}"));
        cursor += next + needle.len();
    }
}
