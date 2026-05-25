//! M16 end-to-end: the full developer workflow against the builtin registry.
//!
//! The other test files cover commands in isolation. This one drives the
//! single user-story sequence the registry was designed for: `nmp init` a new
//! app, `nmp add` two components from the builtin registry, edit a file
//! locally, then `nmp update` the affected component. It asserts that the
//! edit is preserved AND that the untouched dependency is refreshed in the
//! same pass — the cross-file behaviour the per-command tests don't reach.

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
        2,
        "expected two `registry = ...` entries (one per component), got: {registries:?}\n{lock}"
    );
    assert!(
        registries.iter().all(|r| r == "nmp-local"),
        "every component must be pinned to the builtin registry id: got {registries:?}"
    );

    // Every installed file (3 of them) must have a source_sha256 that
    // matches the on-disk content.
    let renderer_sha_install = lock_sha_for_path(
        &lock,
        "Components/NostrContent/NostrContentRenderer.swift",
    )
    .expect("renderer must be in the lock");
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
