//! Native runtime split boundary gates (#2214).
//!
//! These smoke gates lock the #2205 split:
//! - `nmp-native-runtime` owns native runtime/session composition.
//! - `nmp-ffi` is C ABI glue over that runtime, not a composition root.
//! - lower layers do not take default production dependencies on platform
//!   runtime or ABI crates.

#[path = "native_runtime_boundary_support.rs"]
mod support;

use support::{
    allowed_native_nmp_symbols, cargo_metadata, composition_findings, crate_native_rs_files,
    exported_native_nmp_symbol, forbidden_platform_dep_findings, is_nmp_ffi_export_source,
    is_production_source, lower_layer_crates, nmp_ffi_rs_files, relative_to, rust_live_lines,
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

#[test]
fn nmp_ffi_does_not_register_runtime_composition() {
    let (root, files) = nmp_ffi_rs_files();
    let mut violations = Vec::new();

    for path in files {
        if !is_production_source(&path) {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for (idx, live) in rust_live_lines(&body).into_iter().enumerate() {
            for token in composition_findings(&live) {
                violations.push(format!(
                    "{}:{} forbidden runtime composition token `{token}`",
                    relative_to(&root, &path).display(),
                    idx + 1
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "nmp-ffi must stay C ABI glue over nmp-native-runtime. Runtime/session \
         composition belongs in nmp-native-runtime or app Rust crates:\n{}",
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
    // through nmp-native-runtime Rust methods or nmp-uniffi.
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
                        "{}:{} banned C-ABI symbol `{symbol}` — crate-layer C ABI is deleted (M14-D/#2232);                          use nmp-native-runtime Rust API or nmp-uniffi instead",
                        relative_to(&root, path).display(),
                        idx + 1,
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "crates/*/src must not contain #[no_mangle] extern \"C\" fn nmp_app_* or nmp_marmot_* symbols.          The nmp-ffi C-ABI layer was deleted in M14-D and the Marmot C shell in #2232. Platform callers must use          nmp-native-runtime or nmp-uniffi:\n{}",
        violations.join("\n")
    );
}
