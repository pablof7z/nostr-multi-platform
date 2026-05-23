mod ffi_gen;
mod generate;
mod manifest;
// V6 Stage 1 — Swift `Decodable` emitter pilot. Consumes the JSON document
// `nmp-core --features codegen-schema --bin dump_projection_schemas` writes,
// emits one Swift file with one struct per pilot type. See
// `docs/architecture-audit/v6-codegen-plan.md` §6b. Keeps the existing
// `generate_modules` (Rust-shell scaffolding) and the new Swift emitter on
// independent code paths — they share no rendering primitives because the
// existing module-scaffolding generator is parameterised on `AppManifest`,
// not type-schema data.
pub mod swift;
// V6 Stage 2 — dotted-projection-key registry for `SnapshotProjections` +
// `CodingKeys`. Hand-transcribed from the existing Swift declaration in
// `ios/Chirp/Chirp/Bridge/KernelBridge.swift`; the renderer in `swift.rs`
// appends `SnapshotProjections` to the generated file using this slice.
// Lives in `nmp-codegen` (D0-exempt) so the registry can name dotted host
// keys like `"nmp.nip29.group_chat"` without tripping doctrine-lint on
// `nmp-core`. See module doc for the full rationale.
pub mod swift_projections_registry;

use std::path::Path;

pub use generate::{generate_modules, GenerationReport};
pub use manifest::{AppManifest, ModuleSet};
pub use swift::{check_swift, generate_swift, SwiftCheckOutcome, SwiftEmitError};

pub fn check_modules(manifest_path: &Path, out_dir: &Path) -> Result<bool, String> {
    let scratch = out_dir.with_extension("nmp-check");
    if scratch.exists() {
        std::fs::remove_dir_all(&scratch).map_err(|error| error.to_string())?;
    }

    let first = generate_modules(manifest_path, &scratch)?;
    let mut changed = false;
    for relative in first.files {
        let expected = std::fs::read(scratch.join(&relative)).map_err(|error| error.to_string())?;
        let actual_path = out_dir.join(&relative);
        match std::fs::read(actual_path) {
            Ok(actual) if actual == expected => {}
            _ => changed = true,
        }
    }
    std::fs::remove_dir_all(&scratch).map_err(|error| error.to_string())?;
    Ok(!changed)
}

pub(crate) fn rust_crate_name(package: &str) -> String {
    package.replace('-', "_")
}

pub(crate) fn variant_name(package: &str) -> String {
    package
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[must_use] 
pub fn app_crate_name(app_name: &str) -> String {
    format!("nmp-app-{}", app_name.replace('_', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_and_variant_names_are_stable() {
        assert_eq!(rust_crate_name("fixture-todo-core"), "fixture_todo_core");
        assert_eq!(variant_name("nmp-nip01"), "NmpNip01");
        assert_eq!(app_crate_name("demo_app"), "nmp-app-demo-app");
    }

    #[test]
    fn rust_crate_name_replaces_every_dash() {
        // The crate-path identifier form: all dashes become underscores, every
        // occurrence, so `a-b-c` is a valid Rust path segment.
        assert_eq!(rust_crate_name("a-b-c"), "a_b_c");
        // No dashes → unchanged.
        assert_eq!(rust_crate_name("nmpcore"), "nmpcore");
        // Already-underscored input is left alone (only dashes are touched).
        assert_eq!(rust_crate_name("fixture_todo"), "fixture_todo");
    }

    #[test]
    fn variant_name_upper_camel_cases_across_both_separators() {
        // `variant_name` splits on BOTH `-` and `_`, so a mixed-separator
        // package name still produces a single UpperCamelCase identifier.
        assert_eq!(variant_name("nmp-nip01"), "NmpNip01");
        assert_eq!(variant_name("fixture_todo_core"), "FixtureTodoCore");
        assert_eq!(variant_name("nmp-todo_core"), "NmpTodoCore");
    }

    #[test]
    fn variant_name_handles_degenerate_separator_inputs() {
        // Empty input → empty identifier (the caller never feeds this, but the
        // function must not panic on it).
        assert_eq!(variant_name(""), "");
        // All separators / leading + trailing separators: the empty parts are
        // filtered, so no stray empty segment leaks into the output.
        assert_eq!(variant_name("---"), "");
        assert_eq!(variant_name("-nmp-nip01-"), "NmpNip01");
        assert_eq!(variant_name("__a__"), "A");
    }

    #[test]
    fn variant_name_only_touches_the_leading_letter_of_each_segment() {
        // Capitalisation rule: uppercase the first ASCII char of each segment,
        // leave the rest of the segment verbatim. An already-capitalised or
        // numeric tail is preserved exactly — no lowercasing pass.
        assert_eq!(variant_name("nip-01"), "Nip01");
        assert_eq!(variant_name("ABC-def"), "ABCDef");
        // A segment whose first char is a digit cannot be uppercased and is
        // emitted unchanged — `to_ascii_uppercase` on a digit is a no-op.
        assert_eq!(variant_name("01-nip"), "01Nip");
    }

    #[test]
    fn app_crate_name_normalizes_underscores_to_dashes() {
        // `nmp-app-` prefix, and the app name's underscores become dashes so
        // the result is a conventional kebab-case crate name.
        assert_eq!(app_crate_name("demo_app"), "nmp-app-demo-app");
        assert_eq!(app_crate_name("chirp"), "nmp-app-chirp");
        // Dashes in the input are already kebab and pass through untouched.
        assert_eq!(app_crate_name("multi-word-app"), "nmp-app-multi-word-app");
    }
}
