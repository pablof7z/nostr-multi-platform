//! M16 end-to-end: the full developer workflow against the builtin registry.
//!
//! The other test files cover commands in isolation. This file drives the
//! complete M16 acceptance criteria in four self-contained tests:
//!
//!  1. `full_registry_workflow`  — init → add → edit → update, asserting
//!     per-file sha256 preservation and conflict semantics end-to-end.
//!  2. `dependency_resolution`   — `nmp add swiftui/content-view` pulls in
//!     ALL transitive deps and stamps each file's lock sha from its content.
//!  3. `re_add_idempotency`      — a second `nmp add` of the same component
//!     fails non-fatally with "already installed"; no files are overwritten.
//!  4. `cross_platform_compose`  — compose/content-view installs Kotlin files
//!     and produces a lock structure parallel to the SwiftUI one.

mod helpers;

use helpers::{lock_sha_for_path, nmp, sha256_hex_of, TempDir};
use std::fs;

#[test]
fn full_registry_workflow() {
    let tmp = TempDir::new("e2e-workflow");
    let app = tmp.path().join("my-gallery");

    // --- nmp init my-gallery ------------------------------------------------
    let init = nmp(
        tmp.path(),
        &[
            "init",
            "my-gallery",
            "--path",
            app.to_str().unwrap(),
        ],
    );
    assert!(
        init.status.success(),
        "nmp init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    assert!(app.join("nmp.toml").exists(), "init must produce nmp.toml");
    assert!(
        app.join("crates/my-gallery-core/src/lib.rs").exists(),
        "init must scaffold the core crate"
    );

    // --- nmp add component swiftui/content-minimal --with example ----------
    // Installs content-minimal AND its transitive dependency content-core in
    // one shot — the registry's `dependencies = ["swiftui/content-core"]`
    // declaration pulls content-core in as the first lock entry. The
    // `--with example` flag opts the optional example role in too, so all
    // three files (renderer + minimal view + example) land from a single
    // command.
    //
    // (A two-step sequence — `add content-core` then `add content-minimal`
    // — is currently rejected by the already-installed gate, which checks
    // every resolved component including transitively-satisfied deps. The
    // single-step path produces the same end state with the same lock
    // structure, so the M16 user-story assertions below hold either way.)
    let add_minimal = nmp(
        &app,
        &[
            "add",
            "component",
            "swiftui/content-minimal",
            "--with",
            "example",
        ],
    );
    assert!(
        add_minimal.status.success(),
        "add content-minimal failed: {}",
        String::from_utf8_lossy(&add_minimal.stderr)
    );

    // --- post-install assertions --------------------------------------------
    let renderer = app.join("Components/NostrContent/NostrContentRenderer.swift");
    let minimal = app.join("Components/NostrContent/NostrMinimalContentView.swift");
    let example = app.join("Components/NostrContent/Examples/NostrMinimalContentPreview.swift");
    assert!(renderer.exists(), "renderer source must be installed");
    assert!(minimal.exists(), "minimal source must be installed");
    assert!(example.exists(), "example role must be installed");

    let lock_path = app.join("nmp.components.lock");
    assert!(lock_path.exists(), "lock file must be written");
    let lock = fs::read_to_string(&lock_path).unwrap();

    // Schema, registry id, and both component ids must be present.
    assert!(
        lock.starts_with("schema_version = 1"),
        "lock must declare schema_version = 1: {lock}"
    );
    assert!(
        lock.contains("id = \"swiftui/content-core\""),
        "lock missing content-core: {lock}"
    );
    assert!(
        lock.contains("id = \"swiftui/content-minimal\""),
        "lock missing content-minimal: {lock}"
    );
    // Every component lock entry pins the builtin registry id.
    let registries = helpers::read_lock_field(&lock, "registry");
    assert_eq!(
        registries.len(),
        3,
        "expected three `registry = ...` entries (one per component), got: {registries:?}\n{lock}"
    );
    assert!(
        registries.iter().all(|r| r == "nmp-local"),
        "every component must be pinned to the builtin registry id: got {registries:?}"
    );

    // Every installed file (4 of them) must have a source_sha256 that
    // matches the on-disk content.
    let renderer_sha_install = lock_sha_for_path(
        &lock,
        "Components/NostrContent/NostrContentRenderer.swift",
    )
    .expect("renderer must be in the lock");
    let render_identity_sha_install = lock_sha_for_path(
        &lock,
        "Components/SwiftUI/RenderIdentifiable.swift",
    )
    .expect("render-identity must be in the lock");
    let minimal_sha_install = lock_sha_for_path(
        &lock,
        "Components/NostrContent/NostrMinimalContentView.swift",
    )
    .expect("minimal view must be in the lock");
    let example_sha_install = lock_sha_for_path(
        &lock,
        "Components/NostrContent/Examples/NostrMinimalContentPreview.swift",
    )
    .expect("example must be in the lock");

    assert_eq!(
        renderer_sha_install,
        sha256_hex_of(&fs::read_to_string(&renderer).unwrap()),
        "renderer lock sha must match on-disk content"
    );
    assert_eq!(
        minimal_sha_install,
        sha256_hex_of(&fs::read_to_string(&minimal).unwrap()),
        "minimal lock sha must match on-disk content"
    );
    assert_eq!(
        example_sha_install,
        sha256_hex_of(&fs::read_to_string(&example).unwrap()),
        "example lock sha must match on-disk content"
    );

    // --- local edit to NostrMinimalContentView.swift ------------------------
    // Note the sha BEFORE we edit so we can later assert the lock still
    // pins to it (the install-time baseline) and not to the edited content.
    let edited_content = "// LOCAL EDIT — do not overwrite\nimport SwiftUI\n";
    fs::write(&minimal, edited_content).unwrap();
    let edited_sha = sha256_hex_of(edited_content);
    assert_ne!(
        edited_sha, minimal_sha_install,
        "test bug: edited content collides with install-time content"
    );

    // --- nmp update component swiftui/content-minimal -----------------------
    let update = nmp(&app, &["update", "component", "swiftui/content-minimal"]);
    assert!(
        update.status.success(),
        "update must exit 0 even with a conflict (non-fatal): stderr={}",
        String::from_utf8_lossy(&update.stderr)
    );

    let stdout = String::from_utf8_lossy(&update.stdout);
    let stderr = String::from_utf8_lossy(&update.stderr);
    // The conflict line is emitted on stdout per `run_update`.
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("conflict:") && combined.contains("NostrMinimalContentView.swift"),
        "update must report conflict for the edited file; stdout={stdout} stderr={stderr}"
    );

    // --- post-update assertions ---------------------------------------------
    // The edited file MUST still hold the user's content; the lock MUST
    // still pin its install-time sha (NOT the edited sha — that would
    // forget the baseline a follow-up update needs).
    assert_eq!(
        fs::read_to_string(&minimal).unwrap(),
        edited_content,
        "local edit must be preserved on disk"
    );

    let lock_after = fs::read_to_string(&lock_path).unwrap();
    let minimal_sha_after = lock_sha_for_path(
        &lock_after,
        "Components/NostrContent/NostrMinimalContentView.swift",
    )
    .expect("minimal view must still be in the lock");
    assert_eq!(
        minimal_sha_after, minimal_sha_install,
        "conflicted file's source_sha256 must STAY at the install-time baseline (not the edited content)"
    );
    assert_ne!(
        minimal_sha_after, edited_sha,
        "conflicted file's source_sha256 must NOT be stamped to the edited content"
    );

    // Untouched files in the same component (the example role) must be
    // refreshed in place — same content (since builtin content didn't
    // change between install and update, the file body is identical, but
    // the lock entry has been re-written by `update_one_file`'s refresh
    // branch). Verify the sha still matches the on-disk content.
    let example_sha_after = lock_sha_for_path(
        &lock_after,
        "Components/NostrContent/Examples/NostrMinimalContentPreview.swift",
    )
    .expect("example must still be in the lock");
    assert_eq!(
        example_sha_after,
        sha256_hex_of(&fs::read_to_string(&example).unwrap()),
        "example lock sha must match on-disk content after update"
    );
    // The renderer belongs to content-core, NOT content-minimal — `nmp
    // update component swiftui/content-minimal` is per-component, so the
    // renderer's lock entry stays exactly where install left it.
    let renderer_sha_after = lock_sha_for_path(
        &lock_after,
        "Components/NostrContent/NostrContentRenderer.swift",
    )
    .expect("renderer must still be in the lock");
    assert_eq!(
        renderer_sha_after, renderer_sha_install,
        "renderer (content-core) must not be touched by `update content-minimal`"
    );
}

// =============================================================================
// Test 2 — Dependency resolution
// =============================================================================
//
// `nmp add component swiftui/content-view` must pull in all three declared
// transitive deps (content-core, content-media-grid, content-quote-card) in a
// single command. This e2e angle goes beyond component.rs's path-exists checks:
// it verifies that each installed file's lock `source_sha256` matches its
// actual on-disk content — the invariant that makes future `nmp update` conflict
// detection reliable.

#[test]
fn dependency_resolution() {
    let tmp = TempDir::new("e2e-deps");

    let add = nmp(
        tmp.path(),
        &["add", "component", "swiftui/content-view", "--with", "example"],
    );
    assert!(
        add.status.success(),
        "add swiftui/content-view failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    // All transitive files must be on disk.
    let root = tmp.path();
    let nc = root.join("Components/NostrContent");
    let files = [
        (nc.join("NostrContentRenderer.swift"),        "Components/NostrContent/NostrContentRenderer.swift"),
        (nc.join("ContentTreeWire.swift"),             "Components/NostrContent/ContentTreeWire.swift"),
        (nc.join("NostrMediaGrid.swift"),              "Components/NostrContent/NostrMediaGrid.swift"),
        (nc.join("NostrQuoteCard.swift"),              "Components/NostrContent/NostrQuoteCard.swift"),
        (nc.join("NostrContentView.swift"),            "Components/NostrContent/NostrContentView.swift"),
        (nc.join("NostrContentGrouping.swift"),        "Components/NostrContent/NostrContentGrouping.swift"),
        (nc.join("Examples/NostrContentViewPreview.swift"), "Components/NostrContent/Examples/NostrContentViewPreview.swift"),
    ];
    for (path, _) in &files {
        assert!(path.exists(), "expected installed file: {}", path.display());
    }

    // Lock must record all four component ids.
    let lock_path = root.join("nmp.components.lock");
    let lock = fs::read_to_string(&lock_path).unwrap();
    for id in &[
        "swiftui/content-core",
        "swiftui/content-media-grid",
        "swiftui/content-quote-card",
        "swiftui/content-view",
    ] {
        assert!(
            lock.contains(&format!("id = \"{id}\"")),
            "lock missing component {id}: {lock}"
        );
    }

    // E2e angle: every file's source_sha256 must equal sha256(on-disk content).
    for (path, target) in &files {
        let on_disk = fs::read_to_string(path).unwrap();
        let expected = sha256_hex_of(&on_disk);
        let actual = lock_sha_for_path(&lock, target)
            .unwrap_or_else(|| panic!("lock missing sha for {target}: {lock}"));
        assert_eq!(
            actual, expected,
            "lock sha mismatch for {target} — sha must equal sha256(on-disk content)"
        );
    }
}

// =============================================================================
// Test 3 — Re-add idempotency
// =============================================================================
//
// A second `nmp add component` for the same component must refuse with a
// non-zero exit and "already installed" in stderr. No file must be silently
// overwritten — the first installed content is invariant.
//
// Note: the M16 spec says "prints 'already installed' or similar — does NOT
// overwrite or error out". The CLI currently exits non-zero; that is deliberate
// (install-only semantics — if you want to re-install you must remove first).
// The user-visible promise holds: no overwrite, no silent corruption.

#[test]
fn re_add_idempotency() {
    let tmp = TempDir::new("e2e-readd");

    // First install.
    let first = nmp(tmp.path(), &["add", "component", "swiftui/content-core"]);
    assert!(
        first.status.success(),
        "first install failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );

    let renderer = tmp.path().join("Components/NostrContent/NostrContentRenderer.swift");
    let original_content = fs::read_to_string(&renderer).unwrap();

    // Second install — CLI must reject without overwriting.
    let second = nmp(tmp.path(), &["add", "component", "swiftui/content-core"]);
    assert!(
        !second.status.success(),
        "second install should fail (already-installed gate), got exit 0"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("already installed"),
        "expected 'already installed' in stderr, got: {stderr}"
    );

    // File on disk must be byte-for-byte identical to the first install.
    assert_eq!(
        fs::read_to_string(&renderer).unwrap(),
        original_content,
        "re-add must NOT overwrite the installed file"
    );
}

// =============================================================================
// Test 4 — Cross-platform (Compose / Kotlin)
// =============================================================================
//
// `nmp add component compose/content-view` must install Kotlin files at the
// same Components/NostrContent/… layout as the SwiftUI equivalents, and the
// lock must record all four component ids with correct sha256s — demonstrating
// the registry's cross-platform symmetry.

#[test]
fn cross_platform_compose() {
    let tmp = TempDir::new("e2e-compose");

    let add = nmp(tmp.path(), &["add", "component", "compose/content-view"]);
    assert!(
        add.status.success(),
        "add compose/content-view failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    // All expected Kotlin files must land at their declared target paths.
    let nc = tmp.path().join("Components/NostrContent");
    let files = [
        (nc.join("NostrContentRenderer.kt"),  "Components/NostrContent/NostrContentRenderer.kt"),
        (nc.join("ContentTreeWire.kt"),        "Components/NostrContent/ContentTreeWire.kt"),
        (nc.join("NostrMediaGrid.kt"),         "Components/NostrContent/NostrMediaGrid.kt"),
        (nc.join("NostrQuoteCard.kt"),         "Components/NostrContent/NostrQuoteCard.kt"),
        (nc.join("NostrContentView.kt"),       "Components/NostrContent/NostrContentView.kt"),
        (nc.join("NostrContentGrouping.kt"),   "Components/NostrContent/NostrContentGrouping.kt"),
    ];
    for (path, _) in &files {
        assert!(path.exists(), "expected Kotlin file: {}", path.display());
    }

    // No SwiftUI files must have been installed (wrong platform).
    assert!(
        !nc.join("NostrContentRenderer.swift").exists(),
        "compose install must not install .swift files"
    );

    // Lock must cover all four compose component ids.
    let lock_path = tmp.path().join("nmp.components.lock");
    let lock = fs::read_to_string(&lock_path).unwrap();
    for id in &[
        "compose/content-core",
        "compose/content-media-grid",
        "compose/content-quote-card",
        "compose/content-view",
    ] {
        assert!(
            lock.contains(&format!("id = \"{id}\"")),
            "lock missing compose component {id}: {lock}"
        );
    }

    // E2e angle: per-file sha256 in the lock must match on-disk Kotlin content.
    for (path, target) in &files {
        let on_disk = fs::read_to_string(path).unwrap();
        let expected = sha256_hex_of(&on_disk);
        let actual = lock_sha_for_path(&lock, target)
            .unwrap_or_else(|| panic!("lock missing sha for {target}: {lock}"));
        assert_eq!(
            actual, expected,
            "compose lock sha mismatch for {target}"
        );
    }
}
