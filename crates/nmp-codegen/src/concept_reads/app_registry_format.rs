//! JSON shape for app-local concept-read registry input.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistryDocument {
    pub schema: String,
    pub facade: FacadeRow,
    pub reads: Vec<ConceptReadRow>,
    pub outputs: OutputsRow,
    pub drift_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FacadeRow {
    pub rust_type: String,
    pub runtime_accessor: String,
    pub error_type: String,
    pub invalid_target_variant: String,
    pub open_failed_variant: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConceptReadRow {
    pub concept: String,
    pub opened_record: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OutputsRow {
    pub rust: PathBuf,
    pub rust_test_module: Option<String>,
}
