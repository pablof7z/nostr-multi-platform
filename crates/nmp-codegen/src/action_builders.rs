//! ADR-0064 §3 (#1783) — generated typed action-builder codegen (Swift + Kotlin).
//!
//! Emits the app-facing typed write builders that let the iOS (Swift) and
//! Android (Kotlin) shells construct the `DispatchEnvelope` bytes for the native
//! byte doorway `nmp_app_dispatch_action_bytes` (#1752) from TYPED inputs —
//! `client.react(eventId:reaction:)`, `client.follow(pubkey:)`, … — without ever
//! spelling an `action_namespace` string or hand-assembling FlatBuffers.
//!
//! ## Structure (mirrors the projection-cache / typed-decoder codegen)
//!
//! - [`registry`] — the single source of truth: one [`ActionBuilder`] per write
//!   namespace + its FlatBuffers payload field schema.
//! - [`swift`] / [`kotlin`] — the per-platform emitters; byte-deterministic so
//!   the `--check` drift gate can lock the output.
//!
//! The generated builders are checked into the iOS + Android bridge dirs and
//! gated by `.github/workflows/codegen-drift.yml` (the same job that gates the
//! Swift `Decodable` mirrors + typed decoders), so they can NEVER silently drift
//! from the registered modules.

use std::path::Path;

pub mod registry;

pub mod kotlin;
pub mod kotlin_publish;
pub mod swift;
pub mod swift_publish;

pub use registry::{ActionBuilder, FieldKind, PayloadField, ACTION_BUILDERS};

/// Which host language to emit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Emit `ActionBuilders.generated.swift`.
    Swift,
    /// Emit `ActionBuilders.kt`.
    Kotlin,
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
            other => Err(format!(
                "unknown --platform `{other}` (expected swift|kotlin)"
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

/// Write the generated builder source to `out_path`, replacing whatever was
/// there.
///
/// # Errors
/// Filesystem I/O failures.
pub fn generate_action_builders(platform: Platform, out_path: &Path) -> std::io::Result<()> {
    let rendered = render(platform);
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
