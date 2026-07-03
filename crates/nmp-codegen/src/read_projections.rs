//! App-local read-projection registry input.
//!
//! Built-in NMP read generators are sourced from `SNAPSHOT_PROJECTIONS` plus
//! `PROJECTION_CONTRACT`. App crates need the same generated host helpers for
//! app-owned projection keys without adding app nouns to NMP's built-in tables.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::kotlin_projection_cache::{
    check_kotlin_projection_cache_for_app, generate_kotlin_projection_cache_for_app,
    KotlinProjectionCacheCheckOutcome,
};
use crate::swift_projection_cache::{
    check_projection_cache_for_app, generate_projection_cache_for_app, ProjectionCacheCheckOutcome,
};
use crate::swift_projections_registry::{
    SnapshotProjectionEntry, TypedSidecar, SNAPSHOT_PROJECTIONS,
};
use crate::swift_typed_decoders::{
    render_typed_decoders_from_contracts, ProjectionDecoderContract,
};

const REGISTRY_SCHEMA: &str = "nmp.read-projections/1";

#[path = "read_projections/schema.rs"]
mod schema;
pub use schema::{validate_app_read_projection_schema_files, AppReadProjectionSchema};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadProjectionPlatform {
    SwiftTypedDecoders,
    SwiftProjectionCache,
    KotlinProjectionCache,
}

impl ReadProjectionPlatform {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "swift-typed-decoders" => Ok(Self::SwiftTypedDecoders),
            "swift-projection-cache" => Ok(Self::SwiftProjectionCache),
            "kotlin-projection-cache" => Ok(Self::KotlinProjectionCache),
            other => Err(format!(
                "unknown read-projections --platform {other:?}: expected swift-typed-decoders, swift-projection-cache, or kotlin-projection-cache"
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::SwiftTypedDecoders => "swift-typed-decoders",
            Self::SwiftProjectionCache => "swift-projection-cache",
            Self::KotlinProjectionCache => "kotlin-projection-cache",
        }
    }
}

pub struct LoadedAppReadProjectionRegistry {
    pub entries: Vec<SnapshotProjectionEntry>,
    pub contracts: Vec<ProjectionDecoderContract<'static>>,
    pub outputs: AppReadProjectionOutputs,
    pub schemas: Vec<AppReadProjectionSchema>,
}

impl LoadedAppReadProjectionRegistry {
    pub fn output_for(&self, platform: ReadProjectionPlatform) -> &Path {
        match platform {
            ReadProjectionPlatform::SwiftTypedDecoders => &self.outputs.swift_typed_decoders,
            ReadProjectionPlatform::SwiftProjectionCache => &self.outputs.swift_projection_cache,
            ReadProjectionPlatform::KotlinProjectionCache => &self.outputs.kotlin_projection_cache,
        }
    }
}

pub struct AppReadProjectionOutputs {
    pub swift_typed_decoders: PathBuf,
    pub swift_projection_cache: PathBuf,
    pub kotlin_projection_cache: PathBuf,
}

pub struct AppReadProjectionRegistryCheckOutcome {
    pub schema_count: usize,
    pub outputs: Vec<AppReadProjectionOutputCheck>,
}

impl AppReadProjectionRegistryCheckOutcome {
    pub fn up_to_date(&self) -> bool {
        self.outputs.iter().all(|output| output.outcome.up_to_date)
    }
}

pub struct AppReadProjectionOutputCheck {
    pub platform: ReadProjectionPlatform,
    pub out_path: PathBuf,
    pub outcome: AppReadProjectionCheckOutcome,
}

pub struct AppReadProjectionCheckOutcome {
    pub up_to_date: bool,
    pub first_diff_line: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryDocument {
    schema: String,
    snapshot_projections: Vec<ProjectionRow>,
    outputs: OutputsRow,
    drift_checks: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionRow {
    key: String,
    schema: SchemaRow,
    swift: SwiftRow,
    #[serde(default)]
    kotlin: Option<KotlinRow>,
    rust: RustOwnerRow,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaRow {
    schema_path: PathBuf,
    root_type: String,
    file_identifier: String,
    schema_id: String,
    schema_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SwiftRow {
    field: String,
    domain_type: String,
    reader_type: String,
    #[serde(default = "default_glue_type")]
    glue_type: String,
}

fn default_glue_type() -> String {
    "TypedProjectionGlue".to_string()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KotlinRow {
    domain_type: String,
    reader_type: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RustOwnerRow {
    rust_crate: String,
    module: String,
    producer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputsRow {
    swift_typed_decoders: PathBuf,
    swift_projection_cache: PathBuf,
    kotlin_projection_cache: PathBuf,
}

pub fn load_app_read_projection_registry(
    path: &Path,
) -> Result<LoadedAppReadProjectionRegistry, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read app read-projections registry {}: {e}", path.display()))?;
    parse_app_read_projection_registry(&raw)
}

pub fn parse_app_read_projection_registry(
    raw: &str,
) -> Result<LoadedAppReadProjectionRegistry, String> {
    let doc: RegistryDocument = serde_json::from_str(raw)
        .map_err(|e| format!("parse app read-projections registry JSON: {e}"))?;
    if doc.schema != REGISTRY_SCHEMA {
        return Err(format!(
            "app read-projections registry schema must be {REGISTRY_SCHEMA:?}, got {:?}",
            doc.schema
        ));
    }
    if doc.snapshot_projections.is_empty() {
        return Err(
            "app read-projections registry must declare at least one projection".to_string(),
        );
    }
    if doc.drift_checks.is_empty() {
        return Err("app read-projections registry must declare drift_checks".to_string());
    }

    let mut keys = BTreeSet::new();
    let mut schema_ids = BTreeSet::new();
    let mut swift_fields = BTreeSet::new();
    let mut entries = Vec::with_capacity(doc.snapshot_projections.len());
    let mut contracts = Vec::with_capacity(doc.snapshot_projections.len());
    let mut schemas = Vec::with_capacity(doc.snapshot_projections.len());

    for row in doc.snapshot_projections {
        validate_projection_row(&row)?;
        if !keys.insert(row.key.clone()) {
            return Err(format!("duplicate projection key {:?}", row.key));
        }
        if !schema_ids.insert(row.schema.schema_id.clone()) {
            return Err(format!("duplicate schema_id {:?}", row.schema.schema_id));
        }
        if !swift_fields.insert(row.swift.field.clone()) {
            return Err(format!("duplicate Swift field {:?}", row.swift.field));
        }

        schemas.push(AppReadProjectionSchema {
            key: row.key.clone(),
            schema_path: row.schema.schema_path.clone(),
            root_type: row.schema.root_type.clone(),
            file_identifier: row.schema.file_identifier.clone(),
            schema_version: row.schema.schema_version,
            schema_id: row.schema.schema_id.clone(),
        });

        let key = leak_str(row.key);
        let schema_id = leak_str(row.schema.schema_id);
        let file_identifier = leak_str(row.schema.file_identifier);
        contracts.push(ProjectionDecoderContract {
            key,
            schema_id,
            file_identifier,
        });
        entries.push(SnapshotProjectionEntry {
            key,
            swift_field: leak_str(row.swift.field),
            swift_type: leak_str(row.swift.domain_type),
            typed_sidecar: Some(TypedSidecar {
                swift_reader_type: Some(leak_str(row.swift.reader_type)),
            }),
        });
    }

    Ok(LoadedAppReadProjectionRegistry {
        entries,
        contracts,
        outputs: AppReadProjectionOutputs {
            swift_typed_decoders: doc.outputs.swift_typed_decoders,
            swift_projection_cache: doc.outputs.swift_projection_cache,
            kotlin_projection_cache: doc.outputs.kotlin_projection_cache,
        },
        schemas,
    })
}

pub fn generate_read_projections_from_registry(
    platform: ReadProjectionPlatform,
    loaded: &LoadedAppReadProjectionRegistry,
    source_label: &str,
    out_path: &Path,
) -> Result<(), String> {
    match platform {
        ReadProjectionPlatform::SwiftTypedDecoders => {
            let rendered = render_typed_decoders_from_contracts(
                &loaded.entries,
                &loaded.contracts,
                source_label,
            )?;
            write_generated(out_path, rendered).map_err(|e| e.to_string())
        }
        ReadProjectionPlatform::SwiftProjectionCache => {
            generate_projection_cache_for_app(&loaded.entries, source_label, out_path)
                .map_err(|e| e.to_string())
        }
        ReadProjectionPlatform::KotlinProjectionCache => {
            generate_kotlin_projection_cache_for_app(&loaded.entries, source_label, out_path)
                .map_err(|e| e.to_string())
        }
    }
}

pub fn check_read_projections_from_registry(
    platform: ReadProjectionPlatform,
    loaded: &LoadedAppReadProjectionRegistry,
    source_label: &str,
    out_path: &Path,
) -> Result<AppReadProjectionCheckOutcome, String> {
    match platform {
        ReadProjectionPlatform::SwiftTypedDecoders => {
            let rendered = render_typed_decoders_from_contracts(
                &loaded.entries,
                &loaded.contracts,
                source_label,
            )?;
            check_rendered(out_path, rendered).map_err(|e| e.to_string())
        }
        ReadProjectionPlatform::SwiftProjectionCache => Ok(swift_cache_outcome(
            check_projection_cache_for_app(&loaded.entries, source_label, out_path)
                .map_err(|e| e.to_string())?,
        )),
        ReadProjectionPlatform::KotlinProjectionCache => Ok(kotlin_cache_outcome(
            check_kotlin_projection_cache_for_app(&loaded.entries, source_label, out_path)
                .map_err(|e| e.to_string())?,
        )),
    }
}

pub fn check_app_read_projection_registry(
    registry_path: &Path,
) -> Result<AppReadProjectionRegistryCheckOutcome, String> {
    let loaded = load_app_read_projection_registry(registry_path)?;
    validate_app_read_projection_schema_files(registry_path, &loaded)?;
    let source_label = registry_path.to_string_lossy();
    let platforms = [
        ReadProjectionPlatform::SwiftTypedDecoders,
        ReadProjectionPlatform::SwiftProjectionCache,
        ReadProjectionPlatform::KotlinProjectionCache,
    ];
    let outputs = platforms
        .into_iter()
        .map(|platform| {
            let out_path = resolve_registry_output(registry_path, loaded.output_for(platform));
            let outcome =
                check_read_projections_from_registry(platform, &loaded, &source_label, &out_path)?;
            Ok(AppReadProjectionOutputCheck {
                platform,
                out_path,
                outcome,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(AppReadProjectionRegistryCheckOutcome {
        schema_count: loaded.schemas.len(),
        outputs,
    })
}

fn validate_projection_row(row: &ProjectionRow) -> Result<(), String> {
    if row.key.trim().is_empty() {
        return Err("projection key must not be empty".to_string());
    }
    if SNAPSHOT_PROJECTIONS
        .iter()
        .any(|entry| entry.key == row.key)
    {
        return Err(format!(
            "app-local projection key {:?} collides with a built-in NMP projection key",
            row.key
        ));
    }
    if row.schema.schema_path.as_os_str().is_empty()
        || row.schema.root_type.trim().is_empty()
        || row.schema.schema_id.trim().is_empty()
    {
        return Err(format!(
            "projection {:?} schema_path/root_type/schema_id must not be empty",
            row.key
        ));
    }
    if row.schema.file_identifier.len() != 4 || !row.schema.file_identifier.is_ascii() {
        return Err(format!(
            "projection {:?} file_identifier must be exactly four ASCII bytes",
            row.key
        ));
    }
    if row.schema.schema_version == 0 {
        return Err(format!(
            "projection {:?} schema_version must be non-zero",
            row.key
        ));
    }
    validate_lower_camel("swift.field", &row.swift.field, &row.key)?;
    if row.swift.domain_type.trim().is_empty() || row.swift.reader_type.trim().is_empty() {
        return Err(format!(
            "projection {:?} Swift domain_type/reader_type must not be empty",
            row.key
        ));
    }
    if row.swift.glue_type != "TypedProjectionGlue" {
        return Err(format!(
            "projection {:?} Swift glue_type must be {:?}, got {:?}",
            row.key, "TypedProjectionGlue", row.swift.glue_type
        ));
    }
    if let Some(kotlin) = &row.kotlin {
        if kotlin.domain_type.trim().is_empty() || kotlin.reader_type.trim().is_empty() {
            return Err(format!(
                "projection {:?} Kotlin domain_type/reader_type must not be empty",
                row.key
            ));
        }
    }
    if row.rust.rust_crate.trim().is_empty()
        || row.rust.module.trim().is_empty()
        || row.rust.producer.trim().is_empty()
    {
        return Err(format!(
            "projection {:?} rust owner fields must not be empty",
            row.key
        ));
    }
    Ok(())
}

fn write_generated(out_path: &Path, rendered: String) -> std::io::Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

fn check_rendered(
    out_path: &Path,
    rendered: String,
) -> std::io::Result<AppReadProjectionCheckOutcome> {
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppReadProjectionCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(AppReadProjectionCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    Ok(AppReadProjectionCheckOutcome {
        up_to_date: false,
        first_diff_line: crate::diff_report::first_diff_or_length(&actual, &rendered),
    })
}

fn resolve_registry_output(registry_path: &Path, output: &Path) -> PathBuf {
    if output.is_absolute() {
        return output.to_path_buf();
    }
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(output)
}

fn swift_cache_outcome(outcome: ProjectionCacheCheckOutcome) -> AppReadProjectionCheckOutcome {
    AppReadProjectionCheckOutcome {
        up_to_date: outcome.up_to_date,
        first_diff_line: outcome.first_diff_line,
    }
}

fn kotlin_cache_outcome(
    outcome: KotlinProjectionCacheCheckOutcome,
) -> AppReadProjectionCheckOutcome {
    AppReadProjectionCheckOutcome {
        up_to_date: outcome.up_to_date,
        first_diff_line: outcome.first_diff_line,
    }
}

fn validate_lower_camel(kind: &str, value: &str, key: &str) -> Result<(), String> {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(format!("projection {key:?} {kind} must not be empty"));
    };
    if !first.is_ascii_lowercase() || !chars.all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!(
            "projection {key:?} {kind} {value:?} must be lowerCamelCase ASCII"
        ));
    }
    Ok(())
}

fn leak_str(value: String) -> &'static str {
    Box::leak(value.into_boxed_str())
}

#[cfg(test)]
#[path = "read_projections/tests.rs"]
mod tests;
