//! End-to-end coverage for `nmp add component`.
//!
//! This file holds the happy path: builtin-registry installs of each content
//! component, transitive dependency resolution, lock-file ordering, and the
//! duplicate/unknown rejection paths. The registry-seam edge cases (custom
//! filesystem registries, target-file collisions, and the partial-install
//! atomicity gate) live in the sibling `edge_cases` submodule.

mod helpers;

// Registry-seam edge cases (filesystem registries, target collisions, atomic
// rollback) live in a sibling submodule to keep this file under the file-size
// ceiling. They share the same `helpers` primitives via `crate::helpers`.
// `#[path]` keeps the file in a `component/` subdir so cargo does not compile
// it as a standalone integration-test crate.
#[path = "component/edge_cases.rs"]
mod edge_cases;

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
fn add_component_installs_content_kind_registry() {
    let tmp = TempDir::new("kind-registry");

    let out = nmp(
        tmp.path(),
        &["add", "component", "swiftui/content-kind-registry"],
    );
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-kind-registry failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbeddedEvent.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbedHostEnvironment.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-kind-registry\""));
}

#[test]
fn add_component_installs_compose_content_kind_registry() {
    let tmp = TempDir::new("compose-kind-registry");

    let out = nmp(
        tmp.path(),
        &["add", "component", "compose/content-kind-registry"],
    );
    assert!(
        out.status.success(),
        "nmp add component compose/content-kind-registry failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbeddedEvent.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbedKindProjection.kt")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"compose/content-kind-registry\""));
}

#[test]
fn add_component_installs_compose_content_kind_9802() {
    let tmp = TempDir::new("compose-kind-9802");

    let out = nmp(
        tmp.path(),
        &["add", "component", "compose/content-kind-9802"],
    );
    assert!(
        out.status.success(),
        "nmp add component compose/content-kind-9802 failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrHighlightCard.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.kt")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"compose/content-kind-9802\""));
    assert!(lock.contains("id = \"compose/content-kind-registry\""));
}

#[test]
fn add_component_installs_content_view_with_transitive_deps() {
    let tmp = TempDir::new("content-view");

    let out = nmp(
        tmp.path(),
        &[
            "add",
            "component",
            "swiftui/content-view",
            "--with",
            "example",
        ],
    );
    assert!(
        out.status.success(),
        "nmp add component swiftui/content-view failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Direct sources.
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentView.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentGrouping.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/Examples/NostrContentViewPreview.swift")
        .exists());

    // Transitive deps pulled by resolver.
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/ContentTreeWire.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMediaGrid.swift")
        .exists());
    // Event refs now render through the kind-dispatch registry (ADR-0072).
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.swift")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbeddedEvent.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-core\""));
    assert!(lock.contains("id = \"swiftui/content-media-grid\""));
    assert!(lock.contains("id = \"swiftui/content-kind-registry\""));
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
    let core_pos = lock
        .find("id = \"swiftui/content-core\"")
        .expect("content-core must be locked");
    let minimal_pos = lock
        .find("id = \"swiftui/content-minimal\"")
        .expect("content-minimal must be locked");
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
    // Event refs now render through the kind-dispatch registry (ADR-0072).
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrKindRegistry.kt")
        .exists());
    assert!(tmp
        .path()
        .join("Components/NostrContent/EmbeddedEvent.kt")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"compose/content-core\""));
    assert!(lock.contains("id = \"compose/content-media-grid\""));
    assert!(lock.contains("id = \"compose/content-kind-registry\""));
    assert!(lock.contains("id = \"compose/content-view\""));
    assert!(lock.contains("source_sha256 = \""));
}

#[test]
fn add_component_installs_desktop_content_view_with_deps() {
    let tmp = TempDir::new("desktop-content-view");

    let out = nmp(tmp.path(), &["add", "component", "desktop/content-view"]);
    assert!(
        out.status.success(),
        "nmp add component desktop/content-view failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for path in [
        "src/components/nostr_content/content_core.rs",
        "src/components/nostr_content/mention_chip.rs",
        "src/components/nostr_content/media_grid.rs",
        "src/components/nostr_content/quote_card.rs",
        "src/components/nostr_content/content_view.rs",
    ] {
        assert!(tmp.path().join(path).exists(), "{path} must be installed");
    }

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    for id in [
        "desktop/content-core",
        "desktop/content-mention-chip",
        "desktop/content-media-grid",
        "desktop/content-quote-card",
        "desktop/content-view",
    ] {
        assert!(
            lock.contains(&format!("id = \"{id}\"")),
            "{id} must be locked"
        );
    }
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
