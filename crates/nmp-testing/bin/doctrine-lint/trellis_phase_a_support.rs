//! Manifest and dependency graph helpers for #2858 Trellis Phase A gates.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::trellis_public_api_support::relative_to;
use super::workspace_root;

#[derive(Default)]
pub(crate) struct ReleaseManifest {
    pub(crate) public_crates: BTreeMap<String, String>,
    pub(crate) private_packages: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub(crate) struct TrellisManifestDep {
    pub(crate) manifest: String,
    pub(crate) dependency: String,
    pub(crate) line_no: usize,
}

#[derive(Clone, Debug)]
struct PendingDependency {
    line_no: usize,
    name: String,
}

pub(crate) fn release_manifest() -> ReleaseManifest {
    let root = workspace_root();
    let text = std::fs::read_to_string(root.join("release/nmp-release.toml"))
        .expect("release manifest must be readable");
    parse_release_manifest(&text)
}

pub(crate) fn cargo_metadata() -> serde_json::Value {
    let root = workspace_root();
    let output = Command::new(env!("CARGO"))
        .current_dir(&root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .expect("cargo metadata must spawn");

    assert!(
        output.status.success(),
        "cargo metadata failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("cargo metadata JSON must parse")
}

pub(crate) fn workspace_dependency_graph(
    metadata: &serde_json::Value,
) -> BTreeMap<String, Vec<String>> {
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let workspace_names: BTreeSet<_> = packages
        .iter()
        .filter_map(|package| package["name"].as_str())
        .collect();
    let mut graph = BTreeMap::new();

    for package in packages {
        let Some(name) = package["name"].as_str() else {
            continue;
        };
        let deps = package["dependencies"]
            .as_array()
            .expect("package dependencies must be an array")
            .iter()
            .filter_map(|dependency| dependency_graph_edge(dependency, &workspace_names))
            .collect();
        graph.insert(name.to_string(), deps);
    }
    graph
}

pub(crate) fn dependency_path(
    graph: &BTreeMap<String, Vec<String>>,
    start: &str,
    target: &str,
) -> Option<Vec<String>> {
    let mut queue = VecDeque::from([vec![start.to_string()]]);
    let mut seen = BTreeSet::from([start.to_string()]);
    while let Some(path) = queue.pop_front() {
        let Some(last) = path.last() else {
            continue;
        };
        for next in graph.get(last).into_iter().flatten() {
            if !seen.insert(next.clone()) {
                continue;
            }
            let mut candidate = path.clone();
            candidate.push(next.clone());
            if next == target {
                return Some(candidate);
            }
            queue.push_back(candidate);
        }
    }
    None
}

pub(crate) fn trellis_manifest_dependencies(root: &Path) -> Vec<TrellisManifestDep> {
    let mut manifests = Vec::new();
    collect_cargo_manifests(&root.join("Cargo.toml"), &mut manifests);
    collect_cargo_manifests(&root.join("crates"), &mut manifests);
    collect_cargo_manifests(&root.join("apps"), &mut manifests);
    manifests.sort();

    let mut deps = Vec::new();
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest)
            .unwrap_or_else(|err| panic!("read {}: {err}", manifest.display()));
        scan_manifest_trellis_deps(root, &manifest, &text, &mut deps);
    }
    deps
}

fn parse_release_manifest(text: &str) -> ReleaseManifest {
    let mut manifest = ReleaseManifest::default();
    let mut section = "";
    let mut name = "";
    let mut path = "";

    for raw in text.lines() {
        let line = raw.trim();
        if line == "[[public_crates]]" || line == "[[private_packages]]" {
            section = line;
            name = "";
            path = "";
            continue;
        }
        if line.starts_with("[[") {
            section = "";
            continue;
        }
        if section.is_empty() {
            continue;
        }
        if let Some(value) = quoted_value(line, "name") {
            name = value;
        } else if let Some(value) = quoted_value(line, "path") {
            path = value;
        }
        if !name.is_empty() && !path.is_empty() {
            let target = if section == "[[public_crates]]" {
                &mut manifest.public_crates
            } else {
                &mut manifest.private_packages
            };
            target.insert(name.to_string(), path.to_string());
            name = "";
            path = "";
        }
    }
    manifest
}

fn dependency_graph_edge(
    dependency: &serde_json::Value,
    workspace_names: &BTreeSet<&str>,
) -> Option<String> {
    let name = dependency["name"].as_str()?;
    let kind = dependency["kind"].as_str().unwrap_or("normal");
    (kind != "dev" && workspace_names.contains(name)).then(|| name.to_string())
}

fn collect_cargo_manifests(path: &Path, out: &mut Vec<PathBuf>) {
    if path.file_name().and_then(|name| name.to_str()) == Some("target") {
        return;
    }
    if path.is_file() {
        if path.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        collect_cargo_manifests(&entry.path(), out);
    }
}

fn scan_manifest_trellis_deps(
    root: &Path,
    manifest: &Path,
    text: &str,
    out: &mut Vec<TrellisManifestDep>,
) {
    let mut in_deps = false;
    let mut pending: Option<PendingDependency> = None;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = strip_inline_comment(raw).trim();
        if trimmed.starts_with('[') {
            finalize_pending(root, manifest, pending.take(), out);
            in_deps = is_dependency_table(trimmed);
            continue;
        }
        if !in_deps || trimmed.is_empty() {
            continue;
        }
        if let Some(dep) = pending.as_mut() {
            if let Some(package) = quoted_value(trimmed, "package") {
                dep.name = package.to_string();
            }
            if trimmed.contains('}') {
                finalize_pending(root, manifest, pending.take(), out);
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let dep = dependency_name(key.trim(), value.trim());
        if value.contains('{') && !value.contains('}') {
            pending = Some(PendingDependency { line_no, name: dep });
        } else {
            push_trellis_dep(root, manifest, line_no, dep, out);
        }
    }
    finalize_pending(root, manifest, pending, out);
}

fn finalize_pending(
    root: &Path,
    manifest: &Path,
    dep: Option<PendingDependency>,
    out: &mut Vec<TrellisManifestDep>,
) {
    if let Some(dep) = dep {
        push_trellis_dep(root, manifest, dep.line_no, dep.name, out);
    }
}

fn push_trellis_dep(
    root: &Path,
    manifest: &Path,
    line_no: usize,
    dependency: String,
    out: &mut Vec<TrellisManifestDep>,
) {
    if dependency.starts_with("trellis-") {
        out.push(TrellisManifestDep {
            manifest: relative_to(root, manifest).display().to_string(),
            dependency,
            line_no,
        });
    }
}

fn dependency_name(key: &str, value: &str) -> String {
    quoted_value(value, "package")
        .unwrap_or_else(|| key.trim_matches('"'))
        .to_string()
}

fn is_dependency_table(header: &str) -> bool {
    let header = header.trim_matches(['[', ']']);
    header == "dependencies"
        || header == "dev-dependencies"
        || header == "build-dependencies"
        || header == "workspace.dependencies"
        || header.ends_with(".dependencies")
        || header.ends_with(".dev-dependencies")
        || header.ends_with(".build-dependencies")
}

fn strip_inline_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut previous = '\0';
    for (idx, ch) in line.char_indices() {
        if ch == '"' && previous != '\\' {
            in_string = !in_string;
        }
        if ch == '#' && !in_string {
            return &line[..idx];
        }
        previous = ch;
    }
    line
}

fn quoted_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (left, right) = line.split_once('=')?;
    (left.trim() == key)
        .then(|| right.trim().strip_prefix('"'))??
        .split('"')
        .next()
}
