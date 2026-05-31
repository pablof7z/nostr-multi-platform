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
    #[serde(default)]
    render_identity_fields: Vec<String>,
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
    if !entry.render_identity_fields.is_empty() {
        conformances.insert("RenderIdentifiable".to_string());
    }
    // The ordered emit list. Anything not in this array is silently dropped
    // from the conformance clause — entries here act as the allowlist AND
    // the emit order. `Sendable` is appended last because Apple convention
    // groups conformances by Codable → Equality → Identity → Hashing →
    // Concurrency; the generated header reads top-down in that order.
    //
    // ── Sendable rationale ────────────────────────────────────────────────
    // Every generated type is an immutable value-typed `struct` with
    // `let` fields whose types are either themselves Sendable primitives
    // (`String`, `Bool`, integer family, `Optional<T>`) or other generated
    // types. So every generated struct is conceptually Sendable, and
    // declaring it explicitly is required for `public` Swift types —
    // unlike `internal` types, Apple does NOT infer Sendable for public
    // structs (SE-0302 §"Sendable type inference"), and a consumer that
    // composes the generated type into a non-Sendable wrapper (e.g.
    // `NoteRenderContext` holding `[String: TimelineItem]` in a
    // `static let`) hard-fails under strict concurrency. The fix is at
    // the source: every generated struct opts in to Sendable explicitly.
    let conformances: Vec<&str> =
        ["Decodable", "Equatable", "RenderIdentifiable", "Identifiable", "Hashable", "Sendable"]
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

    if !entry.render_identity_fields.is_empty() {
        let comparisons: Vec<String> = entry
            .render_identity_fields
            .iter()
            .map(|f| {
                let c = snake_to_camel(f);
                format!("self.{c} == other.{c}")
            })
            .collect();
        out.push('\n');
        out.push_str("    public func rendersIdentically(_ other: Self) -> Bool {\n");
        out.push_str(&format!(
            "        {}\n",
            comparisons.join("\n            && ")
        ));
        out.push_str("    }\n");
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
#[path = "swift/tests.rs"]
mod tests;
