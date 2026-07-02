use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::workspace_root;

const PLATFORM_BOUNDARY_DEPS: &[&str] = &[
    "nmp-native-runtime",
    "nmp-ffi",
    "nmp-wasm",
    "nmp-browser-runtime",
];

const FFI_COMPOSITION_TOKENS: &[&str] = &[
    "register_defaults(",
    ".register_action(",
    ".register_default_action(",
    "register_typed_snapshot_projection(",
    "register_typed_snapshot_projection_with_time(",
    "register_ingest_parser(",
    "replace_kind_parser(",
    "open_observed_projection(",
    "open_observed_interest_pinned(",
];

pub(super) fn cargo_metadata() -> serde_json::Value {
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

pub(super) fn forbidden_platform_dep_findings(
    metadata: &serde_json::Value,
    crates: &[&str],
) -> Vec<String> {
    let lower: BTreeSet<&str> = crates.iter().copied().collect();
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let mut findings = Vec::new();

    for package in packages {
        let package_name = package["name"].as_str().unwrap_or("<unnamed>");
        if !lower.contains(package_name) {
            continue;
        }
        let Some(dependencies) = package["dependencies"].as_array() else {
            continue;
        };
        for dependency in dependencies {
            let dep_name = dependency["name"].as_str().unwrap_or("<unnamed>");
            if !PLATFORM_BOUNDARY_DEPS.contains(&dep_name) {
                continue;
            }
            let kind = dependency["kind"].as_str().unwrap_or("normal");
            let optional = dependency["optional"].as_bool().unwrap_or(false);
            if kind == "dev" || optional {
                continue;
            }
            findings.push(format!("{package_name} -> {dep_name} ({kind})"));
        }
    }

    findings
}

pub(super) fn lower_layer_crates() -> Vec<&'static str> {
    fixture_items(include_str!(
        "fixtures/native_runtime_boundary/lower_layer_crates.txt"
    ))
    .collect()
}

pub(super) fn allowed_native_nmp_symbols() -> BTreeSet<String> {
    fixture_items(include_str!(
        "fixtures/native_runtime_boundary/nmp_ffi_allowed_symbols.txt"
    ))
    .map(str::to_string)
    .collect()
}

pub(super) fn crate_native_rs_files() -> (PathBuf, Vec<PathBuf>) {
    let root = workspace_root();
    let mut files = Vec::new();
    let crates_dir = root.join("crates");
    if let Ok(entries) = std::fs::read_dir(crates_dir) {
        for entry in entries.filter_map(Result::ok) {
            let src = entry.path().join("src");
            if src.is_dir() {
                collect_rs_files(&src, &mut files);
            }
        }
    }
    files.sort();
    (root, files)
}

pub(super) fn is_nmp_ffi_export_source(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            !(name.ends_with("_tests.rs") || name == "tests.rs" || name.starts_with("tests_"))
        })
}

pub(super) fn rust_live_lines(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in source.lines() {
        out.push(strip_rust_comments(line, &mut in_block));
    }
    out
}

pub(super) fn composition_findings(line: &str) -> Vec<&'static str> {
    FFI_COMPOSITION_TOKENS
        .iter()
        .copied()
        .filter(|token| line.contains(token))
        .collect()
}

pub(super) fn exported_native_nmp_symbol(line: &str) -> Option<&str> {
    if !line.contains("extern \"C\"") {
        return None;
    }
    let fn_idx = line.find("fn nmp_")?;
    let rest = &line[fn_idx + "fn ".len()..];
    rest.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .next()
        .filter(|name| !name.is_empty())
}

pub(super) fn relative_to<'a>(root: &Path, path: &'a Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

fn fixture_items(body: &'static str) -> impl Iterator<Item = &'static str> {
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn strip_rust_comments(line: &str, in_block: &mut bool) -> String {
    let mut out = String::new();
    let mut rest = line;
    loop {
        if *in_block {
            if let Some(end) = rest.find("*/") {
                rest = &rest[end + 2..];
                *in_block = false;
            } else {
                break;
            }
        } else if let Some(line_comment) = rest.find("//") {
            out.push_str(&rest[..line_comment]);
            break;
        } else if let Some(start) = rest.find("/*") {
            out.push_str(&rest[..start]);
            rest = &rest[start + 2..];
            *in_block = true;
        } else {
            out.push_str(rest);
            break;
        }
    }
    out
}
