//! Shared test helpers for the `nmp` CLI integration suite.
//!
//! Every test file in `crates/nmp-cli/tests/` wires its scenarios up the same
//! way: spawn the compiled `nmp` binary against an isolated tempdir, optionally
//! point it at a synthetic registry on disk, then read back the produced
//! `nmp.components.lock`. Centralising those primitives keeps drift between
//! files (different tag prefixes, slightly different `Command` wiring) from
//! re-accumulating.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Absolute path to the `nmp` binary cargo built for this test crate.
pub const NMP: &str = env!("CARGO_BIN_EXE_nmp");

/// Filesystem-isolated scratch directory; removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    /// Create a uniquely-named tempdir tagged with `tag` (helps when triaging
    /// a leak — the tag tells you which test left the directory behind).
    pub fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("nmp-cli-{tag}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Run the `nmp` binary in `cwd` with the given args, returning the raw
/// `Output`. Tests inspect status / stdout / stderr directly so they can
/// assert on user-visible messaging.
pub fn nmp(cwd: &Path, args: &[&str]) -> Output {
    Command::new(NMP)
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run nmp")
}

/// Synthetic registry builder used by the `update` and `add` edge-case tests.
///
/// Mirrors the on-disk shape of `crates/nmp-cli/registry/registry.toml` but
/// stays out-of-tree so a test can flip the "upstream" content + version
/// between install and update. The returned path is the directory containing
/// `registry.toml` (suitable for `--registry`).
pub fn write_registry(root: &Path, version: &str, files: &[(&str, &str, &str)]) -> PathBuf {
    let registry_dir = root.join("registry");
    fs::create_dir_all(&registry_dir).unwrap();

    let mut manifest = String::from("schema_version = 1\nregistry_id = \"test-registry\"\n\n");
    manifest.push_str("[[components]]\n");
    manifest.push_str("id = \"widget/sample\"\n");
    manifest.push_str(&format!("version = \"{version}\"\n"));
    manifest.push_str("target = \"swiftui\"\n");
    manifest.push_str("description = \"test component\"\n");
    for (source, target, role) in files {
        manifest.push_str("\n[[components.files]]\n");
        manifest.push_str(&format!("source = \"{source}\"\n"));
        manifest.push_str(&format!("target = \"{target}\"\n"));
        manifest.push_str(&format!("role = \"{role}\"\n"));
    }
    fs::write(registry_dir.join("registry.toml"), manifest).unwrap();

    for (source, _target, _role) in files {
        let source_path = registry_dir.join(source);
        if let Some(parent) = source_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        // Deterministic initial content keyed off the source path so the test
        // can later overwrite it with a known new value.
        fs::write(source_path, format!("// upstream v{version}: {source}\n")).unwrap();
    }
    registry_dir
}

/// Overwrite a source file inside a registry produced by `write_registry`,
/// simulating "upstream shipped a new revision".
pub fn overwrite_registry_file(registry_dir: &Path, source: &str, content: &str) {
    fs::write(registry_dir.join(source), content).unwrap();
}

/// Bump the `version = "..."` line inside the registry manifest. The
/// synthetic registries used by tests only ever declare one component, so
/// rewriting the first match is unambiguous.
pub fn bump_registry_version(registry_dir: &Path, new_version: &str) {
    let manifest_path = registry_dir.join("registry.toml");
    let manifest = fs::read_to_string(&manifest_path).unwrap();
    let mut out = String::new();
    let mut replaced = false;
    for line in manifest.lines() {
        if !replaced && line.starts_with("version = ") {
            out.push_str(&format!("version = \"{new_version}\"\n"));
            replaced = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    fs::write(&manifest_path, out).unwrap();
}

/// Read every value of `key = "..."` in a TOML blob, in document order. Good
/// enough for the lock's flat-shape assertions (every test that needs richer
/// queries can parse with `toml` directly).
pub fn read_lock_field(lock: &str, key: &str) -> Vec<String> {
    lock.lines()
        .filter_map(|line| {
            let line = line.trim();
            let prefix = format!("{key} = \"");
            line.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix('"'))
                .map(ToOwned::to_owned)
        })
        .collect()
}

/// Walk a flat-shape lock and return the `source_sha256` recorded for the
/// `[[components.files]]` whose `path = "..."` matches `target_path`. Returns
/// `None` if no such entry exists.
///
/// The lock layout we walk is the one `ComponentLock::write` emits — one
/// `path` line per file followed (a couple of lines later) by its
/// `source_sha256`. Tests use this to assert a specific file's hash without
/// pulling `toml` into the test build.
pub fn lock_sha_for_path(lock: &str, target_path: &str) -> Option<String> {
    let needle = format!("path = \"{target_path}\"");
    let mut hit = false;
    for line in lock.lines() {
        let trimmed = line.trim();
        if trimmed == needle {
            hit = true;
            continue;
        }
        if hit {
            if let Some(rest) = trimmed.strip_prefix("source_sha256 = \"") {
                if let Some(hash) = rest.strip_suffix('"') {
                    return Some(hash.to_string());
                }
            }
            // Bail if we hit the next `[[components.files]]` header without
            // having found a sha — keeps the search local to the matched
            // file's block.
            if trimmed.starts_with("[[components") {
                return None;
            }
        }
    }
    None
}

pub fn sha256_hex_of(content: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(content.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
