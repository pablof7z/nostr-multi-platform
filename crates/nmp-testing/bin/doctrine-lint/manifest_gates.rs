//! Cargo manifest gates that are cheaper and more precise through
//! `cargo metadata` than source-token scanning.
//!
//! Split out of `tests.rs` (file-size cap). Shared helpers imported from
//! parent module via `super`.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::workspace_root;

const BANNED_APP_NORMAL_TEST_SUPPORT_DEPS: &[&str] = &["nmp-core", "nmp-ffi"];
const WORKSPACE_OWNED_DEPS_WITH_RATCHET: &[&str] = &[
    "nmp-ownership",
    "rustls",
    "serde",
    "serde_json",
    "tungstenite",
    "zeroize",
];

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

#[test]
fn workspace_owned_deps_do_not_reintroduce_member_versions() {
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
        let manifest = PathBuf::from(manifest_path);
        let Ok(relative_manifest) = manifest.strip_prefix(&root) else {
            continue;
        };
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|err| panic!("read {}: {err}", relative_manifest.display()));
        scan_workspace_owned_dependency_declarations(relative_manifest, &text, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "workspace-owned dependency declarations must use `workspace = true` \
         instead of member-local versions or paths:\n{}",
        violations.join("\n")
    );
}

fn scan_workspace_owned_dependency_declarations(
    manifest: &Path,
    text: &str,
    violations: &mut Vec<String>,
) {
    let mut in_dependency_table = false;
    let mut active_multiline_dep: Option<PendingDep> = None;

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = strip_inline_comment(raw_line).trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('[') {
            if let Some(dep) = active_multiline_dep.take() {
                push_pending_violation(manifest, dep, violations);
            }
            in_dependency_table = is_dependency_table(&trimmed);
            continue;
        }

        if let Some(dep) = active_multiline_dep.as_mut() {
            dep.saw_workspace |= has_key(&trimmed, "workspace");
            dep.saw_explicit_version_or_path |=
                has_key(&trimmed, "version") || has_key(&trimmed, "path");
            if trimmed.contains('}') {
                let dep = active_multiline_dep.take().expect("active dep exists");
                push_pending_violation(manifest, dep, violations);
            }
            continue;
        }

        if !in_dependency_table {
            continue;
        }

        let Some((name, value)) = trimmed.split_once('=') else {
            continue;
        };
        let dep_name = name.trim().trim_matches('"');
        if !WORKSPACE_OWNED_DEPS_WITH_RATCHET.contains(&dep_name) {
            continue;
        }

        let value = value.trim();
        let saw_workspace = has_key(value, "workspace");
        let saw_explicit_version_or_path =
            value.starts_with('"') || has_key(value, "version") || has_key(value, "path");
        let dep = PendingDep {
            name: dep_name.to_string(),
            line_no,
            saw_workspace,
            saw_explicit_version_or_path,
        };

        if value.contains('{') && !value.contains('}') {
            active_multiline_dep = Some(dep);
        } else if dep.saw_explicit_version_or_path || !dep.saw_workspace {
            push_pending_violation(manifest, dep, violations);
        }
    }

    if let Some(dep) = active_multiline_dep {
        push_pending_violation(manifest, dep, violations);
    }
}

#[derive(Debug)]
struct PendingDep {
    name: String,
    line_no: usize,
    saw_workspace: bool,
    saw_explicit_version_or_path: bool,
}

fn push_pending_violation(manifest: &Path, dep: PendingDep, violations: &mut Vec<String>) {
    if dep.saw_explicit_version_or_path || !dep.saw_workspace {
        violations.push(format!(
            "{}:{}: {} must use `{} = {{ workspace = true, ... }}`",
            manifest.display(),
            dep.line_no,
            dep.name,
            dep.name
        ));
    }
}

fn is_dependency_table(table: &str) -> bool {
    table == "[dependencies]"
        || table == "[dev-dependencies]"
        || table == "[build-dependencies]"
        || table.ends_with(".dependencies]")
        || table.ends_with(".dev-dependencies]")
        || table.ends_with(".build-dependencies]")
}

fn has_key(text: &str, key: &str) -> bool {
    text.split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        .any(|token| token == key)
}

fn strip_inline_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut previous = '\0';
    for (idx, ch) in line.char_indices() {
        if ch == '"' && previous != '\\' {
            in_string = !in_string;
        }
        if ch == '#' && !in_string {
            return &line[..idx];
        }
        previous = ch;
    }
    line
}

#[test]
fn workspace_owned_dependency_scanner_flags_versions_and_paths() {
    let mut violations = Vec::new();
    scan_workspace_owned_dependency_declarations(
        Path::new("crates/example/Cargo.toml"),
        r#"
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
nmp-ownership = { path = "../nmp-ownership" }
rustls = { workspace = true, default-features = false, features = ["ring"] }
"#,
        &mut violations,
    );

    assert_eq!(violations.len(), 3, "{violations:#?}");
    assert!(violations.iter().any(|v| v.contains("serde must use")));
    assert!(violations.iter().any(|v| v.contains("serde_json must use")));
    assert!(violations
        .iter()
        .any(|v| v.contains("nmp-ownership must use")));
}

#[test]
fn workspace_owned_dependency_scanner_ignores_features_table() {
    let mut violations = Vec::new();
    scan_workspace_owned_dependency_declarations(
        Path::new("crates/example/Cargo.toml"),
        r#"
[features]
default = ["serde"]

[dependencies]
serde = { workspace = true, features = ["derive"] }
zeroize = { workspace = true, features = ["alloc"] }
"#,
        &mut violations,
    );

    assert!(violations.is_empty(), "{violations:#?}");
}
