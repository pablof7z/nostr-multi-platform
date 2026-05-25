//! End-to-end coverage for `nmp update component`.
//!
//! Update semantics: a file is only overwritten when the on-disk content
//! still matches the lock's `source_sha256` baseline. Locally edited files
//! are reported as conflicts and left alone — that promise is the entire
//! reason the lock records `source_sha256` in the first place.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const NMP: &str = env!("CARGO_BIN_EXE_nmp");

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("nmp-cli-update-{tag}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn nmp(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(NMP)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run nmp")
}

/// Write a small synthetic registry the update test can mutate freely.
/// The shape mirrors `crates/nmp-cli/registry/registry.toml` but stays
/// out-of-tree so a test can change the "upstream" source content and
/// version between install and update.
fn write_registry(root: &Path, version: &str, files: &[(&str, &str, &str)]) -> PathBuf {
    let registry_dir = root.join("registry");
    fs::create_dir_all(&registry_dir).unwrap();
    fs::create_dir_all(registry_dir.join("widget")).unwrap();

    let mut manifest = String::from("schema_version = 1\nregistry_id = \"test-registry\"\n\n");
    manifest.push_str("[[components]]\n");
    manifest.push_str("id = \"widget/sample\"\n");
    manifest.push_str(&format!("version = \"{version}\"\n"));
    manifest.push_str("target = \"swiftui\"\n");
    manifest.push_str("description = \"test component\"\n");
    for (source, target, role) in files {
        manifest.push_str("\n[[components.files]]\n");
        manifest.push_str(&format!("source = \"{source}\"\n"));
        manifest.push_str(&format!("target = \"{target}\"\n"));
        manifest.push_str(&format!("role = \"{role}\"\n"));
    }
    fs::write(registry_dir.join("registry.toml"), manifest).unwrap();

    for (source, _target, _role) in files {
        let source_path = registry_dir.join(source);
        if let Some(parent) = source_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // Initial content is the source path itself — a deterministic
        // baseline the test can flip to a new value to simulate upstream
        // changing.
        fs::write(source_path, format!("// upstream v{version}: {source}\n")).unwrap();
    }
    registry_dir
}

fn overwrite_registry_file(registry_dir: &Path, source: &str, content: &str) {
    fs::write(registry_dir.join(source), content).unwrap();
}

fn bump_registry_version(registry_dir: &Path, new_version: &str) {
    let manifest_path = registry_dir.join("registry.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    // Replace the first `version = "..."` line — the test manifest only
    // contains one component, so this is unambiguous.
    let mut out = String::new();
    let mut replaced = false;
    for line in manifest.lines() {
        if !replaced && line.starts_with("version = ") {
            out.push_str(&format!("version = \"{new_version}\"\n"));
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    fs::write(&manifest_path, out).unwrap();
}

fn read_lock_field(lock: &str, key: &str) -> Vec<String> {
    lock.lines()
        .filter_map(|line| {
            let line = line.trim();
            let prefix = format!("{key} = \"");
            line.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix('"'))
                .map(ToOwned::to_owned)
        })
        .collect()
}

#[test]
fn update_overwrites_untouched_file_and_refreshes_lock() {
    let tmp = TempDir::new("untouched");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    let registry = write_registry(
        tmp.path(),
        "0.1.0",
        &[("widget/sample/A.swift", "A.swift", "source")],
    );

    let out = nmp(
        &app,
        &[
            "add",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let installed = app.join("A.swift");
    let original_content = fs::read_to_string(&installed).unwrap();

    // Upstream ships a new revision.
    overwrite_registry_file(&registry, "widget/sample/A.swift", "// upstream v2\n");
    bump_registry_version(&registry, "0.2.0");

    let out = nmp(
        &app,
        &[
            "update",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "update failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("updated 1"), "stdout: {stdout}");
    assert!(stdout.contains("0 conflicts"), "stdout: {stdout}");

    let new_content = fs::read_to_string(&installed).unwrap();
    assert_eq!(new_content, "// upstream v2\n");
    assert_ne!(new_content, original_content);

    let lock = fs::read_to_string(app.join("nmp.components.lock")).unwrap();
    assert!(
        lock.contains("version = \"0.2.0\""),
        "lock did not refresh version: {lock}"
    );
    // The lock's source_sha256 must reflect the new content's hash, not the old one.
    let hashes = read_lock_field(&lock, "source_sha256");
    assert_eq!(hashes.len(), 1);
    let expected = sha256_hex_of("// upstream v2\n");
    assert_eq!(hashes[0], expected, "lock did not refresh hash: {lock}");
}

#[test]
fn update_preserves_locally_edited_file_and_keeps_old_lock_entry() {
    let tmp = TempDir::new("conflict");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    let registry = write_registry(
        tmp.path(),
        "0.1.0",
        &[("widget/sample/A.swift", "A.swift", "source")],
    );

    let out = nmp(
        &app,
        &[
            "add",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());

    let lock_before = fs::read_to_string(app.join("nmp.components.lock")).unwrap();
    let hash_before = read_lock_field(&lock_before, "source_sha256")[0].clone();

    // The user edited the file locally — update must not stomp on this.
    let installed = app.join("A.swift");
    let user_content = "// my local tweak\n";
    fs::write(&installed, user_content).unwrap();

    // Upstream also moved on, but the conflict gate fires before any
    // version bump can land on the lock.
    overwrite_registry_file(&registry, "widget/sample/A.swift", "// upstream v2\n");
    bump_registry_version(&registry, "0.2.0");

    let out = nmp(
        &app,
        &[
            "update",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "update should succeed with conflict report, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("conflict") && stdout.contains("A.swift"),
        "expected conflict line, got: {stdout}"
    );
    assert!(stdout.contains("updated 0"), "stdout: {stdout}");
    assert!(stdout.contains("1 conflicts"), "stdout: {stdout}");

    // File on disk untouched.
    assert_eq!(fs::read_to_string(&installed).unwrap(), user_content);

    // Lock's source_sha256 for this file must stay at the install-time hash:
    // it still reflects what the user diverged from. Stamping it to the
    // current on-disk hash would forget the baseline a future `nmp update`
    // needs to detect resolution.
    let lock_after = fs::read_to_string(app.join("nmp.components.lock")).unwrap();
    let hash_after = read_lock_field(&lock_after, "source_sha256")[0].clone();
    assert_eq!(
        hash_after, hash_before,
        "conflicted file's source_sha256 must not change"
    );
    // Version tracks "what upstream rev are we tracking" at the component
    // level; per-file divergence is captured via the per-file hash. So
    // version DOES advance even with outstanding conflicts.
    assert!(
        lock_after.contains("version = \"0.2.0\""),
        "version should track the registry rev: {lock_after}"
    );
}

#[test]
fn update_with_mixed_files_handles_each_independently() {
    let tmp = TempDir::new("mixed");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    let registry = write_registry(
        tmp.path(),
        "0.1.0",
        &[
            ("widget/sample/A.swift", "A.swift", "source"),
            ("widget/sample/B.swift", "B.swift", "source"),
        ],
    );

    let out = nmp(
        &app,
        &[
            "add",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());

    // User edits A; B stays pristine.
    fs::write(app.join("A.swift"), "// edited A\n").unwrap();

    // Upstream advances both files.
    overwrite_registry_file(&registry, "widget/sample/A.swift", "// upstream A v2\n");
    overwrite_registry_file(&registry, "widget/sample/B.swift", "// upstream B v2\n");
    bump_registry_version(&registry, "0.2.0");

    let out = nmp(
        &app,
        &[
            "update",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("conflict") && stdout.contains("A.swift"),
        "expected A conflict: {stdout}"
    );
    assert!(stdout.contains("updated 1"), "stdout: {stdout}");
    assert!(stdout.contains("1 conflicts"), "stdout: {stdout}");

    // A untouched.
    assert_eq!(fs::read_to_string(app.join("A.swift")).unwrap(), "// edited A\n");
    // B refreshed.
    assert_eq!(
        fs::read_to_string(app.join("B.swift")).unwrap(),
        "// upstream B v2\n"
    );

    // Version always tracks the registry rev — per-file divergence is
    // captured by per-file hashes, not by pinning component version.
    let lock = fs::read_to_string(app.join("nmp.components.lock")).unwrap();
    assert!(
        lock.contains("version = \"0.2.0\""),
        "version should track the registry rev: {lock}"
    );
}

#[test]
fn update_rejects_unknown_component() {
    let tmp = TempDir::new("unknown");

    let out = nmp(tmp.path(), &["update", "component", "swiftui/never-installed"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is not installed"),
        "expected uninstalled-component error, got: {stderr}"
    );
}

fn sha256_hex_of(content: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
