//! Integration tests for `nmp export jsrepo`.
//!
//! The key scenario is a drift-detection test: if someone adds a component to
//! `registry.toml` and forgets to regenerate the committed `registry.json`,
//! this test surfaces the divergence in CI before the broken registry ships.

mod helpers;

use helpers::{nmp, TempDir};
use std::fs;

/// The canonical, committed registry.json lives here relative to the workspace
/// root. We discover the workspace root by walking up from CARGO_MANIFEST_DIR.
fn workspace_root() -> std::path::PathBuf {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // CARGO_MANIFEST_DIR is `crates/nmp-cli`; workspace root is two levels up.
    manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root must be two levels above CARGO_MANIFEST_DIR")
        .to_path_buf()
}

/// `nmp export jsrepo` must succeed and produce `registry.json` + per-item
/// files in `r/` inside the output directory.
#[test]
fn export_jsrepo_produces_registry_json_and_per_item_files() {
    let tmp = TempDir::new("export-jsrepo");
    let registry_dir = workspace_root().join("crates/nmp-cli/registry");

    let out = nmp(
        tmp.path(),
        &[
            "export",
            "jsrepo",
            "--registry",
            registry_dir.to_str().unwrap(),
            "--output",
            tmp.path().to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "nmp export jsrepo failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let json_path = tmp.path().join("registry.json");
    assert!(json_path.exists(), "registry.json must be emitted");

    let json = fs::read_to_string(&json_path).unwrap();
    // Schema and top-level fields.
    assert!(
        json.contains("\"$schema\""),
        "must contain $schema field"
    );
    assert!(json.contains("nmpui.f7z.io"), "must reference the production URL");
    assert!(
        json.contains("swiftui-content-core"),
        "must contain swiftui-content-core item"
    );
    assert!(
        json.contains("compose-content-core"),
        "must contain compose-content-core item"
    );

    // Each source path must use the `registry/` prefix.
    assert!(
        json.contains("\"path\": \"registry/swiftui"),
        "file path must start with registry/"
    );

    // File content must be inlined.
    assert!(
        json.contains("\"content\":"),
        "files must include inlined content"
    );

    // Per-item files.
    assert!(
        tmp.path().join("r/swiftui-content-core.json").exists(),
        "r/swiftui-content-core.json must be emitted"
    );
    assert!(
        tmp.path().join("r/swiftui-content-view.json").exists(),
        "r/swiftui-content-view.json must be emitted"
    );
    assert!(
        tmp.path().join("r/compose-content-view.json").exists(),
        "r/compose-content-view.json must be emitted"
    );

    // registryDependencies must be slug-converted from component deps.
    let core_item = fs::read_to_string(tmp.path().join("r/swiftui-content-minimal.json")).unwrap();
    assert!(
        core_item.contains("swiftui-content-core"),
        "registryDependencies must use slug form (swiftui-content-core): {core_item}"
    );
}

/// Drift-detection: the committed `web/registry/public/registry.json` must
/// match what `nmp export jsrepo` would generate from the current manifest.
///
/// Fails when someone adds a component to `registry.toml` without regenerating
/// the committed JSON. Fix: run `nmp export jsrepo --output web/registry/public`.
#[test]
fn committed_registry_json_matches_generated_output() {
    let root = workspace_root();
    let registry_dir = root.join("crates/nmp-cli/registry");
    let committed = root.join("web/registry/public/registry.json");

    assert!(
        committed.exists(),
        "web/registry/public/registry.json must be committed; run: \
         nmp export jsrepo --output web/registry/public"
    );

    let tmp = TempDir::new("export-drift");
    let out = nmp(
        tmp.path(),
        &[
            "export",
            "jsrepo",
            "--registry",
            registry_dir.to_str().unwrap(),
            "--output",
            tmp.path().to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "nmp export jsrepo failed during drift check: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let generated = fs::read_to_string(tmp.path().join("registry.json")).unwrap();
    let on_disk = fs::read_to_string(&committed).unwrap();

    assert_eq!(
        generated, on_disk,
        "web/registry/public/registry.json is stale.\n\
         Run: cargo run -p nmp-cli --bin nmp -- export jsrepo \\\n\
           --registry crates/nmp-cli/registry \\\n\
           --output web/registry/public"
    );
}

/// The `nmp export` subcommand must fail gracefully when given an unknown
/// target (no panics, exit code 1 with a helpful message).
#[test]
fn export_unknown_target_fails_with_usage() {
    let tmp = TempDir::new("export-unknown");
    let out = nmp(tmp.path(), &["export", "unknown-format"]);
    assert!(
        !out.status.success(),
        "unknown export target must fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown export target"),
        "must name the bad target: {stderr}"
    );
}
