//! Validation for app-local action-builder registry rows.

use std::collections::BTreeMap;

use crate::action_builders::app_registry_format::{ActionContractRow, DispatchKind};
use crate::action_builders::registry::ACTION_BUILDERS;
use crate::action_contract::ACTION_CONTRACT;

pub(super) fn validate_action_contract_row(action: &ActionContractRow) -> Result<(), String> {
    if action.action_namespace.trim().is_empty() {
        return Err("action_namespace must not be empty".to_string());
    }
    validate_app_namespace(&action.action_namespace)?;
    if action.schema.schema_id.trim().is_empty() {
        return Err(format!(
            "action {:?} schema_id must not be empty",
            action.action_namespace
        ));
    }
    if action.schema.schema_path.as_os_str().is_empty() {
        return Err(format!(
            "action {:?} schema_path must not be empty",
            action.action_namespace
        ));
    }
    if action.schema.root_type.trim().is_empty() {
        return Err(format!(
            "action {:?} root_type must not be empty",
            action.action_namespace
        ));
    }
    if action.schema.file_identifier.len() != 4 || !action.schema.file_identifier.is_ascii() {
        return Err(format!(
            "action {:?} file_identifier must be exactly four ASCII bytes",
            action.action_namespace
        ));
    }
    if action.schema.schema_version == 0 {
        return Err(format!(
            "action {:?} schema_version must be non-zero",
            action.action_namespace
        ));
    }
    if action.builder.method.trim().is_empty() {
        return Err(format!(
            "action {:?} builder.method must not be empty",
            action.action_namespace
        ));
    }
    validate_generated_identifier(
        "builder.method",
        &action.builder.method,
        &action.action_namespace,
    )?;
    if action.builder.doc.trim().is_empty() {
        return Err(format!(
            "action {:?} builder.doc must not be empty",
            action.action_namespace
        ));
    }
    if action.rust.rust_crate.trim().is_empty()
        || action.rust.module.trim().is_empty()
        || action.rust.payload_type.trim().is_empty()
        || action.rust.action_module.trim().is_empty()
    {
        return Err(format!(
            "action {:?} rust owner fields must not be empty",
            action.action_namespace
        ));
    }
    let _ = action.event_kind;
    match action.dispatch {
        DispatchKind::PublishesEvent | DispatchKind::AppLocal => {}
    }
    validate_builder_fields(action)?;
    Ok(())
}

fn validate_app_namespace(namespace: &str) -> Result<(), String> {
    if ACTION_CONTRACT
        .iter()
        .any(|contract| contract.namespace == namespace)
        || ACTION_BUILDERS
            .iter()
            .any(|builder| builder.namespace == namespace)
    {
        return Err(format!(
            "app-local action_namespace {namespace:?} collides with a built-in NMP action namespace; choose an app-owned namespace such as \"app.<app>.<action>\""
        ));
    }
    Ok(())
}

fn validate_builder_fields(action: &ActionContractRow) -> Result<(), String> {
    let mut symbols = BTreeMap::new();
    for field in &action.builder.fields {
        validate_generated_identifier("builder field name", &field.name, &action.action_namespace)?;
        insert_generated_symbol(&mut symbols, &field.name, "field", &action.action_namespace)?;
        if let Some(flag_name) = field.presence_flag.as_deref() {
            validate_generated_identifier("presence_flag", flag_name, &action.action_namespace)?;
            insert_generated_symbol(
                &mut symbols,
                flag_name,
                "presence_flag",
                &action.action_namespace,
            )?;
        }
    }
    Ok(())
}

fn insert_generated_symbol(
    symbols: &mut BTreeMap<String, &'static str>,
    symbol: &str,
    kind: &'static str,
    namespace: &str,
) -> Result<(), String> {
    if let Some(previous) = symbols.insert(symbol.to_string(), kind) {
        return Err(format!(
            "action {namespace:?} has duplicate generated symbol {symbol:?}: {previous} conflicts with {kind}"
        ));
    }
    Ok(())
}

fn validate_generated_identifier(kind: &str, value: &str, namespace: &str) -> Result<(), String> {
    if !is_lower_camel_ascii_identifier(value) {
        return Err(format!(
            "action {namespace:?} {kind} {value:?} is not a valid generated identifier; use lowerCamelCase ASCII matching [a-z][A-Za-z0-9]*"
        ));
    }
    if is_reserved_generated_identifier(value) {
        return Err(format!(
            "action {namespace:?} {kind} {value:?} is reserved by Swift/Kotlin/TypeScript or by the generated action-builder template; choose a different lowerCamelCase name"
        ));
    }
    Ok(())
}

fn is_lower_camel_ascii_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase() && chars.all(|c| c.is_ascii_alphanumeric())
}

fn is_reserved_generated_identifier(value: &str) -> bool {
    RESERVED_GENERATED_IDENTIFIERS.contains(&value)
}

const RESERVED_GENERATED_IDENTIFIERS: &[&str] = &[
    // Generated action-builder helper names and locals shared across emitters.
    "actionNamespace",
    "correlationId",
    "dispatchEnvelopeSchemaVersion",
    "encodeDispatchEnvelope",
    "fbb",
    "payload",
    "payloadOffset",
    "payloadRoot",
    "payloadStart",
    "relayMarkerByte",
    "root",
    "schemaVersion",
    "start",
    // Swift keywords.
    "actor",
    "as",
    "associatedtype",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "deinit",
    "do",
    "else",
    "enum",
    "extension",
    "fallthrough",
    "false",
    "fileprivate",
    "for",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "inout",
    "internal",
    "is",
    "let",
    "nil",
    "operator",
    "private",
    "protocol",
    "public",
    "rethrows",
    "return",
    "self",
    "static",
    "struct",
    "subscript",
    "super",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
    // Kotlin keywords and hard keywords that are also valid lowerCamel words.
    "abstract",
    "actual",
    "annotation",
    "by",
    "companion",
    "const",
    "constructor",
    "crossinline",
    "data",
    "delegate",
    "dynamic",
    "expect",
    "external",
    "field",
    "file",
    "final",
    "finally",
    "fun",
    "get",
    "infix",
    "inline",
    "inner",
    "interface",
    "lateinit",
    "noinline",
    "null",
    "object",
    "open",
    "out",
    "override",
    "package",
    "param",
    "property",
    "protected",
    "receiver",
    "reified",
    "sealed",
    "set",
    "suspend",
    "tailrec",
    "this",
    "typealias",
    "typeof",
    "val",
    "vararg",
    "when",
    // TypeScript/JavaScript reserved words.
    "arguments",
    "debugger",
    "delete",
    "eval",
    "export",
    "extends",
    "function",
    "implements",
    "instanceof",
    "new",
    "type",
    "undefined",
    "void",
    "with",
    "yield",
];
