//! Workspace-layout discovery for path-correct codegen.
//!
//! ## Why this exists
//!
//! The Cargo.toml the generator emits for the per-app FFI crate declares one
//! `[dependencies]` entry per module crate, each of the form
//! `<ident> = { package = "<name>", path = "<relative-path>" }`. Computing
//! `<relative-path>` correctly requires knowing **where each module crate
//! actually lives** in the workspace.
//!
//! Prior to this module the generator hard-coded the path template
//! `../../../crates/<name>` for every module — fine for the layered NIP
//! crates that all live under `crates/`, wrong for any crate that lives
//! elsewhere (notably `fixture-todo-core`, which the spec
//! [`docs/architecture/crate-boundaries.md`] §per-crate-table places under
//! `apps/fixture/`).
//!
//! ## How it works
//!
//! [`WorkspaceLayout::discover`] walks up from a starting path looking for
//! a `Cargo.toml` whose top-level `[workspace]` table declares a `members`
//! array. For each member directory it reads `<member>/Cargo.toml` and
//! extracts `[package].name`, building a `name → workspace-relative-path`
//! map. The generator then computes the per-module relative path from the
//! output directory using [`WorkspaceLayout::relative_from`].
//!
//! ## Fallback
//!
//! When discovery returns `None` (a temp-dir test that never built a
//! workspace), [`WorkspaceLayout::relative_from`] is never called; the
//! generator falls back to the legacy `../../../crates/<name>` template. The
//! fallback is exercised by `tests/{determinism,ffi_dispatch}.rs`, which
//! only string-check the emitted source and never link it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A workspace `Cargo.toml`'s membership, indexed for path lookup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceLayout {
    /// Absolute (canonical) path to the directory holding the workspace
    /// `Cargo.toml` (the `[workspace] members = [...]` declaration).
    pub root: PathBuf,
    /// `package-name → workspace-relative directory` for every workspace
    /// member that has a parseable `[package].name`.
    pub members_by_name: BTreeMap<String, PathBuf>,
}

impl WorkspaceLayout {
    /// Walk up from `start` looking for the first `Cargo.toml` that declares
    /// `[workspace] members = [...]`; parse it and every member's
    /// `[package].name`. Returns `None` if no such manifest exists on the
    /// ancestor chain (the standalone-temp-dir test case).
    ///
    /// `start` may be a file (the `nmp.toml`) or a directory; either way we
    /// climb its ancestor directories. The first matching workspace wins —
    /// nested workspaces don't occur in this repo.
    pub fn discover(start: &Path) -> Option<Self> {
        let mut here = if start.is_file() {
            start.parent()?.to_path_buf()
        } else {
            start.to_path_buf()
        };
        // Best-effort canonicalisation: if it fails (path doesn't exist yet),
        // proceed with the lexical path — the ancestor walk only needs to
        // read existing files.
        if let Ok(canonical) = fs::canonicalize(&here) {
            here = canonical;
        }
        loop {
            let manifest = here.join("Cargo.toml");
            if manifest.is_file() {
                if let Some(layout) = Self::try_parse_workspace(&here, &manifest) {
                    return Some(layout);
                }
            }
            here = here.parent()?.to_path_buf();
        }
    }

    /// Try to parse `manifest` as a workspace manifest rooted at `root`.
    /// Returns `None` if the file has no `[workspace] members` array (it's
    /// a leaf-crate Cargo.toml, not a workspace one).
    fn try_parse_workspace(root: &Path, manifest: &Path) -> Option<Self> {
        let body = fs::read_to_string(manifest).ok()?;
        let members = parse_workspace_members(&body)?;
        let mut members_by_name = BTreeMap::new();
        for member in members {
            let member_dir = root.join(&member);
            let member_manifest = member_dir.join("Cargo.toml");
            let Ok(member_body) = fs::read_to_string(&member_manifest) else {
                continue;
            };
            if let Some(name) = parse_package_name(&member_body) {
                members_by_name.insert(name, PathBuf::from(&member));
            }
        }
        Some(Self {
            root: root.to_path_buf(),
            members_by_name,
        })
    }

    /// Compute the relative path from `from_dir` (typically the generator's
    /// `out_dir`) to the workspace-member directory of `package`. Returns
    /// `None` if the workspace has no member with that package name.
    ///
    /// `from_dir` need not exist on disk; we only need to know how many
    /// `..` segments separate it from the workspace root. The output is the
    /// `path = "..."` string written into the generated `Cargo.toml`.
    pub fn relative_from(&self, from_dir: &Path, package: &str) -> Option<String> {
        let member_relative = self.members_by_name.get(package)?;
        let from_canonical = canonical_or_lexical(from_dir);
        // Count how deep `from_dir` is below the workspace root. We strip the
        // root prefix to get its workspace-relative path.
        let from_relative = from_canonical.strip_prefix(&self.root).ok()?;
        let depth = from_relative.components().count();
        let mut path = PathBuf::new();
        for _ in 0..depth {
            path.push("..");
        }
        path.push(member_relative);
        // Normalise to forward slashes for Cargo.toml portability.
        Some(path.to_string_lossy().replace('\\', "/"))
    }
}

/// Best-effort: try `fs::canonicalize`, fall back to the lexical path. The
/// generator may be asked to emit into a directory that doesn't exist yet
/// (the `apps/<name>/nmp-app-<name>` target on a fresh checkout); the lexical
/// path is still adequate for `strip_prefix` against a canonical root iff
/// the input was already in canonical form.
fn canonical_or_lexical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        // If the leaf doesn't exist, canonicalise the longest existing
        // ancestor and re-attach the missing tail — that keeps the prefix
        // form consistent with `WorkspaceLayout::root`.
        let mut existing = path.to_path_buf();
        let mut tail = PathBuf::new();
        while !existing.exists() {
            let Some(name) = existing.file_name().map(ToOwned::to_owned) else {
                break;
            };
            tail = PathBuf::from(name).join(&tail);
            let Some(parent) = existing.parent().map(Path::to_path_buf) else {
                break;
            };
            existing = parent;
        }
        match fs::canonicalize(&existing) {
            Ok(canonical) => canonical.join(tail),
            Err(_) => path.to_path_buf(),
        }
    })
}

/// Extract the `members = [...]` array from a workspace `Cargo.toml`.
/// Returns `None` if there is no `[workspace]` section or no `members` key.
///
/// We hand-parse rather than depend on `toml`: the per-`AppManifest` parser
/// (`manifest.rs`) is already hand-rolled with the same shape and the
/// codegen crate stays serde-free.
fn parse_workspace_members(body: &str) -> Option<Vec<String>> {
    let mut in_workspace = false;
    let mut collecting = false;
    let mut buffer = String::new();
    for raw_line in body.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if collecting {
                // A new section before we closed the `members` array means
                // the array spanned bracket-balanced inline form already
                // captured below; treat absence as failure.
                return None;
            }
            in_workspace = line == "[workspace]";
            continue;
        }
        if !in_workspace {
            continue;
        }
        if collecting {
            buffer.push_str(line);
            if line.contains(']') {
                return parse_string_array(&buffer);
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "members" {
            continue;
        }
        let value = value.trim();
        if value.contains(']') {
            return parse_string_array(value);
        }
        // Multi-line members array: keep accumulating until we see `]`.
        buffer.push_str(value);
        collecting = true;
    }
    None
}

/// Parse a `[ "a", "b", "c" ]`-shaped TOML literal into its string contents.
fn parse_string_array(value: &str) -> Option<Vec<String>> {
    let inner = value.trim().strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for part in inner.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let stripped = part.strip_prefix('"').and_then(|s| s.strip_suffix('"'))?;
        out.push(stripped.to_string());
    }
    Some(out)
}

/// Extract `name = "..."` from the `[package]` section of a Cargo.toml.
/// `name.workspace = true` is accepted but yields `None` (no concrete name
/// at the leaf level — the codegen ignores such members).
fn parse_package_name(body: &str) -> Option<String> {
    let mut in_package = false;
    for raw_line in body.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "name" {
            continue;
        }
        let value = value.trim();
        return value.strip_prefix('"').and_then(|s| s.strip_suffix('"')).map(ToOwned::to_owned);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `tempdir` substitute — uses the process PID + a label for uniqueness
    /// so parallel tests don't collide.
    fn temp_root(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("nmp-codegen-ws-{label}-{}", std::process::id()));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn write(path: &Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    #[test]
    fn discovers_workspace_members_by_package_name() {
        // Two members in different parent directories; both have a normal
        // `[package].name = "..."` — both must appear in the lookup map.
        let root = temp_root("discover-basic");
        write(
            &root.join("Cargo.toml"),
            r#"[workspace]
members = ["crates/foo", "apps/bar/baz"]
"#,
        );
        write(
            &root.join("crates/foo/Cargo.toml"),
            r#"[package]
name = "foo"
version = "0.1.0"
"#,
        );
        write(
            &root.join("apps/bar/baz/Cargo.toml"),
            r#"[package]
name = "baz-pkg"
"#,
        );

        let layout = WorkspaceLayout::discover(&root).expect("workspace found");
        assert_eq!(
            layout.members_by_name.get("foo"),
            Some(&PathBuf::from("crates/foo"))
        );
        assert_eq!(
            layout.members_by_name.get("baz-pkg"),
            Some(&PathBuf::from("apps/bar/baz"))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn returns_none_when_no_workspace_on_ancestor_chain() {
        // A plain directory with no Cargo.toml at any depth must yield
        // `None` — the `crate::generate` fallback then keeps the legacy
        // path template. This is the temp-dir test case.
        let root = temp_root("no-workspace");
        let sub = root.join("a/b/c");
        fs::create_dir_all(&sub).unwrap();
        assert!(WorkspaceLayout::discover(&sub).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relative_from_computes_dotdot_path_per_output_depth() {
        // Output at `apps/fixture/nmp-app-fixture/` (3 deep) → target
        // `apps/fixture/fixture-todo-core` ⇒ `../../../apps/fixture/fixture-todo-core`
        // and a target under `crates/` ⇒ `../../../crates/nmp-core`.
        let root = temp_root("relative-from");
        write(
            &root.join("Cargo.toml"),
            r#"[workspace]
members = ["apps/fixture/fixture-todo-core", "crates/nmp-core"]
"#,
        );
        write(
            &root.join("apps/fixture/fixture-todo-core/Cargo.toml"),
            r#"[package]
name = "fixture-todo-core"
"#,
        );
        write(
            &root.join("crates/nmp-core/Cargo.toml"),
            r#"[package]
name = "nmp-core"
"#,
        );
        // Touch the output dir tree so canonicalisation succeeds on the
        // ancestors.
        let out_dir = root.join("apps/fixture/nmp-app-fixture");
        fs::create_dir_all(&out_dir).unwrap();
        let layout = WorkspaceLayout::discover(&root).expect("workspace found");
        assert_eq!(
            layout.relative_from(&out_dir, "fixture-todo-core").as_deref(),
            Some("../../../apps/fixture/fixture-todo-core"),
        );
        assert_eq!(
            layout.relative_from(&out_dir, "nmp-core").as_deref(),
            Some("../../../crates/nmp-core"),
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn members_with_workspace_inherited_name_are_skipped() {
        // A member that inherits `name.workspace = true` has no concrete
        // package name at parse time (this repo doesn't do this for `name`,
        // but the parser must tolerate it). Such members are silently
        // dropped — the lookup is best-effort.
        let root = temp_root("workspace-inherited");
        write(
            &root.join("Cargo.toml"),
            r#"[workspace]
members = ["weird"]
"#,
        );
        write(
            &root.join("weird/Cargo.toml"),
            r#"[package]
name.workspace = true
"#,
        );
        let layout = WorkspaceLayout::discover(&root).expect("workspace found");
        assert!(layout.members_by_name.is_empty(), "inherited name skipped");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn multi_line_members_array_is_parsed() {
        // The real workspace Cargo.toml uses a multi-line array. The parser
        // must reassemble it correctly.
        let root = temp_root("multiline-members");
        write(
            &root.join("Cargo.toml"),
            r#"[workspace]
members = [
    "a",
    "b",
]
"#,
        );
        write(&root.join("a/Cargo.toml"), r#"[package]
name = "a"
"#);
        write(&root.join("b/Cargo.toml"), r#"[package]
name = "b"
"#);
        let layout = WorkspaceLayout::discover(&root).expect("workspace found");
        assert_eq!(layout.members_by_name.len(), 2);
        assert_eq!(layout.members_by_name.get("a"), Some(&PathBuf::from("a")));
        assert_eq!(layout.members_by_name.get("b"), Some(&PathBuf::from("b")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discover_climbs_through_intermediate_directories() {
        // Asking from a nested subdirectory must still find the workspace
        // at the root — the generator runs from arbitrary subdirs.
        let root = temp_root("discover-climb");
        write(
            &root.join("Cargo.toml"),
            r#"[workspace]
members = ["x"]
"#,
        );
        write(&root.join("x/Cargo.toml"), r#"[package]
name = "x"
"#);
        let nested = root.join("x/src/inner");
        fs::create_dir_all(&nested).unwrap();
        let layout = WorkspaceLayout::discover(&nested).expect("workspace found from deep");
        assert!(layout.members_by_name.contains_key("x"));
        fs::remove_dir_all(root).unwrap();
    }
}
