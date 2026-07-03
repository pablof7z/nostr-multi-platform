//! Swift wrapper emitter for app-local concept reads (#2899).

use super::app_registry::{AppConceptRead, LoadedAppConceptReadRegistry};
use super::registry::TargetInput;

/// Render Swift host wrappers for an app-local concept-read registry.
#[must_use]
pub fn render_registry(registry: &LoadedAppConceptReadRegistry) -> String {
    let mut out = String::new();
    out.push_str("// GENERATED. DO NOT EDIT BY HAND.\n");
    out.push_str("//\n");
    out.push_str("// Regenerate via:\n");
    out.push_str("//   cargo run -p nmp-codegen -- gen concept-reads \\\n");
    out.push_str("//       --registry <app>/concept-reads.json --platform swift\n");
    out.push_str("//\n");
    out.push_str("// Source of truth: app-local concept-reads registry JSON.\n\n");
    out.push_str("import Foundation\n\n");
    out.push_str("public enum GeneratedConceptReads {\n");
    for read in &registry.reads {
        render_read(registry, read, &mut out);
    }
    out.push_str("}\n");
    out
}

fn render_read(registry: &LoadedAppConceptReadRegistry, read: &AppConceptRead, out: &mut String) {
    let app = &registry.facade.rust_type;
    let open_name = snake_to_lower_camel(read.concept.open_fn);
    let close_name = snake_to_lower_camel(read.concept.close_fn);
    let decode_name = snake_to_lower_camel(read.concept.summary.facade_decode_fn);
    let schema_const = format!("{}SchemaId", read.concept.summary.native_family);
    let (arg_name, arg_type) = swift_target_arg(read);

    out.push_str(&format!(
        "    public static let {schema_const} = {:?}\n\n",
        read.concept.summary.schema_id
    ));
    out.push_str(&format!(
        "    public static func {open_name}(\n        app: {app},\n        {arg_name}: {arg_type}\n    ) throws -> {} {{\n",
        read.opened_record
    ));
    out.push_str(&format!(
        "        try app.{open_name}({arg_name}: {arg_name})\n"
    ));
    out.push_str("    }\n\n");
    out.push_str(&format!(
        "    public static func {close_name}(app: {app}, opened: {}) -> Bool {{\n",
        read.opened_record
    ));
    out.push_str(&format!("        app.{close_name}(opened: opened)\n"));
    out.push_str("    }\n\n");
    out.push_str(&format!(
        "    public static func {decode_name}(\n        app: {app},\n        schemaId: String,\n        payload: Data\n    ) throws -> {}? {{\n",
        read.summary.record
    ));
    out.push_str(&format!(
        "        guard schemaId == {schema_const} else {{ return nil }}\n"
    ));
    out.push_str(&format!(
        "        return try app.{decode_name}(payload: payload)\n"
    ));
    out.push_str("    }\n\n");
}

fn swift_target_arg(read: &AppConceptRead) -> (String, &'static str) {
    let arg = match read.concept.target_input {
        TargetInput::Json { arg_name, .. } | TargetInput::PlainString { arg_name } => arg_name,
    };
    (snake_to_lower_camel(arg), "String")
}

fn snake_to_lower_camel(value: &str) -> String {
    let mut out = String::new();
    for (index, part) in value.split('_').enumerate() {
        if index == 0 {
            out.push_str(part);
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.extend(chars);
        }
    }
    out
}
