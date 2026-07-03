//! Native runtime split boundary gates (#2214).
//!
//! These smoke gates lock the #2205 split:
//! - `nmp-native-runtime` owns native runtime/session composition.
//! - lower layers do not take default production dependencies on platform
//!   runtime or ABI crates.
//!
//! ## `nmp-ffi` retirement (M14)
//!
//! `nmp-ffi` — the C-ABI glue crate this split originally separated from
//! `nmp-native-runtime` — was deleted by the M14 migration; UniFFI is now the
//! sole native FFI surface. The smoke test that used to scan
//! `crates/nmp-ffi/src` for forbidden runtime-composition tokens
//! (`nmp_ffi_does_not_register_runtime_composition`) silently no-op'd once
//! that directory stopped existing (`std::fs::read_dir` on a missing path
//! returns `Err`, and the walker treated that as "zero files, zero
//! violations"). Rather than leave that vacuous pass in place, the gates
//! below — modeled on the `nmp-wasm` retired-crate gate in
//! `wasm_abi_gates.rs` — assert the deletion is permanent instead.

#[path = "native_runtime_boundary_support.rs"]
mod support;

use support::{
    allowed_native_nmp_symbols, cargo_metadata, composition_findings, crate_native_rs_files,
    exported_native_nmp_symbol, forbidden_platform_dep_findings, is_nmp_ffi_export_source,
    lower_layer_crates, relative_to, rust_live_lines,
};

#[test]
fn lower_layer_crates_do_not_depend_on_platform_runtime_crates() {
    let metadata = cargo_metadata();
    let lower_layer_crates = lower_layer_crates();
    let findings = forbidden_platform_dep_findings(&metadata, &lower_layer_crates);
    assert!(
        findings.is_empty(),
        "Layer 0-5 crates must not take default production dependencies on \
         platform runtime or ABI crates:\n{}",
        findings.join("\n")
    );
}

// ─── nmp-ffi retired-crate gate (M14) ────────────────────────────────────────

#[test]
fn nmp_ffi_crate_is_not_in_cargo_metadata() {
    let metadata = cargo_metadata();
    let has_nmp_ffi = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array")
        .iter()
        .any(|pkg| pkg["name"] == "nmp-ffi");

    assert!(
        !has_nmp_ffi,
        "nmp-ffi is a RETIRED crate (deleted in the M14 migration). It must not \
         appear in cargo metadata. Native platform API belongs in \
         nmp-native-runtime or an app-owned UniFFI facade."
    );
}

#[test]
fn nmp_ffi_directory_does_not_exist() {
    let root = crate::workspace_root();
    let ffi_dir = root.join("crates").join("nmp-ffi");
    assert!(
        !ffi_dir.exists(),
        "crates/nmp-ffi must not exist — it is a retired crate (deleted in the \
         M14 migration). Native platform API belongs in nmp-native-runtime or \
         app-owned UniFFI facades."
    );
}

/// Scans every `Cargo.toml` under the workspace for evidence that `nmp-ffi`
/// has been re-introduced as a live crate. Mirrors
/// `wasm_abi_gates::nmp_wasm_is_not_reintroduced_as_live_crate_in_source`
/// exactly, substituting `nmp-ffi` for `nmp-wasm`.
#[test]
fn nmp_ffi_is_not_reintroduced_as_live_crate_in_source() {
    let root = crate::workspace_root();
    let toml_roots = [root.join("Cargo.toml"), root.join("release")];
    let banned_phrases = [r#"name = "nmp-ffi""#, r#"path = "crates/nmp-ffi""#];

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
                for (n, line) in text.lines().enumerate() {
                    let trimmed = line.trim_start();
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

    {
        let text =
            std::fs::read_to_string(&toml_roots[0]).expect("root Cargo.toml must be readable");
        for (n, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                continue;
            }
            if line.contains("crates/nmp-ffi") {
                violations.push(format!("Cargo.toml:{}: {}", n + 1, line.trim()));
            }
        }
    }

    scan_toml_dir(&toml_roots[1], &banned_phrases, &mut violations);

    assert!(
        violations.is_empty(),
        "nmp-ffi has been reintroduced as a live crate in TOML source. It is a \
         RETIRED crate (deleted in the M14 migration); native platform API \
         belongs in nmp-native-runtime or app-owned UniFFI facades. Violations:\n{}",
        violations.join("\n")
    );
}

#[test]
fn public_native_nmp_symbols_are_allowlisted() {
    let (_, files) = crate_native_rs_files();
    let allowed = allowed_native_nmp_symbols();
    let mut exported = std::collections::BTreeSet::new();

    for path in files {
        if !is_nmp_ffi_export_source(&path) {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for line in rust_live_lines(&body) {
            if let Some(symbol) = exported_native_nmp_symbol(&line) {
                exported.insert(symbol.to_string());
            }
        }
    }

    let extra: Vec<_> = exported.difference(&allowed).cloned().collect();
    let missing: Vec<_> = allowed
        .iter()
        .filter(|symbol| !exported.contains(*symbol))
        .cloned()
        .collect();

    assert!(
        extra.is_empty() && missing.is_empty(),
        "public native nmp_* C symbols under crates/*/src must be explicit. Additions need \
         an accepted issue or ADR explaining why action/projection/capability/\
         runtime APIs are insufficient.\nextra:\n{}\nmissing:\n{}",
        extra.join("\n"),
        missing.join("\n")
    );
}

#[test]
fn nmp_native_runtime_does_not_reexport_raw_observed_projection_doors() {
    let root = crate::workspace_root();
    let lib = root.join("crates/nmp-native-runtime/src/lib.rs");
    let feeds = root.join("crates/nmp-native-runtime/src/app_impl_feeds.rs");
    let handle = root.join("crates/nmp-native-runtime/src/observed_projection_handle.rs");
    let files = [lib, feeds, handle];
    let forbidden = [
        (
            "pub use nmp_core::substrate::ObservedProjectionCommandHandle",
            "raw command handle re-export",
        ),
        (
            "pub fn open_observed_interest(",
            "raw observed-interest open",
        ),
        (
            "pub fn open_observed_interest_pinned(",
            "raw pinned observed-interest open",
        ),
        ("pub fn event_observers_handle(", "raw observer sink slot"),
        (
            "pub fn observed_projection_handle(",
            "raw observed-projection command handle",
        ),
    ];
    let mut violations = Vec::new();

    for path in files {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for (idx, live) in rust_live_lines(&body).into_iter().enumerate() {
            for (token, reason) in forbidden {
                if live.contains(token) {
                    violations.push(format!(
                        "{}:{} {reason}: `{token}`",
                        relative_to(&root, &path).display(),
                        idx + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "nmp-native-runtime must not expose raw observed-interest/projection \
         executor doors as app-facing API; typed sessions and feature handles \
         own app reads:\n{}",
        violations.join("\n")
    );
}

#[test]
fn nmp_native_runtime_does_not_bundle_nip29_as_production_dependency() {
    let metadata = cargo_metadata();
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let runtime = packages
        .iter()
        .find(|pkg| pkg["name"] == "nmp-native-runtime")
        .expect("nmp-native-runtime package must be in cargo metadata");
    let dependencies = runtime["dependencies"]
        .as_array()
        .expect("package dependencies must be an array");

    let mut dev_dependency_present = false;
    let mut production_findings = Vec::new();
    for dependency in dependencies {
        if dependency["name"] != "nmp-nip29" {
            continue;
        }
        let kind = dependency["kind"].as_str().unwrap_or("normal");
        let optional = dependency["optional"].as_bool().unwrap_or(false);
        if kind == "dev" {
            dev_dependency_present = true;
        } else if !optional {
            production_findings.push(format!("nmp-native-runtime -> nmp-nip29 ({kind})"));
        }
    }

    assert!(
        production_findings.is_empty(),
        "#2797: nmp-native-runtime must not bundle the NIP-29 group concept as \
         a production dependency; concept doorways live in nmp-nip29 and native \
         runtime tests may use it only as a dev-dependency:\n{}",
        production_findings.join("\n")
    );
    assert!(
        dev_dependency_present,
        "nmp-native-runtime still keeps NIP-29 host-seam tests; if those tests \
         move elsewhere, remove this dev-dependency expectation with the tests."
    );
}

#[test]
fn nmp_native_runtime_wallet_nips_are_feature_gated() {
    let metadata = cargo_metadata();
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let runtime = packages
        .iter()
        .find(|pkg| pkg["name"] == "nmp-native-runtime")
        .expect("nmp-native-runtime package must be in cargo metadata");
    let dependencies = runtime["dependencies"]
        .as_array()
        .expect("package dependencies must be an array");

    let mut findings = Vec::new();
    for dependency in dependencies {
        let name = dependency["name"].as_str().unwrap_or_default();
        if !matches!(name, "nmp-nip47" | "nmp-nip57") {
            continue;
        }
        let kind = dependency["kind"].as_str().unwrap_or("normal");
        let optional = dependency["optional"].as_bool().unwrap_or(false);
        if kind != "dev" && !optional {
            findings.push(format!("nmp-native-runtime -> {name} ({kind})"));
        }
    }

    assert!(
        findings.is_empty(),
        "#2797: NIP-47 wallet wiring and NIP-57 payment chaining are opt-in \
         composition, not default native runtime surface. Keep these concept \
         crates behind the nmp-native-runtime `wallet` feature:\n{}",
        findings.join("\n")
    );
}

#[test]
fn nmp_native_runtime_search_concept_is_feature_gated() {
    let metadata = cargo_metadata();
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let runtime = packages
        .iter()
        .find(|pkg| pkg["name"] == "nmp-native-runtime")
        .expect("nmp-native-runtime package must be in cargo metadata");
    let dependencies = runtime["dependencies"]
        .as_array()
        .expect("package dependencies must be an array");

    let mut findings = Vec::new();
    for dependency in dependencies {
        if dependency["name"] != "nmp-nip50" {
            continue;
        }
        let kind = dependency["kind"].as_str().unwrap_or("normal");
        let optional = dependency["optional"].as_bool().unwrap_or(false);
        if kind != "dev" && !optional {
            findings.push(format!("nmp-native-runtime -> nmp-nip50 ({kind})"));
        }
    }

    assert!(
        findings.is_empty(),
        "#2797: NIP-50 search is concept-owned composition. Keep the native \
         runtime SearchHost implementation and text-query dispatch behind the \
         nmp-native-runtime `search` feature:\n{}",
        findings.join("\n")
    );
}

#[test]
fn nmp_native_runtime_op_feed_concepts_are_feature_gated() {
    let metadata = cargo_metadata();
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let runtime = packages
        .iter()
        .find(|pkg| pkg["name"] == "nmp-native-runtime")
        .expect("nmp-native-runtime package must be in cargo metadata");
    let dependencies = runtime["dependencies"]
        .as_array()
        .expect("package dependencies must be an array");

    let mut findings = Vec::new();
    for dependency in dependencies {
        let name = dependency["name"].as_str().unwrap_or_default();
        if !matches!(name, "nmp-nip02" | "nmp-nip51" | "nmp-note-feed") {
            continue;
        }
        let kind = dependency["kind"].as_str().unwrap_or("normal");
        let optional = dependency["optional"].as_bool().unwrap_or(false);
        if kind != "dev" && !optional {
            findings.push(format!("nmp-native-runtime -> {name} ({kind})"));
        }
    }

    assert!(
        findings.is_empty(),
        "#2797: OP-centric active-follows feed composition names NIP-02, \
         NIP-51, and nmp-note-feed; keep those direct native-runtime edges \
         behind the nmp-native-runtime `op-feed` feature:\n{}",
        findings.join("\n")
    );
}

#[test]
fn platform_dependency_gate_negative_fixture_fires() {
    let packages = serde_json::json!({
        "packages": [{
            "name": "nmp-nip29",
            "dependencies": [{
                "name": "nmp-ffi",
                "kind": null,
                "optional": false
            }]
        }]
    });
    let findings = forbidden_platform_dep_findings(&packages, &["nmp-nip29"]);
    assert!(
        findings.iter().any(|f| f.contains("nmp-nip29 -> nmp-ffi")),
        "negative fixture should flag a lower-layer default nmp-ffi dependency; got {findings:?}"
    );
}

#[test]
fn platform_dependency_gate_allows_dev_and_optional_edges() {
    let packages = serde_json::json!({
        "packages": [{
            "name": "nmp-substrate",
            "dependencies": [
                {"name": "nmp-native-runtime", "kind": "dev", "optional": false},
                {"name": "nmp-ffi", "kind": null, "optional": true}
            ]
        }]
    });
    let findings = forbidden_platform_dep_findings(&packages, &["nmp-substrate"]);
    assert!(
        findings.is_empty(),
        "dev/test-support and optional feature edges are not default production deps; got {findings:?}"
    );
}

#[test]
fn ffi_composition_token_negative_fixture_fires() {
    let findings = composition_findings("app.register_typed_snapshot_projection(\"x\", || None);");
    assert!(
        findings.contains(&"register_typed_snapshot_projection("),
        "negative fixture should catch direct nmp-ffi projection registration"
    );
}

#[test]
fn symbol_allowlist_negative_fixture_fires() {
    let symbol = exported_native_nmp_symbol(
        "pub extern \"C\" fn nmp_app_custom_shortcut(app: *mut NmpApp) {}",
    );
    assert_eq!(symbol, Some("nmp_app_custom_shortcut"));
    assert!(
        !allowed_native_nmp_symbols().contains(symbol.unwrap()),
        "negative fixture symbol must not be allowlisted"
    );
}

#[test]
fn no_nmp_app_c_abi_symbols_in_crates() {
    // M14-D ratchet: the nmp-ffi C-ABI crate is deleted. No code in crates/*/src/
    // may re-introduce #[no_mangle] extern "C" fn nmp_app_* symbols. #2232
    // extends this to the deleted nmp_marmot_* shell. New platform API must go
    // through nmp-native-runtime Rust methods or app-owned UniFFI facades.
    let (root, files) = crate_native_rs_files();
    let mut violations = Vec::new();

    for path in &files {
        if !is_nmp_ffi_export_source(path) {
            continue;
        }
        let body = match std::fs::read_to_string(path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for (idx, live) in rust_live_lines(&body).into_iter().enumerate() {
            if let Some(symbol) = exported_native_nmp_symbol(&live) {
                if symbol.starts_with("nmp_app_") || symbol.starts_with("nmp_marmot_") {
                    violations.push(format!(
                        "{}:{} banned C-ABI symbol `{symbol}` — crate-layer C ABI is deleted (M14-D/#2232);                          use nmp-native-runtime Rust API or an app-owned UniFFI facade instead",
                        relative_to(&root, path).display(),
                        idx + 1,
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "crates/*/src must not contain #[no_mangle] extern \"C\" fn nmp_app_* or nmp_marmot_* symbols.          The nmp-ffi C-ABI layer was deleted in M14-D and the Marmot C shell in #2232. Platform callers must use          nmp-native-runtime or app-owned UniFFI facades:\n{}",
        violations.join("\n")
    );
}
