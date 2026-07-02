//! Owns the Stage 1 flat-record Swift `Decodable` struct emitter — turning
//! one [`TypeEntry`] (a flat-object JSON Schema) into a `public struct …:
//! Decodable, …` declaration. This is the "type emission" half of the V6
//! Stage 1 pilot; the sibling [`crate::swift::snapshot_projections_emit`]
//! module owns the unrelated Stage 2 registry-wiring emission, and
//! [`crate::swift`] owns the orchestration that calls into both.
//!
//! Split out of `swift.rs` so the top-level orchestration file stays under
//! the file-size ceiling. `render_type` is the only entry point the parent
//! calls; everything else here is a private implementation detail of that
//! one field-by-field render.

use std::collections::BTreeSet;

use super::error::SwiftEmitError;
use super::schema_input::TypeEntry;

/// Render one type into `out`.
pub(crate) fn render_type(entry: &TypeEntry, out: &mut String) -> Result<(), SwiftEmitError> {
    require_flat_object(entry)?;

    // Provenance comment — source-of-truth line per plan §5c.
    out.push_str(&format!(
        "// MARK: - {}\n// Source: {}\n",
        entry.swift_name, entry.rust_path
    ));

    // Conformance clause. `Identifiable` is appended automatically when
    // `id_field` is `Some` so the registry never has to repeat itself.
    let mut conformances: BTreeSet<String> = entry.conformances.iter().cloned().collect();
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
    // composes the generated type into a non-Sendable wrapper hard-fails under
    // strict concurrency. The fix is at the source: every generated struct opts
    // in to Sendable explicitly.
    let conformances: Vec<&str> = [
        "Decodable",
        "Equatable",
        "RenderIdentifiable",
        "Identifiable",
        "Hashable",
        "Sendable",
    ]
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
            reason: format!("field `{raw_name}` has unsupported schema shape: {raw_schema}"),
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
            out.push_str(&format!("    public var id: String {{ {id_field} }}\n"));
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
    let ty = entry
        .schema
        .ty
        .as_ref()
        .ok_or_else(|| SwiftEmitError::Unsupported {
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
pub(crate) fn map_integer_format(format: Option<&str>) -> &'static str {
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

/// snake_case → camelCase. `relay_url` → `relayUrl`. Leading underscores are
/// preserved (`_secret_bytes` → `_secretBytes`), for future `_internal` fields.
pub(crate) fn snake_to_camel(snake: &str) -> String {
    let mut out = String::with_capacity(snake.len());
    // Preserve leading underscores verbatim (the loop below would drop them).
    let trimmed = snake.trim_start_matches('_');
    out.push_str(&snake[..snake.len() - trimmed.len()]);
    let mut upper_next = false;
    for ch in trimmed.chars() {
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
