//! App-owned concept-read facade codegen (#2899).
//!
//! This generator stamps concept-read bridge calls into each app facade crate.
//! It mirrors the app-local action-builder generator shape: an app registry
//! names exactly the concepts the app composes, then codegen writes checked-in
//! app-owned Rust facade methods. `nmp-codegen` never imports concept crates;
//! it writes their symbol names into the app crate that already depends on
//! them.

use std::path::{Path, PathBuf};

mod app_registry;
mod app_registry_format;
pub mod kotlin;
pub mod registry;
pub mod rust;
pub mod swift;
// #2899 Part D — fail-closed wire-identity drift gate. Reads each concept
// crate's summary.rs consts and asserts they agree with both this gate's
// hardcoded expectations and the CONCEPT_READS registry, so a schema bump can
// never silently desync the codegen table from the concept-crate source.
pub mod wire_identity_gate;

pub use app_registry::{
    load_app_concept_read_registry, parse_app_concept_read_registry, AppConceptRead,
    AppConceptReadOutputs, AppConceptReadSummary, ConceptReadFacade, LoadedAppConceptReadRegistry,
};
pub use registry::{
    concept_read_for, ConceptRead, SummaryOutput, SummaryShape, TargetInput, CONCEPT_READS,
};
pub use wire_identity_gate::{check_all_wire_identities, WireIdentityCheckOutcome};

/// Which host language to emit for concept-read facades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Emit the Rust app-owned UniFFI facade slice.
    Rust,
    /// Emit Swift host wrappers for generated facade methods.
    Swift,
    /// Emit Kotlin host wrappers for generated facade methods.
    Kotlin,
}

impl Platform {
    /// Parse the `--platform` argument.
    ///
    /// # Errors
    /// An unrecognised platform string.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "rust" => Ok(Self::Rust),
            "swift" => Ok(Self::Swift),
            "kotlin" => Ok(Self::Kotlin),
            other => Err(format!(
                "unknown --platform `{other}` (expected rust|swift|kotlin)"
            )),
        }
    }
}

/// Outcome of a concept-read generated-output drift check.
#[derive(Debug)]
pub struct ConceptReadsCheckOutcome {
    /// `true` when the on-disk file matches freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when not up-to-date.
    pub first_diff_line: Option<usize>,
}

/// One output checked by [`check_app_concept_read_registry`].
#[derive(Debug)]
pub struct AppConceptReadOutputCheck {
    /// Host language checked.
    pub platform: Platform,
    /// Output path resolved relative to the registry file.
    pub out_path: PathBuf,
    /// Diff result for this output.
    pub outcome: ConceptReadsCheckOutcome,
}

/// Outcome of the app-local concept-read registry drift gate.
#[derive(Debug)]
pub struct AppConceptReadRegistryCheckOutcome {
    /// Number of concept-read doors declared.
    pub read_count: usize,
    /// Generated outputs checked against the registry.
    pub outputs: Vec<AppConceptReadOutputCheck>,
}

impl AppConceptReadRegistryCheckOutcome {
    /// `true` when every generated output is up to date.
    #[must_use]
    pub fn up_to_date(&self) -> bool {
        self.outputs.iter().all(|output| output.outcome.up_to_date)
    }
}

/// Render generated concept-read code for `platform` from an explicit registry.
#[must_use]
pub fn render_from_registry(platform: Platform, registry: &LoadedAppConceptReadRegistry) -> String {
    match platform {
        Platform::Rust => rust::render_registry(registry),
        Platform::Swift => swift::render_registry(registry),
        Platform::Kotlin => kotlin::render_registry(registry),
    }
}

/// Write generated concept-read code for an explicit registry to `out_path`.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_concept_reads_from_registry(
    platform: Platform,
    registry: &LoadedAppConceptReadRegistry,
    out_path: &Path,
) -> std::io::Result<()> {
    let rendered = render_from_registry(platform, registry);
    write_rendered(out_path, rendered)
}

fn write_rendered(out_path: &Path, rendered: String) -> std::io::Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)
}

/// Diff an explicit-registry generated output against the file at `out_path`.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_concept_reads_from_registry(
    platform: Platform,
    registry: &LoadedAppConceptReadRegistry,
    out_path: &Path,
) -> std::io::Result<ConceptReadsCheckOutcome> {
    let rendered = render_from_registry(platform, registry);
    check_rendered(out_path, rendered)
}

/// Validate an app-local concept-read registry and check its generated output.
///
/// # Errors
/// Invalid registry JSON or filesystem I/O failures while reading generated
/// outputs.
pub fn check_app_concept_read_registry(
    registry_path: &Path,
) -> Result<AppConceptReadRegistryCheckOutcome, String> {
    let loaded = load_app_concept_read_registry(registry_path)?;
    let mut outputs = Vec::new();
    let rust_path = resolve_registry_path(registry_path, &loaded.outputs.rust);
    outputs.push(check_registry_output(Platform::Rust, &loaded, &rust_path)?);
    if let Some(swift) = loaded.outputs.swift.as_deref() {
        let swift_path = resolve_registry_path(registry_path, swift);
        outputs.push(check_registry_output(
            Platform::Swift,
            &loaded,
            &swift_path,
        )?);
    }
    if let Some(kotlin) = loaded.outputs.kotlin.as_deref() {
        let kotlin_path = resolve_registry_path(registry_path, kotlin);
        outputs.push(check_registry_output(
            Platform::Kotlin,
            &loaded,
            &kotlin_path,
        )?);
    }
    Ok(AppConceptReadRegistryCheckOutcome {
        read_count: loaded.reads.len(),
        outputs,
    })
}

fn check_registry_output(
    platform: Platform,
    loaded: &LoadedAppConceptReadRegistry,
    out_path: &Path,
) -> Result<AppConceptReadOutputCheck, String> {
    let outcome = check_concept_reads_from_registry(platform, loaded, out_path)
        .map_err(|e| format!("check {}: {e}", out_path.display()))?;
    Ok(AppConceptReadOutputCheck {
        platform,
        out_path: out_path.to_path_buf(),
        outcome,
    })
}

/// Resolve an app-registry output path relative to the registry file.
#[must_use]
pub fn resolve_registry_path(registry_path: &Path, output: &Path) -> PathBuf {
    if output.is_absolute() {
        return output.to_path_buf();
    }
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(output)
}

fn check_rendered(out_path: &Path, rendered: String) -> std::io::Result<ConceptReadsCheckOutcome> {
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ConceptReadsCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(ConceptReadsCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(ConceptReadsCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}
