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
fn add_component_skips_already_installed_dependencies() {
    // Regression: installing a component whose transitive dep is already
    // installed must succeed (the dep is skipped silently) — only the
    // explicitly-requested target may produce an "already installed" error.
    let tmp = TempDir::new("skip-installed-dep");

    // 1. Install the dep on its own.
    let first = nmp(tmp.path(), &["add", "component", "swiftui/content-core"]);
    assert!(
        first.status.success(),
        "first install failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let core_path = tmp
        .path()
        .join("Components/NostrContent/NostrContentRenderer.swift");
    assert!(core_path.exists());

    // 2. Drop a sentinel into the installed file. If the second invocation
    //    were to re-install content-core (either by rewriting it or by
    //    tripping the "target file already exists" guard), this sentinel
    //    would be lost or the command would fail.
    let sentinel = "// SENTINEL — must survive content-minimal install\n";
    fs::write(&core_path, sentinel).unwrap();

    // 3. Install the dependent component. content-core is already installed,
    //    so the resolver returns [content-core, content-minimal]; only
    //    content-minimal should actually be written.
    let second = nmp(tmp.path(), &["add", "component", "swiftui/content-minimal"]);
    assert!(
        second.status.success(),
        "second install failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    // 4. The dep's file is untouched (sentinel survived).
    let core_contents = fs::read_to_string(&core_path).unwrap();
    assert_eq!(
        core_contents, sentinel,
        "content-core was re-installed even though it was already present"
    );

    // 5. The requested component's files were installed.
    assert!(tmp
        .path()
        .join("Components/NostrContent/NostrMinimalContentView.swift")
        .exists());

    // 6. The lock file has both components recorded.
    let lock = fs::read_to_string(tmp.path().join("nmp.components.lock")).unwrap();
    assert!(
        lock.contains("id = \"swiftui/content-core\""),
        "lock missing content-core: {lock}"
    );
    assert!(
        lock.contains("id = \"swiftui/content-minimal\""),
        "lock missing content-minimal: {lock}"
    );
}
