//! Smoke tests for the no-deprecated ratchet (#2770).

use std::path::{Path, PathBuf};

use super::{run_lint, workspace_root};

#[path = "rules/no_deprecated.rs"]
mod no_deprecated;

#[test]
fn no_deprecated_positive_fixture_fires() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_no_deprecated_pos")
        .join("crates")
        .join("nmp-native-runtime")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_deprecated_pos"),
    );
    std::fs::create_dir_all(&tmp).expect("create fake nmp-native-runtime src dir");
    std::fs::write(
        tmp.join("lib.rs"),
        format!(
            "{}(note = \"use new_api\")]\npub fn old_api() {{}}\n",
            deprecated_prefix()
        ),
    )
    .expect("write positive fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 1,
        "no_deprecated positive must exit 1; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        stdout.contains(&format!("error[{}]", no_deprecated::ID)),
        "positive fixture must emit no_deprecated finding; stdout:\n{}",
        stdout
    );
    assert!(stdout.contains(&deprecated_attr_display()));
}

#[test]
fn no_deprecated_negative_fixture_is_clean() {
    let workspace = workspace_root();
    let tmp = workspace
        .join("target")
        .join("doctrine_lint_no_deprecated_neg")
        .join("crates")
        .join("nmp-native-runtime")
        .join("src");
    let _ = std::fs::remove_dir_all(
        workspace
            .join("target")
            .join("doctrine_lint_no_deprecated_neg"),
    );
    std::fs::create_dir_all(&tmp).expect("create fake nmp-native-runtime src dir");
    std::fs::write(
        tmp.join("lib.rs"),
        format!(
            "pub fn current_api() {{}}\n\n// {}(note = \"commented example\")]\n",
            deprecated_prefix()
        ),
    )
    .expect("write negative fixture");

    let tmp_str = tmp.to_string_lossy().into_owned();
    let (code, stdout, stderr) = run_lint(&["--path", &tmp_str]);
    assert_eq!(
        code, 0,
        "no_deprecated negative must exit 0; stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
    assert!(
        !stdout.contains(&format!("error[{}]", no_deprecated::ID)),
        "negative fixture must produce no no_deprecated finding; stdout:\n{}",
        stdout
    );
}

#[test]
fn workspace_source_has_no_deprecated_attributes() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_source_files(&root.join("crates"), &mut files);
    collect_source_files(&root.join("apps"), &mut files);
    files.sort();
    files.dedup();

    let mut violations = Vec::new();
    for path in files {
        scan_for_deprecated_attribute(&root, &path, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "workspace source must not carry deprecated compatibility surfaces:\n{}",
        violations.join("\n")
    );
}

#[test]
fn workspace_source_has_no_deleted_observed_feed_source_doorway() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_source_files(&root.join("crates"), &mut files);
    collect_source_files(&root.join("apps"), &mut files);
    files.sort();
    files.dedup();

    let banned = ["open_observed", "_feed_source"].concat();
    let mut violations = Vec::new();
    for path in files {
        scan_for_banned_token(&root, &path, &banned, &mut violations);
    }

    assert!(
        violations.is_empty(),
        "deleted observed-feed-source doorway must not reland:\n{}",
        violations.join("\n")
    );
}

#[test]
fn no_deprecated_scope_covers_workspace_source() {
    assert!(no_deprecated::file_in_scope(Path::new(
        "crates/nmp-native-runtime/src/lib.rs"
    )));
    assert!(no_deprecated::file_in_scope(Path::new(
        "apps/demo/src/lib.rs"
    )));
    assert!(!no_deprecated::file_in_scope(Path::new(
        "crates/nmp-core/src/transport/generated/nmp_update_generated.rs"
    )));
}

fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) {
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
            if matches!(name.as_str(), "target" | ".git" | "fixtures" | "generated") {
                continue;
            }
            collect_source_files(&path, out);
        } else if should_scan(&path) {
            out.push(path);
        }
    }
}

fn should_scan(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    path.extension().and_then(|ext| ext.to_str()) == Some("rs") && !name.ends_with("_generated.rs")
}

fn scan_for_deprecated_attribute(root: &Path, path: &Path, violations: &mut Vec<String>) {
    let body = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    for (idx, line) in body.lines().enumerate() {
        if line.contains(&deprecated_prefix()) {
            violations.push(format!(
                "{}:{} contains {}",
                path.strip_prefix(root).unwrap_or(path).display(),
                idx + 1,
                deprecated_prefix()
            ));
        }
    }
}

fn scan_for_banned_token(root: &Path, path: &Path, token: &str, violations: &mut Vec<String>) {
    let body = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
    for (idx, line) in body.lines().enumerate() {
        if line.contains(token) {
            violations.push(format!(
                "{}:{} contains {}",
                path.strip_prefix(root).unwrap_or(path).display(),
                idx + 1,
                token
            ));
        }
    }
}

fn deprecated_prefix() -> String {
    ["#[", "deprecated"].concat()
}

fn deprecated_attr_display() -> String {
    ["#[", "deprecated]"].concat()
}
