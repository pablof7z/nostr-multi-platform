//! Retired-crate gate: `nmp-wasm` is DELETED (#2202).
//!
//! `nmp-wasm` was a dead parallel browser runtime crate. Its ABI
//! responsibilities now live in `crates/nmp-browser-runtime::wasm` (the
//! wasm-bindgen Worker export over `NmpRuntimeCore`). These gates enforce that
//! the retirement is permanent:
//!
//! 1. No package named `nmp-wasm` may appear in `cargo metadata`.
//! 2. The `crates/nmp-wasm` directory must not exist.
//! 3. No live Rust or TOML source may reintroduce `nmp-wasm` as a live crate
//!    (i.e. as a `[package] name = "nmp-wasm"` or workspace member path).
//! 4. No other crate may grow the browser Worker ABI entry point owned by
//!    `nmp-browser-runtime`.
//!
//! These are a permanent backstop so a future change cannot silently
//! re-introduce the deleted crate.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::workspace_root;

// ─── Gate 1: cargo metadata must not know of nmp-wasm ────────────────────────

#[test]
fn nmp_wasm_crate_is_not_in_cargo_metadata() {
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

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata JSON must parse");
    let has_nmp_wasm = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array")
        .iter()
        .any(|pkg| pkg["name"] == "nmp-wasm");

    assert!(
        !has_nmp_wasm,
        "nmp-wasm is a RETIRED crate (deleted in #2202). It must not appear in \
         cargo metadata. Browser ABI glue belongs in nmp-browser-runtime::wasm."
    );
}

// ─── Gate 2: the crates/nmp-wasm directory must not exist ────────────────────

#[test]
fn nmp_wasm_directory_does_not_exist() {
    let root = workspace_root();
    let wasm_dir = root.join("crates").join("nmp-wasm");
    assert!(
        !wasm_dir.exists(),
        "crates/nmp-wasm must not exist — it is a retired crate (deleted in #2202). \
         Browser ABI glue belongs in crates/nmp-browser-runtime."
    );
}

// ─── Gate 3: no live source reintroduces nmp-wasm as a crate name ────────────

/// Scans every `Cargo.toml` under `crates/` and `apps/` for evidence that
/// `nmp-wasm` has been re-introduced as a live crate. Specifically:
///
/// - `[package] name = "nmp-wasm"` — would declare a new crate with that name.
/// - `path = "crates/nmp-wasm"` — would re-add it as a workspace member.
///
/// Comments and doc-string mentions are allowed (they may explain the deletion);
/// only bare code-line occurrences are flagged.
#[test]
fn nmp_wasm_is_not_reintroduced_as_live_crate_in_source() {
    let root = workspace_root();
    // Scan Cargo.toml files throughout the workspace. Only flag declarations
    // that would reconstitute the crate (a `[package] name` or member `path`),
    // not bare dependency mentions.
    let toml_roots = [root.join("Cargo.toml"), root.join("release")];
    let banned_phrases = [r#"name = "nmp-wasm""#, r#"path = "crates/nmp-wasm""#];

    let mut violations = Vec::new();

    fn scan_toml_dir(dir: &std::path::Path, banned: &[&str], violations: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map_or(false, |n| n == "target") {
                    continue;
                }
                scan_toml_dir(&path, banned, violations);
            } else if path.extension().map_or(false, |e| e == "toml") {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let mut in_retired_crates = false;
                for (n, line) in text.lines().enumerate() {
                    let trimmed = line.trim_start();
                    // Allow comment lines that document the deletion.
                    if trimmed.starts_with('#') {
                        continue;
                    }
                    if trimmed.starts_with("[[") {
                        in_retired_crates = trimmed == "[[retired_crates]]";
                        continue;
                    }
                    if in_retired_crates {
                        continue;
                    }
                    for phrase in banned {
                        if line.contains(phrase) {
                            violations.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                n + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }
    }

    // Check the root Cargo.toml (workspace members list).
    {
        let text =
            std::fs::read_to_string(&toml_roots[0]).expect("root Cargo.toml must be readable");
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            if line.contains("crates/nmp-wasm") {
                violations.push(format!("Cargo.toml:{}: {}", n + 1, line.trim()));
            }
        }
    }

    // Check release/ TOML files (nmp-release.toml public_crates list).
    scan_toml_dir(&toml_roots[1], &banned_phrases, &mut violations);

    assert!(
        violations.is_empty(),
        "nmp-wasm has been reintroduced as a live crate in TOML source. \
         It is a RETIRED crate (deleted in #2202); browser ABI glue belongs \
         in nmp-browser-runtime::wasm. Violations:\n{}",
        violations.join("\n")
    );
}

// ─── Gate 4: browser Worker ABI entrypoints have one crate owner ─────────────

/// The rule is deliberately narrower than "no wasm-bindgen outside
/// nmp-browser-runtime": storage shims, NIP-07 browser helpers, and conformance
/// harnesses are valid wasm crates. What must not regrow is a second Worker
/// runtime surface with `NmpWasmRuntime` or its exported control methods.
#[test]
fn browser_worker_abi_entrypoints_stay_in_browser_runtime() {
    let root = workspace_root();
    let mut violations = Vec::new();
    let mut rust_files = Vec::new();

    collect_rust_files(&root.join("crates"), &mut rust_files);
    collect_rust_files(&root.join("apps"), &mut rust_files);

    for path in rust_files {
        if is_allowed_wasm_entrypoint_owner(&root, &path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if is_comment_or_blank(trimmed) {
                continue;
            }

            if contains_worker_runtime_type(trimmed) {
                violations.push(format!("{}:{}: {}", path.display(), idx + 1, trimmed));
                continue;
            }

            if trimmed.starts_with("#[wasm_bindgen")
                && next_code_line_exports_worker_method(&lines, idx + 1)
            {
                violations.push(format!("{}:{}: {}", path.display(), idx + 1, trimmed));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Browser Worker ABI entrypoints must stay in crates/nmp-browser-runtime. \
         Storage/conformance wasm crates may use wasm-bindgen, but they must not \
         export NmpWasmRuntime or its Worker control methods. Violations:\n{}",
        violations.join("\n")
    );
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(name, "target" | ".git" | "vendor") {
                continue;
            }
            collect_rust_files(&path, out);
        } else if path.extension().map_or(false, |e| e == "rs") {
            out.push(path);
        }
    }
}

fn is_allowed_wasm_entrypoint_owner(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    [
        Path::new("crates/nmp-browser-runtime"),
        Path::new("crates/nmp-browser-runtime-conformance"),
        Path::new("crates/nmp-sqlite-wasm"),
        Path::new("crates/nmp-sqlite-wasm-conformance"),
    ]
    .iter()
    .any(|allowed| relative.starts_with(allowed))
}

fn is_comment_or_blank(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with("///")
        || trimmed.starts_with("//!")
        || trimmed.starts_with('*')
}

fn contains_worker_runtime_type(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or(line);
    code.contains("NmpWasmRuntime")
        && (code.contains("pub struct")
            || code.contains("struct ")
            || code.contains("impl ")
            || code.contains("pub use")
            || code.contains("type "))
}

fn next_code_line_exports_worker_method(lines: &[&str], start: usize) -> bool {
    for line in lines.iter().skip(start) {
        let trimmed = line.trim_start();
        if is_comment_or_blank(trimmed) || trimmed.starts_with("#[") {
            continue;
        }
        return exports_worker_method(trimmed);
    }
    false
}

fn exports_worker_method(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or(line);
    [
        "pub fn handle_json",
        "pub fn handle_dispatch_bytes",
        "pub fn set_snapshot_callback",
        "pub async fn prepare_store",
        "pub fn recent_routing_decisions",
        "pub fn nmp_encode_npub",
    ]
    .iter()
    .any(|needle| code.contains(needle))
}

// ─── Retired-crate gate: `nmp-uniffi` is DELETED (#2763) ─────────────────────
//
// `nmp-uniffi` was an unconsumed reference UniFFI facade crate. The blessed
// native binding pattern is an app-owned UniFFI facade crate composed over
// `nmp-uniffi-support` (see docs/architecture/crate-boundaries.md §10); no
// stock consumable binding crate exists. These gates mirror the `nmp-wasm`
// retirement gates above and enforce that the deletion is permanent:
//
// 1. No package named `nmp-uniffi` may appear in `cargo metadata`.
// 2. The `crates/nmp-uniffi` directory must not exist.
// 3. No live TOML source may reintroduce `nmp-uniffi` as a live crate (i.e.
//    as a `[package] name = "nmp-uniffi"` or workspace member/public-crate
//    `path = "crates/nmp-uniffi"`). `nmp-uniffi-support` is a distinct crate
//    and must not be flagged.

#[test]
fn nmp_uniffi_crate_is_not_in_cargo_metadata() {
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

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata JSON must parse");
    let has_nmp_uniffi = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array")
        .iter()
        .any(|pkg| pkg["name"] == "nmp-uniffi");

    assert!(
        !has_nmp_uniffi,
        "nmp-uniffi is a RETIRED crate (deleted in #2763). It must not appear \
         in cargo metadata. The blessed native binding pattern is an \
         app-owned UniFFI facade crate over nmp-uniffi-support."
    );
}

#[test]
fn nmp_uniffi_directory_does_not_exist() {
    let root = workspace_root();
    let uniffi_dir = root.join("crates").join("nmp-uniffi");
    assert!(
        !uniffi_dir.exists(),
        "crates/nmp-uniffi must not exist — it is a retired crate (deleted in \
         #2763). The blessed native binding pattern is an app-owned UniFFI \
         facade crate over nmp-uniffi-support (crates/nmp-uniffi-support is \
         unaffected and must continue to exist)."
    );
}

#[test]
fn nmp_uniffi_is_not_reintroduced_as_live_crate_in_source() {
    let root = workspace_root();
    let toml_roots = [root.join("Cargo.toml"), root.join("release")];
    let banned_phrases = [r#"name = "nmp-uniffi""#, r#"path = "crates/nmp-uniffi""#];

    let mut violations = Vec::new();

    fn scan_toml_dir(dir: &std::path::Path, banned: &[&str], violations: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map_or(false, |n| n == "target") {
                    continue;
                }
                scan_toml_dir(&path, banned, violations);
            } else if path.extension().map_or(false, |e| e == "toml") {
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let mut in_retired_crates = false;
                for (n, line) in text.lines().enumerate() {
                    let trimmed = line.trim_start();
                    if trimmed.starts_with('#') {
                        continue;
                    }
                    if trimmed.starts_with("[[") {
                        in_retired_crates = trimmed == "[[retired_crates]]";
                        continue;
                    }
                    if in_retired_crates {
                        continue;
                    }
                    for phrase in banned {
                        if line.contains(phrase) {
                            violations.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                n + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }
    }

    // Check the root Cargo.toml (workspace members list). Match on the exact
    // quoted member path so `crates/nmp-uniffi-support` is never flagged.
    {
        let text =
            std::fs::read_to_string(&toml_roots[0]).expect("root Cargo.toml must be readable");
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            if line.contains(r#""crates/nmp-uniffi""#) {
                violations.push(format!("Cargo.toml:{}: {}", n + 1, line.trim()));
            }
        }
    }

    // Check release/ TOML files (nmp-release.toml public_crates list).
    scan_toml_dir(&toml_roots[1], &banned_phrases, &mut violations);

    assert!(
        violations.is_empty(),
        "nmp-uniffi has been reintroduced as a live crate in TOML source. \
         It is a RETIRED crate (deleted in #2763); the blessed native \
         binding pattern is an app-owned UniFFI facade crate over \
         nmp-uniffi-support. Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn browser_worker_abi_detection_flags_runtime_exports() {
    let runtime = ["Nmp", "Wasm", "Runtime"].concat();
    assert!(contains_worker_runtime_type(&format!(
        "pub struct {runtime} {{"
    )));
    assert!(contains_worker_runtime_type(&format!("impl {runtime} {{")));
    assert!(next_code_line_exports_worker_method(
        &[
            "#[wasm_bindgen]",
            "pub fn handle_json(&mut self, request: &str) -> JsValue {",
        ],
        1
    ));
    assert!(!next_code_line_exports_worker_method(
        &[
            "#[wasm_bindgen]",
            "pub async fn run_conformance() -> Result<JsValue, JsValue> {",
        ],
        1
    ));
}
