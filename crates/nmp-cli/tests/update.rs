//! End-to-end coverage for `nmp update component`.
//!
//! Update semantics: a file is only overwritten when the on-disk content
//! still matches the lock's `source_sha256` baseline. Locally edited files
//! are reported as conflicts and left alone — that promise is the entire
//! reason the lock records `source_sha256` in the first place.

mod helpers;

use helpers::{
    bump_registry_version, nmp, overwrite_registry_file, read_lock_field, sha256_hex_of,
    write_registry, TempDir,
};
use std::fs;

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

// -----------------------------------------------------------------------------
// Edge-case coverage
// -----------------------------------------------------------------------------

/// Deleting an installed file is a user choice that must NOT be silently
/// undone by `nmp update`. The update treats `read_to_string` failures as
/// conflicts so the file is left absent + reported.
///
/// Without this gate, a user who removed a component file for a reason
/// (e.g. cherry-picked just the example out, deleted the rest) would have
/// their work silently re-created the next time they tracked upstream.
#[test]
fn update_reports_conflict_for_deleted_file() {
    let tmp = TempDir::new("deleted");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    let registry = write_registry(
        tmp.path(),
        "0.1.0",
        &[("widget/sample/A.swift", "A.swift", "source")],
    );

    let install = nmp(
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
        install.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    let installed = app.join("A.swift");
    assert!(installed.exists());

    // User deletes the file deliberately.
    fs::remove_file(&installed).unwrap();

    // Upstream still advances.
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
    // Conflicts are non-fatal — exit 0 with a report.
    assert!(
        out.status.success(),
        "update should succeed (conflicts are non-fatal): {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("conflict") && stdout.contains("A.swift"),
        "deleted file must be flagged as a conflict, got stdout: {stdout}"
    );
    assert!(stdout.contains("updated 0"), "stdout: {stdout}");
    assert!(stdout.contains("1 conflicts"), "stdout: {stdout}");

    // File MUST stay deleted — the user's choice was load-bearing.
    assert!(
        !installed.exists(),
        "deleted file must NOT be re-created on update"
    );
}

/// After a clean update (the on-disk file matched the lock's baseline), the
/// lock's `source_sha256` must equal `sha256(new file content)` exactly.
///
/// The previous untouched-refresh test asserts on a constant string; this
/// one asserts the derived invariant directly off whatever is on disk after
/// the update, so a future change to how `update_one_file` writes content
/// (line-ending normalisation, BOM stripping, etc.) trips the gate.
#[test]
fn update_writes_updated_sha256_in_lock() {
    let tmp = TempDir::new("sha256");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    let registry = write_registry(
        tmp.path(),
        "0.1.0",
        &[("widget/sample/A.swift", "A.swift", "source")],
    );

    let install = nmp(
        &app,
        &[
            "add",
            "component",
            "widget/sample",
            "--registry",
            registry.to_str().unwrap(),
        ],
    );
    assert!(install.status.success());

    // Upstream ships a new revision with content the test gets to pick, so
    // the sha is derived from a known string.
    let new_content = "// upstream A — refreshed body\n";
    overwrite_registry_file(&registry, "widget/sample/A.swift", new_content);
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

    // Read what landed on disk and hash it; then read the lock and compare.
    // We deliberately hash the on-disk content (not the upstream string)
    // so the invariant being asserted is "the lock describes what the user
    // would re-hash to detect drift on the NEXT update".
    let on_disk = fs::read_to_string(app.join("A.swift")).unwrap();
    let expected = sha256_hex_of(&on_disk);

    let lock = fs::read_to_string(app.join("nmp.components.lock")).unwrap();
    let hashes = read_lock_field(&lock, "source_sha256");
    assert_eq!(hashes.len(), 1, "expected exactly one source_sha256 entry");
    assert_eq!(
        hashes[0], expected,
        "lock's source_sha256 must match sha256(on-disk content): lock={lock}"
    );
}
