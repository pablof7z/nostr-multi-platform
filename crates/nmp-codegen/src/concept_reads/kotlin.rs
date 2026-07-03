//! Kotlin wrapper emitter for app-local concept reads (#2899).

use std::collections::BTreeSet;

use super::app_registry::{AppConceptRead, LoadedAppConceptReadRegistry};
use super::registry::TargetInput;

/// Render Kotlin host wrappers for an app-local concept-read registry.
#[must_use]
pub fn render_registry(registry: &LoadedAppConceptReadRegistry) -> String {
    let package = registry
        .outputs
        .kotlin_package
        .as_deref()
        .expect("registry validates kotlin_package when kotlin output is present");
    let uniffi_package = registry
        .outputs
        .kotlin_uniffi_package
        .as_deref()
        .expect("registry validates kotlin_uniffi_package when kotlin output is present");
    let mut out = String::new();
    out.push_str("// GENERATED. DO NOT EDIT BY HAND.\n");
    out.push_str("//\n");
    out.push_str("// Regenerate via:\n");
    out.push_str("//   cargo run -p nmp-codegen -- gen concept-reads \\\n");
    out.push_str("//       --registry <app>/concept-reads.json --platform kotlin\n");
    out.push_str("//\n");
    out.push_str("// Source of truth: app-local concept-reads registry JSON.\n\n");
    out.push_str(&format!("package {package}\n\n"));
    render_imports(registry, uniffi_package, &mut out);
    out.push_str("\nobject GeneratedConceptReads {\n");
    for read in &registry.reads {
        render_read(registry, read, &mut out);
    }
    out.push_str("}\n");
    out
}

fn render_imports(registry: &LoadedAppConceptReadRegistry, uniffi_package: &str, out: &mut String) {
    let mut imports = BTreeSet::new();
    imports.insert(registry.facade.rust_type.as_str());
    for read in &registry.reads {
        imports.insert(read.opened_record.as_str());
        imports.insert(read.summary.record.as_str());
    }
    for name in imports {
        out.push_str(&format!("import {uniffi_package}.{name}\n"));
    }
}

fn render_read(registry: &LoadedAppConceptReadRegistry, read: &AppConceptRead, out: &mut String) {
    let app = &registry.facade.rust_type;
    let open_name = snake_to_lower_camel(read.concept.open_fn);
    let close_name = snake_to_lower_camel(read.concept.close_fn);
    let decode_name = snake_to_lower_camel(read.concept.summary.facade_decode_fn);
    let schema_const = read.concept.summary.native_schema_const;
    let (arg_name, arg_type) = kotlin_target_arg(read);

    out.push_str(&format!(
        "    const val {schema_const}: String = {:?}\n\n",
        read.concept.summary.schema_id
    ));
    out.push_str(&format!(
        "    fun {open_name}(app: {app}, {arg_name}: {arg_type}): {} =\n",
        read.opened_record
    ));
    out.push_str(&format!("        app.{open_name}({arg_name})\n\n"));
    out.push_str(&format!(
        "    fun {close_name}(app: {app}, opened: {}): Boolean =\n",
        read.opened_record
    ));
    out.push_str(&format!("        app.{close_name}(opened)\n\n"));
    out.push_str(&format!(
        "    fun {decode_name}(app: {app}, schemaId: String, payload: ByteArray): {}? {{\n",
        read.summary.record
    ));
    out.push_str(&format!(
        "        if (schemaId != {schema_const}) return null\n"
    ));
    out.push_str(&format!("        return app.{decode_name}(payload)\n"));
    out.push_str("    }\n\n");
}

fn kotlin_target_arg(read: &AppConceptRead) -> (String, &'static str) {
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
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.extend(chars);
            }
        }
    }
    out
}
