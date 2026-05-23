//! V6 Stage 1 — Swift `Decodable` emitter.
//!
//! Reads a `ProjectionSchemaDocument` (the JSON the `dump_projection_schemas`
//! binary writes) and renders Swift `struct` declarations conforming to
//! `Decodable` (plus Equatable / Identifiable when registry metadata asks).
//!
//! Stage 1 covers flat-record types only — every pilot schema decodes as a
//! JSON Schema `object` with scalar / nullable-scalar / array-of-scalar
//! properties. Tagged enums (`ActionStage`, `TimelineBlock`) and the
//! dotted-projection-key registry (`SnapshotProjections`) are Stage 2/3 work
//! and are explicitly out of scope here. Any pilot schema that doesn't match
//! the flat-record shape returns a [`SwiftEmitError::Unsupported`] so the
//! CI gate fails loudly rather than emitting silent wrong-shape Swift.
//!
//! ## Output determinism
//!
//! The emitter is byte-deterministic. Type order matches the input
//! document; field order matches the input schema's `properties` object
//! (which `nmp-core::codegen_schema` sorts alphabetically via schemars).
//! That stability is what makes the `--check` CI gate possible — running
//! the emitter twice on the same input produces byte-identical output.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use crate::swift_projections_registry::{SnapshotProjectionEntry, SNAPSHOT_PROJECTIONS};

/// Parsed shape of the document `dump_projection_schemas` writes.
#[derive(Debug, Deserialize)]
struct ProjectionSchemaDocument {
    version: u32,
    types: Vec<TypeEntry>,
}

#[derive(Debug, Deserialize)]
struct TypeEntry {
    rust_path: String,
    swift_name: String,
    #[serde(default)]
    id_field: Option<String>,
    conformances: Vec<String>,
    schema: TypeSchema,
}

/// Subset of JSON Schema (draft-07) the emitter actually decodes. `schemars`
/// produces strictly richer schemas (`$schema`, `description`, `minimum`,
/// `format` for distinguishing `u32`/`u64`); we ignore everything we don't
/// need so future schemars upgrades don't break the decode.
#[derive(Debug, Deserialize)]
struct TypeSchema {
    #[serde(rename = "type", default)]
    ty: Option<serde_json::Value>,
    #[serde(default)]
    title: Option<String>,
    /// Map of field-name → field-schema. `serde_json::Map` with
    /// `preserve_order` feature on keeps insertion order; schemars emits
    /// alphabetically, so the iteration order is deterministic regardless.
    #[serde(default)]
    properties: serde_json::Map<String, serde_json::Value>,
    /// JSON-Schema `required` list — fields not in here are optional.
    #[serde(default)]
    required: Vec<String>,
}

/// What went wrong during Swift emission. Carries enough context that a
/// regression in Stage 1 (Rust type took on a non-flat field shape) names
/// the offending Swift type and Rust path.
///
/// Keeps `nmp-codegen` dependency-free of `thiserror` to match the existing
/// crate posture (every other module uses `String` errors). The hand-rolled
/// `Display` + `Error` impls below give Stage 1 callers `?` propagation
/// without dragging in a new dep tree.
#[derive(Debug)]
pub enum SwiftEmitError {
    /// The input JSON did not decode as a [`ProjectionSchemaDocument`].
    ParseFailed { reason: String },
    /// The schema document version doesn't match the emitter's supported
    /// set. Bump emitter + document together when this trips.
    UnsupportedDocumentVersion { found: u32, expected: u32 },
    /// One pilot type's schema isn't a flat object — Stage 1 deliberately
    /// rejects this so the dotted-key / tagged-enum work goes through
    /// Stage 2 / 3 instead of being silently emitted wrong here.
    Unsupported {
        swift_name: String,
        rust_path: String,
        reason: String,
    },
    /// Filesystem operations behind `generate_swift` / `check_swift`.
    Io(std::io::Error),
}

impl std::fmt::Display for SwiftEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed { reason } => {
                write!(f, "failed to parse projection schema document: {reason}")
            }
            Self::UnsupportedDocumentVersion { found, expected } => write!(
                f,
                "projection schema document version {found} unsupported by this nmp-codegen \
                 build (expected version {expected}). Regenerate by re-running \
                 `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas`."
            ),
            Self::Unsupported {
                swift_name,
                rust_path,
                reason,
            } => write!(
                f,
                "cannot emit Swift for `{swift_name}` ({rust_path}): {reason}. \
                 Stage 1 only supports flat-record schemas; tagged enums and \
                 nested registries are Stage 2/3 scope per \
                 docs/architecture-audit/v6-codegen-plan.md."
            ),
            Self::Io(err) => write!(f, "io: {err}"),
        }
    }
}
impl std::error::Error for SwiftEmitError {}
impl From<std::io::Error> for SwiftEmitError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Stage 1 supports exactly version 1 of the schema document. The Rust
/// side bumps this in lockstep with any change to the document shape.
const SUPPORTED_DOCUMENT_VERSION: u32 = 1;

/// Header comment emitted at the top of every generated file. The
/// regeneration command must stay accurate — CI fails on a stale
/// generated file, so anyone hitting the failure needs the exact
/// command to reproduce the regeneration locally.
const HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Regenerate via:
//   cargo run -p nmp-core --features codegen-schema \\
//       --bin dump_projection_schemas \\
//       | cargo run -p nmp-codegen -- gen swift --stdin --out <path>
//
// Source of truth: the Rust projection types listed in the per-struct
// provenance comments below. The CI gate (`.github/workflows/codegen-drift.yml`)
// fails any PR whose generated Swift differs from a fresh run.
//
// Stage 1 pilot — 7 flat-record types (V6, docs/architecture-audit/
// v6-codegen-plan.md §6b). Stage 2 expands to the dotted-projection-key
// registry; Stage 3 sweeps the remaining hand-written Decodables.
// ─────────────────────────────────────────────────────────────────────────────

import Foundation
";

/// Generate the Swift source for the given schema-document JSON.
///
/// Returns the rendered Swift as a `String`. Caller is responsible for
/// writing it to disk (the indirection lets [`check_swift`] diff against
/// the committed file without going through the filesystem).
///
/// # Errors
/// - [`SwiftEmitError::ParseFailed`] if `document_json` isn't valid
///   `ProjectionSchemaDocument`.
/// - [`SwiftEmitError::UnsupportedDocumentVersion`] if the document version
///   doesn't match this emitter.
/// - [`SwiftEmitError::Unsupported`] if any type has a non-flat-record
///   schema.
#[must_use]
pub fn render_swift(document_json: &str) -> Result<String, SwiftEmitError> {
    let document: ProjectionSchemaDocument = serde_json::from_str(document_json)
        .map_err(|err| SwiftEmitError::ParseFailed {
            reason: err.to_string(),
        })?;

    if document.version != SUPPORTED_DOCUMENT_VERSION {
        return Err(SwiftEmitError::UnsupportedDocumentVersion {
            found: document.version,
            expected: SUPPORTED_DOCUMENT_VERSION,
        });
    }

    let mut out = String::from(HEADER);
    out.push('\n');
    for entry in &document.types {
        render_type(entry, &mut out)?;
        out.push('\n');
    }
    // V6 Stage 2 — append the `SnapshotProjections` registry struct +
    // `CodingKeys` enum. Driven by the static slice in
    // [`crate::swift_projections_registry`] rather than a schemars schema,
    // because the registry is a list of (json_key, swift_field, swift_type)
    // triples — there is no Rust type to reflect (the projection values
    // come from many different crates, including app-layer ones).
    render_snapshot_projections(SNAPSHOT_PROJECTIONS, &mut out);
    Ok(out)
}

/// Render the V6 Stage 2 `SnapshotProjections` struct and its `CodingKeys`
/// enum to `out`, driven by the [`SNAPSHOT_PROJECTIONS`] registry.
///
/// Output shape, drop-in for the hand-written declaration in
/// `ios/Chirp/Chirp/Bridge/KernelBridge.swift`:
///
/// ```swift
/// internal struct SnapshotProjections: Decodable, Equatable {
///     let wallet: WalletStatusData?
///     // ... one line per entry ...
///
///     enum CodingKeys: String, CodingKey {
///         case wallet
///         case bunkerHandshake
///         case groupChat = "nmp.nip29.groupChat"
///         // ... case per entry, raw value only when post-transform key
///         //     differs from the Swift field name ...
///     }
/// }
/// ```
///
/// Visibility is `internal` (no modifier) to match the hand-written
/// declaration's visibility verbatim — the conformance test in
/// `SnapshotProjectionsConformanceTests.swift` accesses the type via
/// `@testable import Chirp`, which exposes `internal` symbols. Bumping to
/// `public` would change the symbol-table surface area unnecessarily.
fn render_snapshot_projections(entries: &[SnapshotProjectionEntry], out: &mut String) {
    out.push_str("// MARK: - SnapshotProjections\n");
    out.push_str("// Source: crates/nmp-codegen/src/swift_projections_registry.rs (Stage 2 registry)\n");
    out.push_str("//\n");
    out.push_str("// The kernel's host-extensible `projections` map. Each entry mirrors one\n");
    out.push_str("// registered snapshot-projection key. Every member is optional so a stale\n");
    out.push_str("// kernel build that predates a projection still decodes (D1 forward-compat).\n");
    out.push_str("//\n");
    out.push_str("// The `CodingKeys` enum below uses post-`.convertFromSnakeCase` raw values\n");
    out.push_str("// (the iOS shell's `KernelHandle.decode` sets that strategy). Cases whose\n");
    out.push_str("// raw value matches the Swift property name carry no explicit literal.\n");
    out.push_str("struct SnapshotProjections: Decodable, Equatable {\n");

    for entry in entries {
        out.push_str(&format!(
            "    let {}: {}?\n",
            entry.swift_field, entry.swift_type
        ));
    }

    out.push('\n');
    out.push_str("    enum CodingKeys: String, CodingKey {\n");
    for entry in entries {
        let post_transform = post_convert_from_snake_case(entry.json_key);
        if post_transform == entry.swift_field {
            out.push_str(&format!("        case {}\n", entry.swift_field));
        } else {
            out.push_str(&format!(
                "        case {} = \"{}\"\n",
                entry.swift_field, post_transform
            ));
        }
    }
    out.push_str("    }\n");
    out.push_str("}\n");
}

/// Apple's `JSONDecoder.KeyDecodingStrategy.convertFromSnakeCase` algorithm.
///
/// The strategy transforms an incoming JSON key BEFORE matching it against
/// any `CodingKey.stringValue`. The transform per Apple's docs:
///
/// 1. Capture all leading underscores (preserved verbatim on the output).
/// 2. Capture all trailing underscores (preserved verbatim on the output).
/// 3. Split the middle on each `_`, lowercase the first run, uppercase the
///    first letter of every subsequent run.
///
/// What the docs leave implicit and what bit the iOS shell historically:
///
/// - **`.` is opaque.** `.convertFromSnakeCase` does NOT split on `.`; it
///   only touches `_`. So `"nmp.nip29.group_chat"` becomes
///   `"nmp.nip29.groupChat"`, NOT `"nmp.Nip29.GroupChat"`. The dot-separated
///   prefix passes through unchanged, and only the `group_chat` tail
///   camelises.
/// - **Single-word inputs are returned unchanged.** `"wallet"` → `"wallet"`,
///   `"profile"` → `"profile"`. Apple's algorithm has nothing to do, so the
///   strategy is a no-op for any key without `_`.
///
/// This implementation handles both observed shapes (`snake_case` and
/// `nmp.<nip>.snake_case`) plus the pure-camel pass-through case. It is
/// NOT a complete reimplementation of Apple's full edge-case set (leading
/// and trailing underscores in particular) — none of the registry keys
/// carry those, and the docstring on
/// [`crate::swift_projections_registry::SnapshotProjectionEntry`] tells the
/// next contributor to validate any new key shape here before adding it.
fn post_convert_from_snake_case(key: &str) -> String {
    // Single-word fast path: no `_`, the strategy returns the input
    // unchanged. Covers `wallet`, `profile`, `timeline`, etc.
    if !key.contains('_') {
        return key.to_string();
    }
    // The `.` is opaque to `.convertFromSnakeCase`. Split on `.` first,
    // transform each segment independently, rejoin. A bare snake_case
    // key (no `.`) hits the same path with a single segment.
    let segments: Vec<String> = key.split('.').map(camelize_snake_segment).collect();
    segments.join(".")
}

/// Transform one `.`-delimited segment of a key (or the whole key when it
/// has no `.`s). Splits on `_`, lowercases the first run, uppercases the
/// first letter of every subsequent run. The implementation matches
/// Apple's reference for the inner `_`-handling step of
/// `.convertFromSnakeCase`.
fn camelize_snake_segment(segment: &str) -> String {
    let mut parts = segment.split('_');
    let mut out = parts.next().unwrap_or("").to_string();
    for part in parts {
        if part.is_empty() {
            // Consecutive `__` is preserved as nothing — Apple's algorithm
            // collapses empty runs. None of the registry keys hit this.
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Render one type into `out`.
fn render_type(entry: &TypeEntry, out: &mut String) -> Result<(), SwiftEmitError> {
    require_flat_object(entry)?;

    // Provenance comment — source-of-truth line per plan §5c.
    out.push_str(&format!(
        "// MARK: - {}\n// Source: {}\n",
        entry.swift_name, entry.rust_path
    ));

    // Conformance clause. `Identifiable` is appended automatically when
    // `id_field` is `Some` so the registry never has to repeat itself.
    let mut conformances: BTreeSet<String> =
        entry.conformances.iter().cloned().collect();
    if entry.id_field.is_some() {
        conformances.insert("Identifiable".to_string());
    }
    let conformances: Vec<&str> = ["Decodable", "Equatable", "Identifiable", "Hashable"]
        .into_iter()
        .filter(|c| conformances.contains(*c))
        .collect();
    let conformances_clause = conformances.join(", ");

    out.push_str(&format!(
        "public struct {}: {} {{\n",
        entry.swift_name, conformances_clause
    ));

    // Identifiable `id` accessor — when `id_field` is set AND the struct
    // doesn't already have a literal `id` field, render the computed
    // property. When the field IS literally named `id`, Swift's
    // synthesised Identifiable conformance picks it up automatically and
    // no extra property is needed (it would be a duplicate-declaration
    // error).
    let required: BTreeSet<&str> = entry.schema.required.iter().map(String::as_str).collect();
    let mut field_decls: Vec<String> = Vec::with_capacity(entry.schema.properties.len());
    for (raw_name, raw_schema) in &entry.schema.properties {
        let swift_field = snake_to_camel(raw_name);
        let is_required = required.contains(raw_name.as_str());
        let swift_type = swift_type_for(raw_schema).ok_or_else(|| SwiftEmitError::Unsupported {
            swift_name: entry.swift_name.clone(),
            rust_path: entry.rust_path.clone(),
            reason: format!(
                "field `{raw_name}` has unsupported schema shape: {raw_schema}"
            ),
        })?;
        let optional_suffix = if is_required { "" } else { "?" };
        field_decls.push(format!(
            "    public let {swift_field}: {swift_type}{optional_suffix}"
        ));
    }
    for decl in &field_decls {
        out.push_str(decl);
        out.push('\n');
    }

    if let Some(id_field) = entry.id_field.as_deref() {
        if id_field != "id" {
            out.push('\n');
            out.push_str(&format!(
                "    public var id: String {{ {id_field} }}\n"
            ));
        }
    }

    // No explicit CodingKeys. `KernelBridge.decode()` configures
    // `JSONDecoder.keyDecodingStrategy = .convertFromSnakeCase`, which
    // transforms wire keys (snake_case) to Swift identifiers (camelCase)
    // before Codable matches them. Emitting `case foo = "foo_bar"` would
    // cause a double-transform failure: the decoder converts "foo_bar" →
    // "fooBar", then looks for a CodingKeys rawValue "fooBar" but finds
    // "foo_bar" → KEY_NOT_FOUND on every field. The synthesised CodingKeys
    // (no explicit rawValues) matches correctly because each case's implicit
    // rawValue equals its Swift identifier, which is exactly what the
    // decoder produces after the convertFromSnakeCase transform.

    out.push_str("}\n");
    Ok(())
}

/// Ensure the entry's schema is a flat object with `properties`. Anything
/// else (a tagged enum's `oneOf`, an array root, a `$ref`) returns
/// `Unsupported`.
fn require_flat_object(entry: &TypeEntry) -> Result<(), SwiftEmitError> {
    let ty = entry.schema.ty.as_ref().ok_or_else(|| SwiftEmitError::Unsupported {
        swift_name: entry.swift_name.clone(),
        rust_path: entry.rust_path.clone(),
        reason: "schema root has no `type` field (likely an enum or $ref)".to_string(),
    })?;
    let is_object = match ty {
        serde_json::Value::String(s) => s == "object",
        _ => false,
    };
    if !is_object {
        return Err(SwiftEmitError::Unsupported {
            swift_name: entry.swift_name.clone(),
            rust_path: entry.rust_path.clone(),
            reason: format!("schema root `type` is {ty}, expected \"object\""),
        });
    }
    let _ = entry.schema.title.as_deref();
    Ok(())
}

/// Convert one field schema to a Swift base type. Returns `None` for
/// shapes the Stage 1 emitter doesn't know about (the caller turns that
/// into [`SwiftEmitError::Unsupported`] with field-name context).
fn swift_type_for(raw: &serde_json::Value) -> Option<String> {
    let schema = raw.as_object()?;
    // `type` may be a string ("integer") OR an array (["integer", "null"]
    // for an Option). We treat the array case as nullable-of-the-non-null
    // tag — the caller's `required` check is the canonical source of
    // optionality, so we strip "null" here and let optionality come from
    // the `required` list.
    let type_kind = match schema.get("type")? {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(tags) => tags
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|s| *s != "null")?
            .to_string(),
        _ => return None,
    };

    let format = schema.get("format").and_then(serde_json::Value::as_str);

    match type_kind.as_str() {
        "string" => Some("String".to_string()),
        "boolean" => Some("Bool".to_string()),
        "integer" => Some(map_integer_format(format).to_string()),
        "number" => Some("Double".to_string()),
        "array" => {
            let items = schema.get("items")?;
            let inner = swift_type_for(items)?;
            Some(format!("[{inner}]"))
        }
        // `object` at field level means a nested struct / map. Stage 1
        // doesn't render either — that's Stage 2/3 work.
        _ => None,
    }
}

/// Map a JSON Schema integer `format` (`int32`, `uint64`, …) to the Swift
/// integer type Chirp's existing hand-written types use. The existing
/// convention (KernelBridge.swift) maps Rust `u32`→`UInt32`,
/// `u64`/`u128`/`usize`→`UInt64`, `i32`/`i64`→`Int`. The `uint128`
/// collapse is deliberate: Swift has no `UInt128` Decodable shape (it's
/// not in Foundation's `Codable` synthesis path); millisecond-epoch
/// timestamps the kernel emits as `u128` fit in `UInt64` for the next
/// ~580 million years, and the hand-written code has used this mapping
/// since day one.
fn map_integer_format(format: Option<&str>) -> &'static str {
    match format {
        Some("uint8") | Some("uint16") | Some("uint32") => "UInt32",
        Some("uint64") | Some("uint128") => "UInt64",
        // `usize` (schemars emits `format: "uint"`) maps to `Int` to match
        // the Swift convention for `Array.count`-style counters in the
        // existing hand-written Decodables.
        Some("uint") => "Int",
        Some("int8") | Some("int16") | Some("int32") | Some("int64") | Some("int") => "Int",
        // No format hint → safest default that fits any positive integer
        // schemars produces.
        _ => "Int",
    }
}

/// snake_case → camelCase. `relay_url` → `relayUrl`. Leading underscores
/// are preserved as-is (none appear in the pilot set; included for
/// robustness against future fields like `_internal`).
fn snake_to_camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    let mut upper_next = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            out.extend(ch.to_uppercase());
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Outcome of a `--check` run.
#[derive(Debug)]
pub struct SwiftCheckOutcome {
    /// `true` when the on-disk file matches the freshly-rendered output.
    pub up_to_date: bool,
    /// First differing line (1-based) when not up-to-date; `None` when
    /// up-to-date OR when the file doesn't exist yet.
    pub first_diff_line: Option<usize>,
}

/// Write the rendered Swift to `out_path`, replacing whatever was there.
///
/// # Errors
/// As [`render_swift`], plus filesystem I/O failures.
#[must_use]
pub fn generate_swift(document_json: &str, out_path: &Path) -> Result<(), SwiftEmitError> {
    let rendered = render_swift(document_json)?;
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(out_path, rendered)?;
    Ok(())
}

/// Diff a freshly-rendered output against the file at `out_path`.
///
/// # Errors
/// As [`render_swift`]. A missing file returns `up_to_date = false` with
/// `first_diff_line = None`, not an error — the CI gate treats "file
/// doesn't exist" the same as "file is stale".
#[must_use]
pub fn check_swift(document_json: &str, out_path: &Path) -> Result<SwiftCheckOutcome, SwiftEmitError> {
    let rendered = render_swift(document_json)?;
    let actual = match std::fs::read_to_string(out_path) {
        Ok(s) => s,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SwiftCheckOutcome {
                up_to_date: false,
                first_diff_line: None,
            });
        }
        Err(err) => return Err(SwiftEmitError::Io(err)),
    };
    if actual == rendered {
        return Ok(SwiftCheckOutcome {
            up_to_date: true,
            first_diff_line: None,
        });
    }
    let first_diff_line = actual
        .lines()
        .zip(rendered.lines())
        .position(|(a, b)| a != b)
        .map(|p| p + 1);
    Ok(SwiftCheckOutcome {
        up_to_date: false,
        first_diff_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal one-type document — covers the "no nested objects, mixed
    /// optional/required, snake↔camel transform" case.
    fn one_type_document() -> &'static str {
        r#"{
          "version": 1,
          "types": [
            {
              "rust_path": "nmp_core::demo::Sample",
              "swift_name": "Sample",
              "id_field": "id",
              "conformances": ["Decodable", "Equatable"],
              "schema": {
                "type": "object",
                "title": "Sample",
                "properties": {
                  "id": { "type": "string" },
                  "open_views": { "type": "integer", "format": "uint32", "minimum": 0 },
                  "first_event_ms": { "type": ["integer", "null"], "format": "uint128" },
                  "relay_urls": { "type": "array", "items": { "type": "string" } },
                  "denied": { "type": "boolean" }
                },
                "required": ["id", "open_views", "denied", "relay_urls"]
              }
            }
          ]
        }"#
    }

    #[test]
    fn renders_one_type_with_required_and_optional_fields() {
        let out = render_swift(one_type_document()).expect("renders");
        // Per-field expectations — assert the exact lines rather than
        // matching against a golden file, so test failures point at the
        // emitter rule that broke.
        assert!(out.contains("public struct Sample: Decodable, Equatable, Identifiable {"));
        // `id` field with literal name — synthesised Identifiable picks
        // it up; no extra `var id: String { id }` should appear.
        assert!(out.contains("    public let id: String\n"));
        assert!(
            !out.contains("public var id: String { id }"),
            "literal `id` field should NOT get a computed accessor"
        );
        assert!(out.contains("    public let openViews: UInt32\n"));
        // Optional field — `first_event_ms` is NOT in required, so `?`.
        assert!(out.contains("    public let firstEventMs: UInt64?\n"));
        // Array of strings.
        assert!(out.contains("    public let relayUrls: [String]\n"));
        assert!(out.contains("    public let denied: Bool\n"));
        // No explicit CodingKeys — convertFromSnakeCase handles the
        // snake_case → camelCase mapping at decode time. Emitting CodingKeys
        // with snake_case rawValues causes KEY_NOT_FOUND because the decoder
        // transforms JSON keys before matching rawValues.
        assert!(
            !out.contains("CodingKeys"),
            "Stage-1 types must not emit explicit CodingKeys (convertFromSnakeCase conflict)"
        );
        assert!(
            !out.contains("= \"open_views\""),
            "snake_case rawValues must not appear in generated code"
        );
    }

    #[test]
    fn identifiable_with_non_id_field_emits_computed_accessor() {
        let doc = r#"{
          "version": 1,
          "types": [
            {
              "rust_path": "demo::Row",
              "swift_name": "Row",
              "id_field": "key",
              "conformances": ["Decodable", "Equatable"],
              "schema": {
                "type": "object",
                "properties": { "key": { "type": "string" } },
                "required": ["key"]
              }
            }
          ]
        }"#;
        let out = render_swift(doc).expect("renders");
        assert!(out.contains("public struct Row: Decodable, Equatable, Identifiable {"));
        assert!(out.contains("public var id: String { key }"));
    }

    #[test]
    fn rejects_unknown_document_version() {
        let doc = r#"{ "version": 999, "types": [] }"#;
        let err = render_swift(doc).expect_err("must reject unknown version");
        assert!(matches!(
            err,
            SwiftEmitError::UnsupportedDocumentVersion { found: 999, expected: 1 }
        ));
    }

    #[test]
    fn rejects_non_object_root() {
        // Stage 1 must NOT silently render a tagged enum (its root schema
        // is `oneOf`, no `"type": "object"`). The error must name the
        // type so a future Stage 2/3 author knows what to migrate.
        let doc = r#"{
          "version": 1,
          "types": [
            {
              "rust_path": "demo::Tag",
              "swift_name": "Tag",
              "id_field": null,
              "conformances": ["Decodable", "Equatable"],
              "schema": { "oneOf": [{ "type": "object" }] }
            }
          ]
        }"#;
        let err = render_swift(doc).expect_err("rejects non-object root");
        match err {
            SwiftEmitError::Unsupported { swift_name, .. } => {
                assert_eq!(swift_name, "Tag");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn snake_to_camel_handles_common_shapes() {
        assert_eq!(snake_to_camel("relay_url"), "relayUrl");
        assert_eq!(snake_to_camel("first_event_ms"), "firstEventMs");
        assert_eq!(snake_to_camel("id"), "id");
        assert_eq!(snake_to_camel("a_b_c"), "aBC");
        // Already camelCase passes through unchanged.
        assert_eq!(snake_to_camel("alreadyCamel"), "alreadyCamel");
    }

    #[test]
    fn integer_format_mapping_matches_chirp_convention() {
        assert_eq!(map_integer_format(Some("uint32")), "UInt32");
        assert_eq!(map_integer_format(Some("uint64")), "UInt64");
        // `usize` (schemars `uint`) is the Swift-side `Int` for counts.
        assert_eq!(map_integer_format(Some("uint")), "Int");
        // `u128` collapses to `UInt64` — see `map_integer_format` doc.
        assert_eq!(map_integer_format(Some("uint128")), "UInt64");
        assert_eq!(map_integer_format(Some("int64")), "Int");
        // Unknown format → Int (safe default for any integer schemars emits).
        assert_eq!(map_integer_format(None), "Int");
    }

    #[test]
    fn check_swift_returns_up_to_date_on_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out.swift");
        generate_swift(one_type_document(), &out).expect("write");
        let result = check_swift(one_type_document(), &out).expect("check");
        assert!(result.up_to_date);
        assert_eq!(result.first_diff_line, None);
    }

    #[test]
    fn check_swift_flags_stale_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("out.swift");
        std::fs::write(&out, "// stale\n").expect("write");
        let result = check_swift(one_type_document(), &out).expect("check");
        assert!(!result.up_to_date);
        assert_eq!(result.first_diff_line, Some(1));
    }

    #[test]
    fn check_swift_treats_missing_file_as_stale() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("never_written.swift");
        let result = check_swift(one_type_document(), &out).expect("check");
        assert!(!result.up_to_date);
        assert_eq!(result.first_diff_line, None);
    }

    // ── V6 Stage 2 ──────────────────────────────────────────────────────────
    //
    // Tests for the `SnapshotProjections` registry render. These cover the
    // three load-bearing pieces independently — the
    // `.convertFromSnakeCase` algorithm, single-entry rendering, and the
    // full-registry render that the CI gate diffs against the committed
    // file.

    #[test]
    fn post_convert_handles_single_word_pass_through() {
        // No `_`, no `.` → strategy is a no-op. Covers `wallet`, `profile`,
        // `timeline`, `accounts`, etc.
        assert_eq!(post_convert_from_snake_case("wallet"), "wallet");
        assert_eq!(post_convert_from_snake_case("profile"), "profile");
        assert_eq!(post_convert_from_snake_case("zaps"), "zaps");
    }

    #[test]
    fn post_convert_camelises_snake_case() {
        // Standard snake_case → camelCase. Covers the bulk of the
        // built-in projection keys.
        assert_eq!(post_convert_from_snake_case("bunker_handshake"), "bunkerHandshake");
        assert_eq!(post_convert_from_snake_case("publish_queue"), "publishQueue");
        assert_eq!(post_convert_from_snake_case("active_account"), "activeAccount");
        assert_eq!(post_convert_from_snake_case("relay_diagnostics"), "relayDiagnostics");
    }

    #[test]
    fn post_convert_leaves_dots_opaque() {
        // `.` is NOT a separator for `.convertFromSnakeCase`; only `_` is.
        // The dotted host-registered keys camelise per segment.
        assert_eq!(
            post_convert_from_snake_case("nmp.nip29.group_chat"),
            "nmp.nip29.groupChat"
        );
        assert_eq!(
            post_convert_from_snake_case("nmp.nip17.dm_inbox"),
            "nmp.nip17.dmInbox"
        );
        assert_eq!(
            post_convert_from_snake_case("nmp.nip29.discovered_groups"),
            "nmp.nip29.discoveredGroups"
        );
        assert_eq!(
            post_convert_from_snake_case("nmp.nip17.dm_relay_list"),
            "nmp.nip17.dmRelayList"
        );
        // `nmp.follow_list` — only the tail `follow_list` carries an `_`.
        assert_eq!(
            post_convert_from_snake_case("nmp.follow_list"),
            "nmp.followList"
        );
        // `nmp.nip57.zaps` — no `_` anywhere. Strategy returns it
        // unchanged. The renderer must STILL emit an explicit raw value
        // because declaring `CodingKeys` overrides synthesis — the
        // synthesised default for the Swift property `zaps` would be the
        // bare string `"zaps"`, which doesn't match the dotted kernel key.
        assert_eq!(
            post_convert_from_snake_case("nmp.nip57.zaps"),
            "nmp.nip57.zaps"
        );
    }

    #[test]
    fn render_snapshot_projections_emits_one_field_and_one_case_per_entry() {
        // Three-entry hand-rolled registry covers the three case shapes:
        // single-word (`wallet`), snake_case → camelCase (`bunker_handshake`),
        // and dotted (`nmp.nip29.group_chat`).
        let entries = vec![
            SnapshotProjectionEntry {
                json_key: "wallet",
                swift_field: "wallet",
                swift_type: "WalletStatusData",
            },
            SnapshotProjectionEntry {
                json_key: "bunker_handshake",
                swift_field: "bunkerHandshake",
                swift_type: "BunkerHandshake",
            },
            SnapshotProjectionEntry {
                json_key: "nmp.nip29.group_chat",
                swift_field: "groupChat",
                swift_type: "GroupChatSnapshot",
            },
        ];
        let mut out = String::new();
        render_snapshot_projections(&entries, &mut out);

        // Struct header + per-field optional declaration.
        assert!(out.contains("struct SnapshotProjections: Decodable, Equatable {"));
        assert!(out.contains("    let wallet: WalletStatusData?\n"));
        assert!(out.contains("    let bunkerHandshake: BunkerHandshake?\n"));
        assert!(out.contains("    let groupChat: GroupChatSnapshot?\n"));

        // CodingKeys enum.
        assert!(out.contains("    enum CodingKeys: String, CodingKey {\n"));
        // `wallet`: post-transform equals the Swift field → no raw value.
        assert!(out.contains("        case wallet\n"));
        // `bunker_handshake`: post-transform `bunkerHandshake` matches the
        // Swift field → no raw value.
        assert!(out.contains("        case bunkerHandshake\n"));
        assert!(
            !out.contains("case bunkerHandshake = \"bunker_handshake\""),
            "snake_case keys whose camelCase post-transform matches the Swift field MUST not carry an explicit raw value"
        );
        // `nmp.nip29.group_chat`: post-transform `nmp.nip29.groupChat`
        // differs from the Swift field `groupChat` → explicit raw value.
        assert!(out.contains("        case groupChat = \"nmp.nip29.groupChat\"\n"));
    }

    #[test]
    fn render_snapshot_projections_emits_explicit_raw_for_dotted_no_underscore_key() {
        // The `zaps` trap: `nmp.nip57.zaps` has no `_`, so the strategy
        // returns it unchanged. The synthesised default for property
        // `zaps` would be `"zaps"`, which doesn't match the dotted key.
        // The renderer MUST emit an explicit `= "nmp.nip57.zaps"` raw
        // value because post-transform `"nmp.nip57.zaps"` != swift field
        // `"zaps"`.
        let entries = vec![SnapshotProjectionEntry {
            json_key: "nmp.nip57.zaps",
            swift_field: "zaps",
            swift_type: "ZapsAggregateSnapshot",
        }];
        let mut out = String::new();
        render_snapshot_projections(&entries, &mut out);
        assert!(
            out.contains("        case zaps = \"nmp.nip57.zaps\"\n"),
            "dotted no-underscore key MUST emit explicit raw value; got:\n{out}"
        );
    }

    #[test]
    fn render_swift_appends_snapshot_projections_section_after_pilot_types() {
        // The full pipeline: a Stage 1 document renders the seven pilot
        // types AND the Stage 2 SnapshotProjections at the bottom. The
        // CI gate diffs the whole file, so the section order is
        // load-bearing.
        let out = render_swift(one_type_document()).expect("renders");
        // Stage 1 output is still there.
        assert!(out.contains("public struct Sample: Decodable, Equatable, Identifiable {"));
        // Stage 2 SnapshotProjections is appended after.
        assert!(out.contains("struct SnapshotProjections: Decodable, Equatable {"));
        let sample_pos = out
            .find("public struct Sample:")
            .expect("Stage 1 Sample present");
        let snap_pos = out
            .find("struct SnapshotProjections:")
            .expect("Stage 2 SnapshotProjections present");
        assert!(
            snap_pos > sample_pos,
            "Stage 2 SnapshotProjections must follow Stage 1 types"
        );
    }
}
