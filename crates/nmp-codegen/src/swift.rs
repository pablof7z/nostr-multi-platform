/// Swift Codable struct emitter for NMP codegen.
///
/// This module provides library functions to emit Swift `struct … : Codable`
/// definitions from a structured field description.  The emitter is
/// deliberately zero-dependency (matching the rest of `nmp-codegen`) and
/// produces byte-deterministic output: given identical input it always
/// produces identical output (no map iteration, no timestamps, no env).
///
/// # CodingKeys rule
///
/// Swift requires that once you opt into a `CodingKeys` enum it must list
/// **every** field in the struct.  The emitter mirrors this:
///
/// * If **no** field has a JSON key that differs from its Swift field name →
///   emit the struct body only, no `CodingKeys` enum.
/// * If **any** field is renamed → emit a full `CodingKeys` enum that lists
///   all fields.  Only the renamed ones carry a `= "json_key"` string literal;
///   the rest are bare `case` lines.

/// The Swift type of a struct field.
#[derive(Debug, Clone)]
pub enum SwiftType {
    StringType,
    U64,
    I64,
    Bool,
    Double,
    Optional(Box<SwiftType>),
    Array(Box<SwiftType>),
    Custom(String),
}

impl SwiftType {
    /// Return the Swift source spelling of this type.
    pub fn swift_name(&self) -> String {
        match self {
            SwiftType::StringType => "String".into(),
            SwiftType::U64 => "UInt64".into(),
            SwiftType::I64 => "Int64".into(),
            SwiftType::Bool => "Bool".into(),
            SwiftType::Double => "Double".into(),
            SwiftType::Optional(inner) => format!("{}?", inner.swift_name()),
            SwiftType::Array(inner) => format!("[{}]", inner.swift_name()),
            SwiftType::Custom(name) => name.clone(),
        }
    }
}

/// Emit a Swift `Codable` struct definition.
///
/// Each entry in `fields` is `(swift_field_name, json_key, type)`.  The
/// caller supplies both the camelCase Swift field name **and** the
/// snake_case JSON key because the mapping is not always mechanically
/// reversible (e.g. single-word names, acronyms).
///
/// A `CodingKeys` enum is emitted if and only if at least one field's
/// `json_key` differs from its `swift_field_name`.  When emitted it lists
/// every field; renamed fields include `= "json_key"`, bare fields do not.
///
/// The output always ends with a trailing newline.
pub fn emit_codable(
    name: &str,
    fields: &[(String, String, SwiftType)], // (swift_field_name, json_key, type)
) -> String {
    let needs_coding_keys = fields
        .iter()
        .any(|(swift_name, json_key, _)| swift_name != json_key);

    let mut out = String::new();

    // ── struct header ─────────────────────────────────────────────────────
    out.push_str(&format!("struct {name}: Codable {{\n"));

    // ── stored properties ─────────────────────────────────────────────────
    for (swift_name, _, ty) in fields {
        out.push_str(&format!("    let {swift_name}: {}\n", ty.swift_name()));
    }

    // ── CodingKeys (conditional) ──────────────────────────────────────────
    if needs_coding_keys {
        out.push('\n');
        out.push_str("    enum CodingKeys: String, CodingKey {\n");
        for (swift_name, json_key, _) in fields {
            if swift_name != json_key {
                out.push_str(&format!(
                    "        case {swift_name} = \"{json_key}\"\n"
                ));
            } else {
                out.push_str(&format!("        case {swift_name}\n"));
            }
        }
        out.push_str("    }\n");
    }

    // ── closing brace ─────────────────────────────────────────────────────
    out.push_str("}\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build the `ActionOutcome` field list used across multiple tests.
    fn action_outcome_fields() -> Vec<(String, String, SwiftType)> {
        vec![
            (
                "correlationId".to_string(),
                "correlation_id".to_string(),
                SwiftType::StringType,
            ),
            (
                "namespace".to_string(),
                "namespace".to_string(),
                SwiftType::StringType,
            ),
            (
                "relayUrl".to_string(),
                "relay_url".to_string(),
                SwiftType::StringType,
            ),
            (
                "accepted".to_string(),
                "accepted".to_string(),
                SwiftType::Bool,
            ),
            (
                "reason".to_string(),
                "reason".to_string(),
                SwiftType::Optional(Box::new(SwiftType::StringType)),
            ),
        ]
    }

    // ── pilot round-trip ──────────────────────────────────────────────────

    #[test]
    fn action_outcome_round_trip() {
        // This is the canonical pilot test: the exact Swift output expected
        // for the `ActionOutcome` Rust struct (see task description).
        let expected = r#"struct ActionOutcome: Codable {
    let correlationId: String
    let namespace: String
    let relayUrl: String
    let accepted: Bool
    let reason: String?

    enum CodingKeys: String, CodingKey {
        case correlationId = "correlation_id"
        case namespace
        case relayUrl = "relay_url"
        case accepted
        case reason
    }
}
"#;
        let actual = emit_codable("ActionOutcome", &action_outcome_fields());
        assert_eq!(actual, expected, "ActionOutcome output must be byte-identical");
    }

    // ── no-rename path (no CodingKeys emitted) ────────────────────────────

    #[test]
    fn no_rename_omits_coding_keys() {
        // When every swift_field_name matches its json_key, the CodingKeys
        // enum must NOT appear — emitting it when it's unnecessary clutters
        // the Swift file.
        let fields = vec![
            (
                "id".to_string(),
                "id".to_string(),
                SwiftType::StringType,
            ),
            (
                "count".to_string(),
                "count".to_string(),
                SwiftType::U64,
            ),
        ];
        let out = emit_codable("SimpleStruct", &fields);
        assert!(
            !out.contains("CodingKeys"),
            "no-rename struct must not contain CodingKeys:\n{out}"
        );
        assert_eq!(
            out,
            "struct SimpleStruct: Codable {\n    let id: String\n    let count: UInt64\n}\n"
        );
    }

    // ── any-rename path forces full CodingKeys ────────────────────────────

    #[test]
    fn one_renamed_field_emits_full_coding_keys() {
        // Even a single renamed field triggers a full CodingKeys enum with
        // all four fields listed — the bare ones without `= "…"`.
        let fields = vec![
            (
                "userId".to_string(),
                "user_id".to_string(),
                SwiftType::StringType,
            ),
            (
                "name".to_string(),
                "name".to_string(),
                SwiftType::StringType,
            ),
        ];
        let out = emit_codable("User", &fields);
        // Both fields must appear in CodingKeys.
        assert!(out.contains("case userId = \"user_id\""));
        assert!(out.contains("case name\n"));
        // Bare field must NOT carry a string literal.
        assert!(!out.contains("case name = "));
    }

    // ── zero-field degenerate case ────────────────────────────────────────

    #[test]
    fn empty_fields_emits_empty_struct() {
        let out = emit_codable("Empty", &[]);
        assert_eq!(out, "struct Empty: Codable {\n}\n");
    }

    // ── determinism invariant ─────────────────────────────────────────────

    #[test]
    fn emit_is_deterministic() {
        // Same fields in → byte-identical output on successive calls.
        let fields = action_outcome_fields();
        let a = emit_codable("ActionOutcome", &fields);
        let b = emit_codable("ActionOutcome", &fields);
        assert_eq!(a, b);
    }

    // ── type name coverage ────────────────────────────────────────────────

    #[test]
    fn swift_type_names_are_correct() {
        assert_eq!(SwiftType::StringType.swift_name(), "String");
        assert_eq!(SwiftType::U64.swift_name(), "UInt64");
        assert_eq!(SwiftType::I64.swift_name(), "Int64");
        assert_eq!(SwiftType::Bool.swift_name(), "Bool");
        assert_eq!(SwiftType::Double.swift_name(), "Double");
        assert_eq!(
            SwiftType::Optional(Box::new(SwiftType::StringType)).swift_name(),
            "String?"
        );
        assert_eq!(
            SwiftType::Array(Box::new(SwiftType::U64)).swift_name(),
            "[UInt64]"
        );
        assert_eq!(SwiftType::Custom("MyType".to_string()).swift_name(), "MyType");
        // Nested: Optional<Array<String>>
        assert_eq!(
            SwiftType::Optional(Box::new(SwiftType::Array(Box::new(SwiftType::StringType))))
                .swift_name(),
            "[String]?"
        );
    }
}
