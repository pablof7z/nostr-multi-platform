//! Cargo manifest gates that are cheaper and more precise through
//! `cargo metadata` than source-token scanning.
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use std::path::Path;
use std::process::Command;

use super::workspace_root;

const BANNED_APP_NORMAL_TEST_SUPPORT_DEPS: &[&str] = &["nmp-core", "nmp-ffi"];

#[test]
fn app_packages_do_not_enable_framework_test_support_in_normal_deps() {
    let root = workspace_root();
    let output = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("cargo metadata must spawn");

    assert!(
        output.status.success(),
        "cargo metadata failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata JSON must parse");
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");

    let mut violations = Vec::new();
    for package in packages {
        let Some(manifest_path) = package["manifest_path"].as_str() else {
            continue;
        };
        let manifest = Path::new(manifest_path);
        let Ok(relative_manifest) = manifest.strip_prefix(&root) else {
            continue;
        };
        if !relative_manifest.starts_with("apps") {
            continue;
        }

        let package_name = package["name"].as_str().unwrap_or("<unnamed>");
        let Some(dependencies) = package["dependencies"].as_array() else {
            continue;
        };
        for dependency in dependencies {
            let dep_name = dependency["name"].as_str().unwrap_or("<unnamed>");
            let is_normal_dep = dependency["kind"].is_null();
            let enables_test_support = dependency["features"]
                .as_array()
                .is_some_and(|features| features.iter().any(|f| f == "test-support"));
            if is_normal_dep
                && enables_test_support
                && BANNED_APP_NORMAL_TEST_SUPPORT_DEPS.contains(&dep_name)
            {
                violations.push(format!(
                    "{package_name} ({}) enables {dep_name}/test-support in [dependencies]",
                    relative_manifest.display()
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "apps/** production dependencies must not enable nmp-core/test-support \
         or nmp-ffi/test-support; move them to [dev-dependencies]:\n{}",
        violations.join("\n")
    );
}
