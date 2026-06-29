//! ADR-0064 §3 (#1783 / #1776) — generated typed action-builder codegen
//! (Swift + Kotlin + TypeScript).
//!
//! Emits the app-facing typed write builders that let the iOS (Swift), Android
//! (Kotlin), and Web (TypeScript) shells construct the `DispatchEnvelope` bytes
//! for the byte doorway (`nmp_app_dispatch_action_bytes` natively, #1752; the
//! `dispatch_bytes` wasm seam on web, #1750) from TYPED inputs —
//! `client.react(eventId:reaction:)`, `client.follow(pubkey:)`, … — without ever
//! spelling an `action_namespace` string or hand-assembling FlatBuffers.
//!
//! ## Structure (mirrors the projection-cache / typed-decoder codegen)
//!
//! - [`crate::action_contract`] — the neutral identity contract for each
//!   default typed action namespace/schema/file-id/tier.
//! - [`registry`] — builder-specific host API shape: one [`ActionBuilder`] per
//!   generated flat-table method plus payload field order.
//! - [`swift`] / [`kotlin`] / [`ts`] — the per-platform emitters;
//!   byte-deterministic so the `--check` drift gate can lock the output.
//!
//! The generated builders are checked into the iOS + Android bridge dirs and the
//! web `runtime-web` package, gated by `.github/workflows/codegen-drift.yml` (the
//! same job that gates the Swift `Decodable` mirrors + typed decoders), so they
//! can NEVER silently drift from the registered modules.
//!
//! ## TypeScript: same wire, slot-indexed FlatBuffers API
//!
//! The TS emitter ([`ts`] / [`ts_publish`]) targets the `flatbuffers` npm
//! runtime's low-level `Builder` API (`startObject`/`addFieldOffset`/
//! `addFieldInt32`/`endObject`), which is slot-indexed exactly like Kotlin's
//! `FlatBufferBuilder` (not vtable-byte-offset like Swift). It reuses the
//! hand-written `encodeDispatchEnvelope` already shipped in
//! `web/packages/runtime-web/src/dispatchEnvelope.ts` (Cut A #1809) rather than
//! re-emitting an envelope helper, so the envelope wrapper has a single web
//! source of truth.

use std::path::Path;

mod app_registry;
mod app_registry_format;
mod app_registry_schema;
pub mod registry;
mod registry_input;
// M14-1c / #2169 — the `nmp.marmot` union-builder registry slice, split out of
// `registry.rs` for the 500-LOC ceiling (V-12). `registry` re-exports it via
// `pub use super::registry_marmot::*;` so the flat `registry::` surface is kept.
mod registry_marmot;

pub mod kotlin;
mod kotlin_bookmark_set;
// NIP-51 bookmark helpers split out of `kotlin.rs` for 500-LOC cap compliance.
pub mod kotlin_marmot;
mod kotlin_nip51;
pub mod kotlin_publish;
pub mod swift;
mod swift_bookmark_set;
// NIP-51 bookmark helpers split out of `swift.rs` for 500-LOC cap compliance.
pub mod swift_marmot;
mod swift_nip51;
pub mod swift_publish;
pub mod ts;
// NIP-51 bookmark helpers split out of `ts.rs` for 500-LOC cap compliance.
pub mod ts_marmot;
mod ts_nip51;
pub mod ts_publish;

pub use app_registry::{
    load_app_action_builder_registry, parse_app_action_builder_registry, AppActionBuilderOutputs,
    LoadedAppActionBuilderRegistry,
};
pub use app_registry_schema::{validate_app_action_builder_schema_files, AppActionBuilderSchema};
pub use registry::{
    ActionBuilder, FieldKind, MarmotBodyShape, MarmotBuilder, PayloadField, ACTION_BUILDERS,
    MARMOT_BUILDERS, MARMOT_NAMESPACE,
};
pub use registry_input::{
    ActionBuilderRegistry, ActionBuilderWireContract, AppActionBuilderWireContract,
};

/// Which host language to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Emit `ActionBuilders.generated.swift`.
    Swift,
    /// Emit `ActionBuilders.kt`.
    Kotlin,
    /// Emit `actionBuilders.generated.ts`.
    Ts,
}

impl Platform {
    /// Parse the `--platform` argument.
    ///
    /// # Errors
    /// An unrecognised platform string.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "swift" => Ok(Self::Swift),
            "kotlin" => Ok(Self::Kotlin),
            "ts" => Ok(Self::Ts),
            other => Err(format!(
                "unknown --platform `{other}` (expected swift|kotlin|ts)"
            )),
        }
    }
}

/// Render the generated action-builders for `platform` from the default
/// [`ACTION_BUILDERS`] registry.
#[must_use]
pub fn render(platform: Platform) -> String {
    match platform {
        Platform::Swift => swift::render_default(),
        Platform::Kotlin => kotlin::render_default(),
        Platform::Ts => ts::render_default(),
    }
}

/// Render generated action-builders for `platform` from an explicit registry.
#[must_use]
pub fn render_from_registry(platform: Platform, registry: &ActionBuilderRegistry<'_>) -> String {
    match platform {
        Platform::Swift => swift::render_registry(registry),
        Platform::Kotlin => kotlin::render_registry(registry),
        Platform::Ts => ts::render_registry(registry),
    }
}

/// Outcome of a `--check` run. Mirrors the other codegen check outcomes.
#[derive(Debug)]
pub struct ActionBuildersCheckOutcome {
    /// `true` when the on-disk file matches the freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when not up-to-date; `None` when
    /// up-to-date OR when the file doesn't exist.
    pub first_diff_line: Option<usize>,
}

/// One output checked by [`check_app_action_builder_registry`].
#[derive(Debug)]
pub struct AppActionBuilderOutputCheck {
    /// Host language checked.
    pub platform: Platform,
    /// Output path resolved relative to the registry file.
    pub out_path: std::path::PathBuf,
    /// Diff result for this output.
    pub outcome: ActionBuildersCheckOutcome,
}

/// Outcome of the app-local registry drift gate.
#[derive(Debug)]
pub struct AppActionBuilderRegistryCheckOutcome {
    /// Number of schema files validated.
    pub schema_count: usize,
    /// Generated outputs checked against the registry.
    pub outputs: Vec<AppActionBuilderOutputCheck>,
}

impl AppActionBuilderRegistryCheckOutcome {
    /// `true` when every generated output is up to date.
    #[must_use]
    pub fn up_to_date(&self) -> bool {
        self.outputs.iter().all(|output| output.outcome.up_to_date)
    }
}

/// Write the generated builder source to `out_path`, replacing whatever was
/// there.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_action_builders(platform: Platform, out_path: &Path) -> std::io::Result<()> {
    let rendered = render(platform);
    write_rendered(out_path, rendered)
}

/// Write generated builder source for an explicit registry to `out_path`.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_action_builders_from_registry(
    platform: Platform,
    registry: &ActionBuilderRegistry<'_>,
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

/// Diff a freshly-rendered output against the file at `out_path`. A missing file
/// is reported as stale (`up_to_date = false`), matching the other gates'
/// treatment.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_action_builders(
    platform: Platform,
    out_path: &Path,
) -> std::io::Result<ActionBuildersCheckOutcome> {
    let rendered = render(platform);
    check_rendered(out_path, rendered)
}

/// Diff an explicit-registry generated output against the file at `out_path`.
///
/// # Errors
/// Filesystem I/O failures other than NotFound.
pub fn check_action_builders_from_registry(
    platform: Platform,
    registry: &ActionBuilderRegistry<'_>,
    out_path: &Path,
) -> std::io::Result<ActionBuildersCheckOutcome> {
    let rendered = render_from_registry(platform, registry);
    check_rendered(out_path, rendered)
}

/// Validate an app-local action-builder registry and check all generated outputs
/// declared by that registry.
///
/// # Errors
/// Invalid registry JSON, schema-file contract mismatches, or filesystem I/O
/// failures while reading generated outputs.
pub fn check_app_action_builder_registry(
    registry_path: &Path,
) -> Result<AppActionBuilderRegistryCheckOutcome, String> {
    let loaded = load_app_action_builder_registry(registry_path)?;
    validate_app_action_builder_schema_files(registry_path, &loaded)?;
    let registry = loaded.as_registry();
    let mut outputs = Vec::new();
    for platform in [Platform::Swift, Platform::Kotlin, Platform::Ts] {
        let out_path = resolve_registry_path(registry_path, loaded.output_for(platform));
        let outcome = check_action_builders_from_registry(platform, &registry, &out_path)
            .map_err(|e| format!("check {}: {e}", out_path.display()))?;
        outputs.push(AppActionBuilderOutputCheck {
            platform,
            out_path,
            outcome,
        });
    }
    Ok(AppActionBuilderRegistryCheckOutcome {
        schema_count: loaded.schemas.len(),
        outputs,
    })
}

fn resolve_registry_path(registry_path: &Path, output: &Path) -> std::path::PathBuf {
    if output.is_absolute() {
        return output.to_path_buf();
    }
    registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(output)
}

fn check_rendered(
    out_path: &Path,
    rendered: String,
) -> std::io::Result<ActionBuildersCheckOutcome> {
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ActionBuildersCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(err),
    };
    if actual == rendered {
        return Ok(ActionBuildersCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = crate::diff_report::first_diff_or_length(&actual, &rendered);
    Ok(ActionBuildersCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
#[path = "action_builders/tests.rs"]
mod tests;
