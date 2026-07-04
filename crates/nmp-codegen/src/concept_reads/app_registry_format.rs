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
    /// Module path the facade type lives in, e.g. `facade` or `app`.
    /// Defaults to `facade` for backward compatibility with registries
    /// predating #3004.
    #[serde(default = "default_rust_module")]
    pub rust_module: String,
    pub runtime_accessor: String,
    pub error_type: String,
    pub invalid_target_variant: String,
    pub open_failed_variant: String,
    pub decode_failed_variant: String,
}

fn default_rust_module() -> String {
    "facade".to_string()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConceptReadRow {
    pub concept: String,
    pub opened_record: String,
    pub summary: SummaryRow,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SummaryRow {
    pub record: String,
    pub group_record: Option<String>,
    pub zapper_record: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OutputsRow {
    pub rust: PathBuf,
    pub rust_test_module: Option<String>,
    pub swift: Option<PathBuf>,
    pub kotlin: Option<PathBuf>,
    pub kotlin_package: Option<String>,
    pub kotlin_uniffi_package: Option<String>,
}
