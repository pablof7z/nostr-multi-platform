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

pub mod registry;
// M14-1c / #2169 — the `nmp.marmot` union-builder registry slice, split out of
// `registry.rs` for the 500-LOC ceiling (V-12). `registry` re-exports it via
// `pub use super::registry_marmot::*;` so the flat `registry::` surface is kept.
mod registry_marmot;

pub mod kotlin;
mod kotlin_bookmark_set;
// NIP-51 bookmark helpers split out of `kotlin.rs` for 500-LOC cap compliance.
mod kotlin_nip51;
pub mod kotlin_marmot;
pub mod kotlin_publish;
pub mod swift;
mod swift_bookmark_set;
// NIP-51 bookmark helpers split out of `swift.rs` for 500-LOC cap compliance.
mod swift_nip51;
pub mod swift_marmot;
pub mod swift_publish;
pub mod ts;
// NIP-51 bookmark helpers split out of `ts.rs` for 500-LOC cap compliance.
mod ts_nip51;
pub mod ts_marmot;
pub mod ts_publish;

pub use registry::{
    ActionBuilder, FieldKind, MarmotBodyShape, MarmotBuilder, PayloadField, ACTION_BUILDERS,
    MARMOT_BUILDERS, MARMOT_NAMESPACE,
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
