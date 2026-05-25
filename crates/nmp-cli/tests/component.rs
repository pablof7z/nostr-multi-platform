//! End-to-end coverage for `nmp add component`.

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
        path.push(format!("nmp-cli-component-{tag}-{nanos}"));
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
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrQuoteCard.swift")
        .exists());

    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(lock.contains("id = \"swiftui/content-core\""));
    assert!(lock.contains("id = \"swiftui/content-media-grid\""));
    assert!(lock.contains("id = \"swiftui/content-quote-card\""));
    assert!(lock.contains("id = \"swiftui/content-view\""));
    assert!(lock.contains("role = \"example\""));
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
        "nmp add component swiftui/content-minimal failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMinimalContentView.swift")
        .exists());
}
