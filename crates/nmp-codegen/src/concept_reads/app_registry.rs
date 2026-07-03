//! App-local concept-read registry input (#2899).
//!
//! The registry lists only reads an app actually composes. `nmp-codegen` uses
//! the list to stamp app-owned UniFFI facade methods; it does not centralize a
//! shared binding namespace or take dependencies on concept crates.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::app_registry_format::RegistryDocument;
use super::registry::{concept_read_for, ConceptRead};

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

/// App facade facts used by the Rust emitter.
#[derive(Debug)]
pub struct ConceptReadFacade {
    /// Rust facade type, e.g. `GalleryApp`.
    pub rust_type: String,
    /// Crate-visible method returning a `ReadHost`, e.g. `runtime`.
    pub runtime_accessor: String,
    /// Facade-local UniFFI error enum, e.g. `GalleryReadError`.
    pub error_type: String,
    /// Error variant for malformed target input.
    pub invalid_target_variant: String,
    /// Error variant for open/read-plan rejection after target decoding.
    pub open_failed_variant: String,
}

/// One app-selected concept read.
#[derive(Debug)]
pub struct AppConceptRead {
    /// Default concept row selected by `concept`.
    pub concept: &'static ConceptRead,
    /// Facade-local opened-handle record type.
    pub opened_record: String,
}

/// App-declared generated output paths.
#[derive(Debug)]
pub struct AppConceptReadOutputs {
    /// Rust generated facade slice path.
    pub rust: PathBuf,
    /// Optional test module path to keep existing in-crate tests attached to the
    /// generated file.
    pub rust_test_module: Option<String>,
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
    validate_lower_ident("facade.runtime_accessor", &doc.facade.runtime_accessor)?;
    validate_upper_ident("facade.error_type", &doc.facade.error_type)?;
    validate_upper_ident(
        "facade.invalid_target_variant",
        &doc.facade.invalid_target_variant,
    )?;
    validate_upper_ident(
        "facade.open_failed_variant",
        &doc.facade.open_failed_variant,
    )?;

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
        reads.push(AppConceptRead {
            concept,
            opened_record: row.opened_record,
        });
    }

    Ok(LoadedAppConceptReadRegistry {
        facade: ConceptReadFacade {
            rust_type: doc.facade.rust_type,
            runtime_accessor: doc.facade.runtime_accessor,
            error_type: doc.facade.error_type,
            invalid_target_variant: doc.facade.invalid_target_variant,
            open_failed_variant: doc.facade.open_failed_variant,
        },
        reads,
        outputs: AppConceptReadOutputs {
            rust: doc.outputs.rust,
            rust_test_module: doc.outputs.rust_test_module,
        },
        drift_checks: doc.drift_checks,
    })
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

#[cfg(test)]
#[path = "app_registry_tests.rs"]
mod tests;
