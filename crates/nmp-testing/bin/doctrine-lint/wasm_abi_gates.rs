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
//!
//! These are a permanent backstop so a future change cannot silently
//! re-introduce the deleted crate.

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
    let banned_phrases = [
        r#"name = "nmp-wasm""#,
        r#"path = "crates/nmp-wasm""#,
    ];

    let mut violations = Vec::new();

    fn scan_toml_dir(
        dir: &std::path::Path,
        banned: &[&str],
        violations: &mut Vec<String>,
    ) {
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
                for (n, line) in text.lines().enumerate() {
                    let trimmed = line.trim_start();
                    // Allow comment lines that document the deletion.
                    if trimmed.starts_with('#') {
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
        let text = std::fs::read_to_string(&toml_roots[0])
            .expect("root Cargo.toml must be readable");
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            if line.contains("crates/nmp-wasm") {
                violations.push(format!(
                    "Cargo.toml:{}: {}",
                    n + 1,
                    line.trim()
                ));
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
