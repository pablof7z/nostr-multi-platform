use serde::Deserialize;

/// Stage 1 supports exactly version 1 of the schema document. Schema owners
/// bump this in lockstep with any change to the document shape.
pub(crate) const SUPPORTED_DOCUMENT_VERSION: u32 = 1;

/// Parsed shape of each document a `dump_projection_schemas` binary writes.
#[derive(Debug, Deserialize)]
pub(crate) struct ProjectionSchemaDocument {
    pub(crate) version: u32,
    pub(crate) types: Vec<TypeEntry>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct TypeEntry {
    pub(crate) rust_path: String,
    pub(crate) swift_name: String,
    #[serde(default)]
    pub(crate) id_field: Option<String>,
    pub(crate) conformances: Vec<String>,
    #[serde(default)]
    pub(crate) render_identity_fields: Vec<String>,
    pub(crate) schema: TypeSchema,
}

/// Subset of JSON Schema (draft-07) the emitter actually decodes. `schemars`
/// produces richer schemas (`$schema`, `description`, `minimum`, `format` for
/// distinguishing `u32`/`u64`); the emitter ignores what it does not need so
/// future schemars upgrades do not break the decode.
#[derive(Debug, Deserialize)]
pub(crate) struct TypeSchema {
    #[serde(rename = "type", default)]
    pub(crate) ty: Option<serde_json::Value>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    /// Map of field-name to field-schema. `serde_json::Map` with
    /// `preserve_order` keeps insertion order; schemars emits alphabetically,
    /// so iteration order is deterministic regardless.
    #[serde(default)]
    pub(crate) properties: serde_json::Map<String, serde_json::Value>,
    /// JSON-Schema `required` list; fields not in here are optional.
    #[serde(default)]
    pub(crate) required: Vec<String>,
}

pub(crate) fn parse_schema_documents(
    document_json: &str,
) -> Result<Vec<ProjectionSchemaDocument>, serde_json::Error> {
    serde_json::Deserializer::from_str(document_json)
        .into_iter()
        .collect()
}
