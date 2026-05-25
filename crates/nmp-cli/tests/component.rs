//! End-to-end coverage for `nmp add component`.
//!
//! The happy path (builtin registry, single-component install with deps and
//! optional roles) lives at the top; the edge-case block at the bottom
//! exercises the seams that the happy path can't reach — custom filesystem
//! registries, dependency ordering in the lock, target-file collisions, and
//! the atomicity gate that keeps a failed install from leaving a partial
//! lock entry behind.

mod helpers;

use helpers::{nmp, TempDir};
use std::fs;

#[test]
fn add_component_installs_dependencies_optional_roles_and_lock() {
    let tmp = TempDir::new("install");

    let out = nmp(
        tmp.path(),
        &[
            "add",
            "component",
            "swiftui/content-minimal",
            "--with",
            "example",
        ],
    );
    assert!(
        out.status.success(),
        "nmp add component failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMinimalContentView.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/Examples/NostrMinimalContentPreview.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-core\""));
    assert!(lock.contains("id = \"swiftui/content-minimal\""));
    assert!(lock.contains("role = \"example\""));
    assert!(lock.contains("source_sha256 = \""));
}

#[test]
fn add_component_rejects_duplicate_installs() {
    let tmp = TempDir::new("duplicate");

    let first = nmp(tmp.path(), &["add", "component", "swiftui/content-minimal"]);
    assert!(first.status.success());

    let second = nmp(tmp.path(), &["add", "component", "swiftui/content-minimal"]);
    assert!(!second.status.success());
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("already installed"), "{stderr}");
}

#[test]
fn add_component_rejects_unknown_component() {
    let tmp = TempDir::new("unknown");

    let out = nmp(tmp.path(), &["add", "component", "swiftui/does-not-exist"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("unknown component"), "{stderr}");
}

// -----------------------------------------------------------------------------
// Edge-case coverage
// -----------------------------------------------------------------------------

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

#[test]
fn add_component_installs_content_core_with_wire_mirror() {
    let tmp = TempDir::new("content-core");

    let out = nmp(tmp.path(), &["add", "component", "swiftui/content-core"]);
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-core failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/ContentTreeWire.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-core\""));
    assert!(lock.contains("version = \"0.2.0\""));
    assert!(lock.contains("ContentTreeWire.swift"));
}

#[test]
fn add_component_installs_content_mention_chip() {
    let tmp = TempDir::new("mention-chip");

    let out = nmp(
        tmp.path(),
        &["add", "component", "swiftui/content-mention-chip"],
    );
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-mention-chip failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMentionChip.swift")
        .exists());
    // Dependency was pulled in.
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/ContentTreeWire.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-core\""));
    assert!(lock.contains("id = \"swiftui/content-mention-chip\""));
}

#[test]
fn add_component_installs_content_media_grid() {
    let tmp = TempDir::new("media-grid");

    let out = nmp(
        tmp.path(),
        &["add", "component", "swiftui/content-media-grid"],
    );
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-media-grid failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMediaGrid.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-media-grid\""));
}

#[test]
fn add_component_installs_content_quote_card() {
    let tmp = TempDir::new("quote-card");

    let out = nmp(
        tmp.path(),
        &["add", "component", "swiftui/content-quote-card"],
    );
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-quote-card failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrQuoteCard.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-quote-card\""));
}

#[test]
fn add_component_installs_content_view_with_transitive_deps() {
    let tmp = TempDir::new("content-view");

    let out = nmp(
        tmp.path(),
        &["add", "component", "swiftui/content-view", "--with", "example"],
    );
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-view failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Direct sources.
    assert!(tmp.path().join("Components/NostrContent/NostrContentView.swift").exists());
    assert!(tmp.path().join("Components/NostrContent/NostrContentGrouping.swift").exists());
    assert!(tmp.path().join("Components/NostrContent/Examples/NostrContentViewPreview.swift").exists());

    // Transitive deps pulled by resolver.
    assert!(tmp.path().join("Components/NostrContent/NostrContentRenderer.swift").exists());
    assert!(tmp.path().join("Components/NostrContent/ContentTreeWire.swift").exists());
    assert!(tmp.path().join("Components/NostrContent/NostrMediaGrid.swift").exists());
    assert!(tmp.path().join("Components/NostrContent/NostrQuoteCard.swift").exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-core\""));
    assert!(lock.contains("id = \"swiftui/content-media-grid\""));
    assert!(lock.contains("id = \"swiftui/content-quote-card\""));
    assert!(lock.contains("id = \"swiftui/content-view\""));
    assert!(lock.contains("role = \"example\""));
    assert!(lock.contains("source_sha256 = \""));
}

/// Installing a component must lock its transitive dependencies BEFORE the
/// requested component itself.
#[test]
fn add_component_dependency_order() {
    let tmp = TempDir::new("dep-order");
    let out = nmp(tmp.path(), &["add", "component", "swiftui/content-minimal"]);
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-minimal failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    let core_pos = lock.find("id = \"swiftui/content-core\"").expect("content-core must be locked");
    let minimal_pos = lock.find("id = \"swiftui/content-minimal\"").expect("content-minimal must be locked");
    assert!(
        core_pos < minimal_pos,
        "content-core must appear before content-minimal: core@{core_pos}, minimal@{minimal_pos}"
    );
}

#[test]
fn add_component_installs_compose_content_core() {
    let tmp = TempDir::new("compose-content-core");

    let out = nmp(tmp.path(), &["add", "component", "compose/content-core"]);
    assert!(
        out.status.success(),
        "nmp add component compose/content-core failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/ContentTreeWire.kt")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"compose/content-core\""));
    assert!(lock.contains("ContentTreeWire.kt"));
    assert!(lock.contains("NostrContentRenderer.kt"));
}

#[test]
fn add_component_installs_compose_content_view_with_deps() {
    let tmp = TempDir::new("compose-content-view");

    let out = nmp(tmp.path(), &["add", "component", "compose/content-view"]);
    assert!(
        out.status.success(),
        "nmp add component compose/content-view failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Direct sources.
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentView.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentGrouping.kt")
        .exists());

    // Transitive dependencies pulled by the resolver.
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/ContentTreeWire.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMediaGrid.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrQuoteCard.kt")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"compose/content-core\""));
    assert!(lock.contains("id = \"compose/content-media-grid\""));
    assert!(lock.contains("id = \"compose/content-quote-card\""));
    assert!(lock.contains("id = \"compose/content-view\""));
    assert!(lock.contains("source_sha256 = \""));
}

/// The previous toy `swiftui/content-minimal` must remain installable so apps
/// that adopted it keep working.
#[test]
fn add_component_keeps_content_minimal_installable() {
    let tmp = TempDir::new("content-minimal-still-works");

    let out = nmp(tmp.path(), &["add", "component", "swiftui/content-minimal"]);
    assert!(
        out.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    let core_pos = lock
        .find("id = \"swiftui/content-core\"")
        .expect("content-core must be locked");
    let minimal_pos = lock
        .find("id = \"swiftui/content-minimal\"")
        .expect("content-minimal must be locked");
    assert!(
        core_pos < minimal_pos,
        "content-core must appear before content-minimal in the lock — got core@{core_pos}, minimal@{minimal_pos}\n{lock}"
    );
}

/// `nmp add component` is install-only — it never claims authority to
/// overwrite a file the user already authored. Pre-creating any of the
/// component's target paths must abort the install with a clear error so the
/// user can either move the existing file out of the way or skip the install.
#[test]
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
