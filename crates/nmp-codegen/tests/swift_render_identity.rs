use nmp_codegen::swift::render_swift;

const ROW_DOC: &str = r#"{
  "version": 1,
  "types": [{
    "rust_path": "x::SampleRow",
    "swift_name": "SampleRow",
    "id_field": "id",
    "conformances": ["Decodable", "Equatable", "Hashable", "Sendable"],
    "render_identity_fields": ["id", "display_name", "relay_count"],
    "schema": { "type": "object",
      "properties": {
        "id": {"type": "string"},
        "display_name": {"type": "string"},
        "relay_count": {"type": "integer", "format": "uint32"}
      },
      "required": ["id", "display_name", "relay_count"] }
  }]
}"#;

#[test]
fn row_type_emits_render_identity_member_and_conformance() {
    let out = render_swift(ROW_DOC).expect("renders");
    assert!(out.contains("public struct SampleRow:") || out.contains("SampleRow:"));
    assert!(out.contains("RenderIdentifiable"));
    assert!(out.contains("rendersIdentically(_ other: Self) -> Bool") ||
            out.contains("rendersIdentically("));
    assert!(out.contains("self.id == other.id"));
    assert!(out.contains("self.displayName == other.displayName"));
    assert!(out.contains("self.relayCount == other.relayCount"));
}

#[test]
fn non_row_type_emits_no_render_identity() {
    let doc = ROW_DOC.replace(
        r#""render_identity_fields": ["id", "display_name", "relay_count"],"#, "");
    let out = render_swift(&doc).expect("renders");
    // type still emitted
    assert!(out.contains("SampleRow"));
    // but no render identity
    assert!(!out.contains("rendersIdentically"));
    assert!(!out.contains("RenderIdentifiable"));
}
