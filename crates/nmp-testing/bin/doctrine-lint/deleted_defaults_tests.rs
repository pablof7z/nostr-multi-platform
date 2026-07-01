//! Smoke tests for the deleted `nmp-defaults` ratchet.

use std::path::{Path, PathBuf};

use super::{fixture_path, run_lint, workspace_root};

#[path = "rules/deleted_defaults.rs"]
mod deleted_defaults;

#[test]
fn deleted_defaults_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_deleted_defaults_pos")
        .join("apps")
        .join("demo")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_deleted_defaults_pos"),
    );
    std::fs::create_dir_all(&tmp).expect("create fake app src dir");
    let pos_src = workspace.join(fixture_path("deleted_defaults/pos.rs"));
    std::fs::copy(&pos_src, tmp.join("lib.rs")).expect("copy positive fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 1,
        "deleted_defaults positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains(&format!("error[{}]", deleted_defaults::ID)),
        "positive fixture must emit deleted_defaults finding; stdout:\n{}",
        stdout
    );
    for token in [
        "nmp_defaults",
        "register_defaults_with_handles",
        "TestDefaults",
    ] {
        assert!(
            stdout.contains(token),
            "positive fixture must flag `{token}`; stdout:\n{}",
            stdout
        );
    }
}

#[test]
fn deleted_defaults_negative_fixture_is_clean() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_deleted_defaults_neg")
        .join("apps")
        .join("demo")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_deleted_defaults_neg"),
    );
    std::fs::create_dir_all(&tmp).expect("create fake app src dir");
    let neg_src = workspace.join(fixture_path("deleted_defaults/neg.rs"));
    std::fs::copy(&neg_src, tmp.join("lib.rs")).expect("copy negative fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "deleted_defaults negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains(&format!("error[{}]", deleted_defaults::ID)),
        "negative fixture must produce no deleted_defaults finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn production_and_scaffold_code_do_not_reference_deleted_defaults() {
    let root = workspace_root();
    let mut files = Vec::new();

    push_if_file(&mut files, root.join("Cargo.toml"));
    collect_production_crate_files(&root.join("crates"), &mut files);
    collect_app_files(&root.join("apps"), &mut files);
    collect_files(&root.join("crates/nmp-cli/templates"), &mut files);
    files.sort();
    files.dedup();

    let mut violations = Vec::new();
    for path in files {
        scan_text_file(&root, &path, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "production/scaffold code must treat nmp-defaults as deleted. \
         Do not add compatibility shims, test helpers, default presets, or \
         replacement bundles:\n{}",
        violations.join("\n")
    );
}

#[test]
fn deleted_defaults_scope_covers_production_and_scaffold_code() {
    assert!(deleted_defaults::file_in_scope(Path::new(
        "apps/demo/src/lib.rs"
    )));
    assert!(deleted_defaults::file_in_scope(Path::new(
        "crates/nmp-cli/src/main.rs"
    )));
    assert!(deleted_defaults::file_in_scope(Path::new(
        "crates/nmp-cli/templates/lib.rs.tmpl"
    )));
    assert!(!deleted_defaults::file_in_scope(Path::new(
        "crates/nmp-testing/src/lib.rs"
    )));
}

fn collect_production_crate_files(crates_dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(crates_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == "nmp-testing" || name == "nmp-content-fixtures" {
            continue;
        }
        push_if_file(out, entry.path().join("Cargo.toml"));
        collect_files(&entry.path().join("src"), out);
    }
}

fn collect_app_files(apps_dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(apps_dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        collect_files(&entry.path(), out);
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if matches!(name.as_str(), "target" | ".git" | "tests" | "examples") {
                continue;
            }
            collect_files(&path, out);
        } else if should_scan_text_file(&path) {
            out.push(path);
        }
    }
}

fn push_if_file(out: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() {
        out.push(path);
    }
}

fn should_scan_text_file(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "Cargo.lock" || is_test_like_file(name) {
        return false;
    }
    if name == "Cargo.toml" {
        return true;
    }
    if name.ends_with(".tmpl") {
        return true;
    }
    path.extension().and_then(|ext| ext.to_str()) == Some("rs")
}

fn is_test_like_file(name: &str) -> bool {
    name == "tests.rs"
        || name.ends_with("_tests.rs")
        || name.starts_with("tests_")
        || name.contains("_tests_")
        || name.ends_with("_support.rs")
}

fn scan_text_file(root: &Path, path: &Path, violations: &mut Vec<String>) {
    let body = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    for (idx, line) in body.lines().enumerate() {
        let is_comment = line_is_comment(path, line);
        for (col, message, _) in deleted_defaults::check(line, is_comment, false) {
            violations.push(format!(
                "{}:{}:{} {}",
                path.strip_prefix(root).unwrap_or(path).display(),
                idx + 1,
                col,
                message
            ));
        }
    }
}

fn line_is_comment(path: &Path, line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return true;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "Cargo.toml" || name.ends_with(".toml.tmpl") {
        return trimmed.starts_with('#');
    }
    false
}
