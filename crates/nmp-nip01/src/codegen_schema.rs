//! NIP-01-owned projection-type schema export for Swift `Decodable` codegen.
//!
//! `nmp-codegen gen swift` accepts a stream of schema documents. `nmp-core`
//! dumps kernel-owned flat records, and this module dumps the remaining
//! NIP-01-owned flat `TimelineItem` row so the core crate no longer owns that
//! schema.

use schemars::{schema_for, JsonSchema};
use serde::Serialize;
use serde_json::Value;

use crate::TimelineItem;

#[derive(Serialize)]
pub struct TypeEntry {
    pub rust_path: &'static str,
    pub swift_name: &'static str,
    pub id_field: Option<&'static str>,
    pub conformances: &'static [&'static str],
    pub render_identity_fields: &'static [&'static str],
    pub schema: Value,
}

#[derive(Serialize)]
pub struct ProjectionSchemaDocument {
    pub version: u32,
    pub types: Vec<TypeEntry>,
}

fn schema_value<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).unwrap_or(serde_json::Value::Null)
}

#[must_use]
pub fn dump_pilot_schemas() -> ProjectionSchemaDocument {
    ProjectionSchemaDocument {
        version: 1,
        types: vec![TypeEntry {
            rust_path: "nmp_nip01::TimelineItem",
            swift_name: "TimelineItem",
            id_field: Some("id"),
            conformances: &["Decodable", "Equatable", "Hashable", "Sendable"],
            render_identity_fields: &[
                "id",
                "author_pubkey",
                "author_display_name",
                "author_picture_url",
                "author_lnurl",
                "content",
                "content_preview",
                "created_at",
                "is_repost",
                "kind",
                "nav_target_id",
                "repost_inner_content",
                "relay_count",
                "relay_provenance",
            ],
            schema: schema_value::<TimelineItem>(),
        }],
    }
}

#[must_use]
pub fn dump_pilot_schemas_json() -> String {
    serde_json::to_string_pretty(&dump_pilot_schemas()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_document_has_timeline_item_entry() {
        let document = dump_pilot_schemas();
        assert_eq!(document.version, 1);
        let names: Vec<_> = document
            .types
            .iter()
            .map(|entry| entry.swift_name)
            .collect();
        assert_eq!(names, vec!["TimelineItem"]);
    }

    #[test]
    fn timeline_item_entry_has_schema() {
        let document = dump_pilot_schemas();
        assert!(document.types[0].schema.is_object());
    }
}
