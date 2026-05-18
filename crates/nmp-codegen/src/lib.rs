mod ffi_gen;
mod generate;
mod manifest;

use std::path::Path;

pub use generate::{generate_modules, GenerationReport};
pub use manifest::{AppManifest, ModuleSet};

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
}
