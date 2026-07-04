//! App-local concept-read registry input (#2899).
//!
//! The registry lists only reads an app actually composes. `nmp-codegen` uses
//! the list to stamp app-owned UniFFI facade methods; it does not centralize a
//! shared binding namespace or take dependencies on concept crates.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::app_registry_format::RegistryDocument;
use super::registry::{concept_read_for, ConceptRead, SummaryShape};

const SCHEMA: &str = "nmp.concept-reads/1";

/// Parsed app-local concept-read registry.
#[derive(Debug)]
pub struct LoadedAppConceptReadRegistry {
    /// App facade object type that receives the generated `#[uniffi::export]`
    /// impl block.
    pub facade: ConceptReadFacade,
    /// Concept read rows, in app declaration order.
    pub reads: Vec<AppConceptRead>,
    /// Generated output paths.
    pub outputs: AppConceptReadOutputs,
    /// Declared drift-check commands, kept as durable app-local documentation.
    pub drift_checks: Vec<String>,
}

/// Shape of the facade's runtime accessor, i.e. how the generated `open_*` /
/// `close_*` methods reach the read host that the concept doors marshal into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeAccessorShape {
    /// `self.<accessor>()` returns a plain read-host reference the facade holds
    /// for its whole lifetime (iOS `ChirpApp` owning `Arc<NmpApp>`). The
    /// concept door is called directly: `open_fn(self.<accessor>(), target)`.
    Ref,
    /// `self.<accessor>(|app| { ... }) -> Option<R>` guards the read host behind
    /// a closure-scoped accessor (Android `AppHandle::with_app`, which holds a
    /// lock for the call's duration to prevent a UAF race with `nmp_app_free`).
    /// The concept door is called inside the closure — `self.<accessor>(|app|
    /// open_fn(app, target))` — and a `None` return (dead/inert handle) maps to
    /// the facade's open-failed error.
    Closure,
}

/// App facade facts used by the Rust emitter.
#[derive(Debug)]
pub struct ConceptReadFacade {
    /// Rust facade type, e.g. `GalleryApp`.
    pub rust_type: String,
    /// Module path the facade type lives in, e.g. `facade` or `app`.
    pub rust_module: String,
    /// Crate-visible accessor reaching the `ReadHost`, e.g. `runtime` (ref
    /// shape) or `with_app` (closure shape). Its call form is selected by
    /// [`ConceptReadFacade::runtime_accessor_shape`].
    pub runtime_accessor: String,
    /// Whether `runtime_accessor` is a plain ref accessor or a closure-guarded
    /// one. Defaults to [`RuntimeAccessorShape::Ref`].
    pub runtime_accessor_shape: RuntimeAccessorShape,
    /// Facade-local UniFFI error enum, e.g. `GalleryReadError`.
    pub error_type: String,
    /// Error variant for malformed target input.
    pub invalid_target_variant: String,
    /// Error variant for open/read-plan rejection after target decoding.
    pub open_failed_variant: String,
    /// Error variant for invalid typed summary payloads.
    pub decode_failed_variant: String,
}

/// One app-selected concept read.
#[derive(Debug)]
pub struct AppConceptRead {
    /// Default concept row selected by `concept`.
    pub concept: &'static ConceptRead,
    /// Facade-local opened-handle record type.
    pub opened_record: String,
    /// Facade-local typed summary output record names.
    pub summary: AppConceptReadSummary,
}

/// App-selected record names for one concept-read summary.
#[derive(Debug)]
pub struct AppConceptReadSummary {
    /// Facade-local summary record type.
    pub record: String,
    /// Facade-local reaction group record type.
    pub group_record: Option<String>,
    /// Facade-local zapper total record type.
    pub zapper_record: Option<String>,
}

/// App-declared generated output paths.
#[derive(Debug)]
pub struct AppConceptReadOutputs {
    /// Rust generated facade slice path.
    pub rust: PathBuf,
    /// Optional test module path to keep existing in-crate tests attached to the
    /// generated file.
    pub rust_test_module: Option<String>,
    /// Optional Swift wrapper output path.
    pub swift: Option<PathBuf>,
    /// Optional Kotlin wrapper output path.
    pub kotlin: Option<PathBuf>,
    /// Kotlin package for the wrapper output.
    pub kotlin_package: Option<String>,
    /// UniFFI package containing the generated app facade Kotlin bindings.
    pub kotlin_uniffi_package: Option<String>,
}

/// Load and parse an app-local concept-read registry JSON file.
///
/// # Errors
/// Filesystem failures, invalid JSON, unknown concepts, duplicate concepts, or
/// invalid Rust identifiers.
pub fn load_app_concept_read_registry(path: &Path) -> Result<LoadedAppConceptReadRegistry, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read app concept-read registry {}: {e}", path.display()))?;
    parse_app_concept_read_registry(&raw)
}

/// Parse an app-local concept-read registry JSON document.
///
/// # Errors
/// Invalid JSON, unknown concepts, duplicate concepts, or invalid Rust
/// identifiers.
pub fn parse_app_concept_read_registry(raw: &str) -> Result<LoadedAppConceptReadRegistry, String> {
    let doc: RegistryDocument = serde_json::from_str(raw)
        .map_err(|e| format!("parse app concept-read registry JSON: {e}"))?;
    if doc.schema != SCHEMA {
        return Err(format!(
            "app concept-read registry schema must be {SCHEMA:?}, got {:?}",
            doc.schema
        ));
    }
    if doc.reads.is_empty() {
        return Err("app concept-read registry must declare at least one read".to_string());
    }
    if doc.drift_checks.is_empty() {
        return Err("app concept-read registry must declare drift_checks".to_string());
    }
    validate_upper_ident("facade.rust_type", &doc.facade.rust_type)?;
    validate_module_path("facade.rust_module", &doc.facade.rust_module)?;
    validate_lower_ident("facade.runtime_accessor", &doc.facade.runtime_accessor)?;
    let runtime_accessor_shape =
        parse_runtime_accessor_shape(doc.facade.runtime_accessor_shape.as_deref())?;
    validate_upper_ident("facade.error_type", &doc.facade.error_type)?;
    validate_upper_ident(
        "facade.invalid_target_variant",
        &doc.facade.invalid_target_variant,
    )?;
    validate_upper_ident(
        "facade.open_failed_variant",
        &doc.facade.open_failed_variant,
    )?;
    validate_upper_ident(
        "facade.decode_failed_variant",
        &doc.facade.decode_failed_variant,
    )?;
    validate_kotlin_outputs(&doc.outputs)?;

    let mut seen = BTreeSet::new();
    let mut records = BTreeSet::new();
    let mut reads = Vec::with_capacity(doc.reads.len());
    for row in doc.reads {
        if !seen.insert(row.concept.clone()) {
            return Err(format!("duplicate concept read {:?}", row.concept));
        }
        let concept = concept_read_for(&row.concept)
            .ok_or_else(|| format!("unknown concept read {:?}", row.concept))?;
        validate_upper_ident("reads[].opened_record", &row.opened_record)?;
        if !records.insert(row.opened_record.clone()) {
            return Err(format!("duplicate opened_record {:?}", row.opened_record));
        }
        validate_upper_ident("reads[].summary.record", &row.summary.record)?;
        if !records.insert(row.summary.record.clone()) {
            return Err(format!("duplicate summary record {:?}", row.summary.record));
        }
        validate_summary_nested_records(concept, &row.summary, &mut records)?;
        reads.push(AppConceptRead {
            concept,
            opened_record: row.opened_record,
            summary: AppConceptReadSummary {
                record: row.summary.record,
                group_record: row.summary.group_record,
                zapper_record: row.summary.zapper_record,
            },
        });
    }

    Ok(LoadedAppConceptReadRegistry {
        facade: ConceptReadFacade {
            rust_type: doc.facade.rust_type,
            rust_module: doc.facade.rust_module,
            runtime_accessor: doc.facade.runtime_accessor,
            runtime_accessor_shape,
            error_type: doc.facade.error_type,
            invalid_target_variant: doc.facade.invalid_target_variant,
            open_failed_variant: doc.facade.open_failed_variant,
            decode_failed_variant: doc.facade.decode_failed_variant,
        },
        reads,
        outputs: AppConceptReadOutputs {
            rust: doc.outputs.rust,
            rust_test_module: doc.outputs.rust_test_module,
            swift: doc.outputs.swift,
            kotlin: doc.outputs.kotlin,
            kotlin_package: doc.outputs.kotlin_package,
            kotlin_uniffi_package: doc.outputs.kotlin_uniffi_package,
        },
        drift_checks: doc.drift_checks,
    })
}

fn parse_runtime_accessor_shape(raw: Option<&str>) -> Result<RuntimeAccessorShape, String> {
    match raw {
        None | Some("ref") => Ok(RuntimeAccessorShape::Ref),
        Some("closure") => Ok(RuntimeAccessorShape::Closure),
        Some(other) => Err(format!(
            "facade.runtime_accessor_shape must be \"ref\" or \"closure\", got {other:?}"
        )),
    }
}

fn validate_summary_nested_records(
    concept: &ConceptRead,
    summary: &super::app_registry_format::SummaryRow,
    records: &mut BTreeSet<String>,
) -> Result<(), String> {
    match concept.summary.shape {
        SummaryShape::Reaction => {
            let group_record = summary.group_record.as_deref().ok_or_else(|| {
                format!(
                    "concept read {:?} requires summary.group_record",
                    concept.id
                )
            })?;
            validate_upper_ident("reads[].summary.group_record", group_record)?;
            if !records.insert(group_record.to_string()) {
                return Err(format!("duplicate summary group_record {group_record:?}"));
            }
            if summary.zapper_record.is_some() {
                return Err(format!(
                    "concept read {:?} must not declare summary.zapper_record",
                    concept.id
                ));
            }
        }
        SummaryShape::Zap => {
            let zapper_record = summary.zapper_record.as_deref().ok_or_else(|| {
                format!(
                    "concept read {:?} requires summary.zapper_record",
                    concept.id
                )
            })?;
            validate_upper_ident("reads[].summary.zapper_record", zapper_record)?;
            if !records.insert(zapper_record.to_string()) {
                return Err(format!("duplicate summary zapper_record {zapper_record:?}"));
            }
            if summary.group_record.is_some() {
                return Err(format!(
                    "concept read {:?} must not declare summary.group_record",
                    concept.id
                ));
            }
        }
        SummaryShape::Reply | SummaryShape::Repost => {
            if summary.group_record.is_some() || summary.zapper_record.is_some() {
                return Err(format!(
                    "concept read {:?} does not use nested summary records",
                    concept.id
                ));
            }
        }
    }
    Ok(())
}

fn validate_kotlin_outputs(outputs: &super::app_registry_format::OutputsRow) -> Result<(), String> {
    let has_kotlin = outputs.kotlin.is_some();
    if has_kotlin != outputs.kotlin_package.is_some()
        || has_kotlin != outputs.kotlin_uniffi_package.is_some()
    {
        return Err(
            "outputs.kotlin requires outputs.kotlin_package and outputs.kotlin_uniffi_package"
                .to_string(),
        );
    }
    if let Some(package) = outputs.kotlin_package.as_deref() {
        validate_dotted_ident("outputs.kotlin_package", package)?;
    }
    if let Some(package) = outputs.kotlin_uniffi_package.as_deref() {
        validate_dotted_ident("outputs.kotlin_uniffi_package", package)?;
    }
    Ok(())
}

fn validate_lower_ident(field: &str, value: &str) -> Result<(), String> {
    validate_ident(field, value)?;
    let first = value.as_bytes()[0];
    if !first.is_ascii_lowercase() {
        return Err(format!("{field} must start with [a-z], got {value:?}"));
    }
    Ok(())
}

fn validate_upper_ident(field: &str, value: &str) -> Result<(), String> {
    validate_ident(field, value)?;
    let first = value.as_bytes()[0];
    if !first.is_ascii_uppercase() {
        return Err(format!("{field} must start with [A-Z], got {value:?}"));
    }
    Ok(())
}

fn validate_ident(field: &str, value: &str) -> Result<(), String> {
    let mut chars = value.as_bytes().iter();
    let Some(first) = chars.next() else {
        return Err(format!("{field} must not be empty"));
    };
    if !(first.is_ascii_alphabetic() || *first == b'_') {
        return Err(format!("{field} must be a Rust identifier, got {value:?}"));
    }
    if chars.any(|b| !(b.is_ascii_alphanumeric() || *b == b'_')) {
        return Err(format!("{field} must be a Rust identifier, got {value:?}"));
    }
    Ok(())
}

fn validate_module_path(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    for segment in value.split("::") {
        validate_lower_ident(field, segment)?;
    }
    Ok(())
}

fn validate_dotted_ident(field: &str, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    for part in value.split('.') {
        validate_lower_ident(field, part)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "app_registry_tests.rs"]
mod tests;
