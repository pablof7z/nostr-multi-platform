//! Edge-case coverage for `nmp add component`.
//!
//! These tests exercise the seams the happy-path install tests in the parent
//! `component.rs` can't reach — custom filesystem registries pointed at via
//! `--registry`, target-file collisions, and the atomicity gate that keeps a
//! failed install from leaving a partial lock entry behind.

use crate::helpers::{nmp, TempDir};
use std::fs;

/// A custom filesystem registry pointed to via `--registry` must take
/// precedence over the builtin and install its declared files unchanged.
///
/// This is the load-bearing test for the `--registry` flag: without it, a
/// user wiring up an in-house registry has no integration coverage that the
/// CLI ever consults the on-disk manifest.
#[test]
fn add_component_with_filesystem_registry() {
    let tmp = TempDir::new("fs-registry");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    // Write a registry root with one component + one file. Use a registry_id
    // (`fs-test-registry`) that is distinct from both the builtin
    // (`nmp-local`) and the shared helper (`test-registry`) so the lock
    // assertion below can only pass if the CLI read THIS manifest.
    let registry_dir = tmp.path().join("registry");
    fs::create_dir_all(registry_dir.join("widget")).unwrap();

    fs::write(
        registry_dir.join("registry.toml"),
        "schema_version = 1\n\
         registry_id = \"fs-test-registry\"\n\
         \n\
         [[components]]\n\
         id = \"widget/custom\"\n\
         version = \"0.1.0\"\n\
         target = \"swiftui\"\n\
         description = \"custom\"\n\
         \n\
         [[components.files]]\n\
         source = \"widget/custom/Renderer.swift\"\n\
         target = \"Components/Custom/Renderer.swift\"\n\
         role = \"source\"\n",
    )
    .unwrap();
    let upstream_content = "// custom registry source v1\n";
    fs::create_dir_all(registry_dir.join("widget/custom")).unwrap();
    fs::write(
        registry_dir.join("widget/custom/Renderer.swift"),
        upstream_content,
    )
    .unwrap();

    let out = nmp(
        &app,
        &[
            "add",
            "component",
            "widget/custom",
            "--registry",
            registry_dir.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "install from fs registry failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let installed = app.join("Components/Custom/Renderer.swift");
    assert_eq!(fs::read_to_string(&installed).unwrap(), upstream_content);

    let lock = fs::read_to_string(app.join("nmp.components.lock")).unwrap();
    assert!(
        lock.contains("registry = \"fs-test-registry\""),
        "lock should pin registry id: {lock}"
    );
    assert!(lock.contains("id = \"widget/custom\""), "{lock}");
}

/// `nmp add component` is install-only — it never claims authority to
/// overwrite a file the user already authored. Pre-creating any of the
/// component's target paths must abort the install with a clear error so the
/// user can either move the existing file out of the way or skip the install.
#[test]
fn add_component_rejects_preexisting_target_file() {
    let tmp = TempDir::new("preexisting");
    let target_dir = tmp.path().join("Components/NostrContent");
    fs::create_dir_all(&target_dir).unwrap();
    fs::write(
        target_dir.join("NostrContentRenderer.swift"),
        "// my own file, don't touch\n",
    )
    .unwrap();

    let out = nmp(tmp.path(), &["add", "component", "swiftui/content-core"]);
    assert!(
        !out.status.success(),
        "install must fail when target exists"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("already exists"),
        "expected `already exists` in stderr, got: {stderr}"
    );

    // The user's file is untouched.
    assert_eq!(
        fs::read_to_string(target_dir.join("NostrContentRenderer.swift")).unwrap(),
        "// my own file, don't touch\n"
    );
    // And the lock never came into being, since plan_files runs before any
    // write.
    assert!(
        !tmp.path().join("nmp.components.lock").exists(),
        "lock must not be written when install aborts"
    );
}

/// If any source file declared in the registry manifest is missing on disk,
/// the install must fail BEFORE writing anything — no partial lock entry, no
/// half-installed file tree. `plan_files` reads every source first and only
/// then calls `write_files` / `write_lock_entries`, so this test is the
/// regression gate for that ordering.
#[test]
fn lock_file_survives_partial_install() {
    let tmp = TempDir::new("partial");
    let app = tmp.path().join("app");
    fs::create_dir_all(&app).unwrap();

    // Hand-write a registry that names two files but only ships one on disk.
    let registry_dir = tmp.path().join("registry");
    fs::create_dir_all(registry_dir.join("widget/broken")).unwrap();
    fs::write(
        registry_dir.join("registry.toml"),
        "schema_version = 1\n\
         registry_id = \"broken-registry\"\n\
         \n\
         [[components]]\n\
         id = \"widget/broken\"\n\
         version = \"0.1.0\"\n\
         target = \"swiftui\"\n\
         description = \"broken\"\n\
         \n\
         [[components.files]]\n\
         source = \"widget/broken/A.swift\"\n\
         target = \"A.swift\"\n\
         role = \"source\"\n\
         \n\
         [[components.files]]\n\
         source = \"widget/broken/MISSING.swift\"\n\
         target = \"MISSING.swift\"\n\
         role = \"source\"\n",
    )
    .unwrap();
    fs::write(
        registry_dir.join("widget/broken/A.swift"),
        "// upstream A\n",
    )
    .unwrap();
    // widget/broken/MISSING.swift intentionally not written.

    let out = nmp(
        &app,
        &[
            "add",
            "component",
            "widget/broken",
            "--registry",
            registry_dir.to_str().unwrap(),
        ],
    );
    assert!(
        !out.status.success(),
        "install with missing source must fail"
    );

    // No partial install on disk:
    assert!(!app.join("A.swift").exists(), "A.swift must not be written");
    assert!(
        !app.join("MISSING.swift").exists(),
        "MISSING.swift must not be written"
    );
    // No lock file at all — plan failed before write_lock_entries:
    assert!(
        !app.join("nmp.components.lock").exists(),
        "lock must not be written when planning fails"
    );
}
