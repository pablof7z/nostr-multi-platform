//! End-to-end: `nmp init` into a tempdir must produce a scaffold that
//! `cargo check`s green, and `nmp gen modules` must succeed and be
//! deterministic on it.

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
        path.push(format!("nmp-cli-{tag}-{nanos}"));
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
fn init_scaffold_compiles_and_gen_is_deterministic() {
    let tmp = TempDir::new("init");
    let root = tmp.path().join("demoapp");

    // 1. Scaffold.
    let out = nmp(
        tmp.path(),
        &["init", "demoapp", "--path", root.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "nmp init failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(root.join("nmp.toml").exists());
    assert!(root.join("crates/demoapp-core/src/lib.rs").exists());
    assert!(root.join("crates/demoapp-core/examples/shell.rs").exists());

    // 2. The scaffold compiles as-is (lib + example + tests).
    let check = Command::new(env!("CARGO"))
        .args(["check", "--all-targets"])
        .current_dir(&root)
        .output()
        .expect("run cargo check");
    assert!(
        check.status.success(),
        "scaffold failed cargo check:\n{}",
        String::from_utf8_lossy(&check.stderr)
    );

    // 3. Skeleton tests pass.
    let test = Command::new(env!("CARGO"))
        .args(["test", "-p", "demoapp-core"])
        .current_dir(&root)
        .output()
        .expect("run cargo test");
    assert!(
        test.status.success(),
        "scaffold tests failed:\n{}",
        String::from_utf8_lossy(&test.stderr)
    );

    // 4. `nmp gen modules` succeeds.
    let gen = nmp(&root, &["gen", "modules"]);
    assert!(
        gen.status.success(),
        "nmp gen modules failed: {}",
        String::from_utf8_lossy(&gen.stderr)
    );
    assert!(root
        .join("apps/demoapp/nmp-app-demoapp/src/lib.rs")
        .exists());

    // 5. Codegen is deterministic.
    let recheck = nmp(&root, &["gen", "modules", "--check"]);
    assert!(
        recheck.status.success(),
        "nmp gen modules --check reported drift: {}",
        String::from_utf8_lossy(&recheck.stderr)
    );
}

#[test]
fn init_rejects_invalid_names() {
    let tmp = TempDir::new("reject");
    for bad in ["Demo", "1app", "my--app", "my_app", "app-"] {
        let out = nmp(
            tmp.path(),
            &[
                "init",
                bad,
                "--path",
                tmp.path().join("x").to_str().unwrap(),
            ],
        );
        assert!(!out.status.success(), "expected `{bad}` to be rejected");
    }
}
