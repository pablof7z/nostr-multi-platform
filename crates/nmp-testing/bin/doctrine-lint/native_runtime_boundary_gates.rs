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
    allowed_nmp_ffi_symbols, cargo_metadata, composition_findings, exported_nmp_app_symbol,
    forbidden_platform_dep_findings, is_nmp_ffi_export_source, is_production_source,
    lower_layer_crates, nmp_ffi_rs_files, relative_to, rust_live_lines,
};

#[test]
fn nmp_defaults_stays_platform_runtime_free_for_production_deps() {
    let metadata = cargo_metadata();
    let findings = forbidden_platform_dep_findings(&metadata, &["nmp-defaults"]);
    assert!(
        findings.is_empty(),
        "nmp-defaults must compose through AppHost and must not take default \
         production dependencies on native/browser runtime or ABI crates:\n{}",
        findings.join("\n")
    );
}

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
fn nmp_ffi_public_nmp_app_symbols_are_allowlisted() {
    let (_, files) = nmp_ffi_rs_files();
    let allowed = allowed_nmp_ffi_symbols();
    let mut exported = std::collections::BTreeSet::new();

    for path in files {
        if !is_nmp_ffi_export_source(&path) {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for line in body.lines() {
            if let Some(symbol) = exported_nmp_app_symbol(line) {
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
        "generic nmp-ffi nmp_app_* C symbols must be explicit. Additions need \
         an accepted issue or ADR explaining why action/projection/capability/\
         runtime APIs are insufficient.\nextra:\n{}\nmissing:\n{}",
        extra.join("\n"),
        missing.join("\n")
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
            "name": "nmp-defaults",
            "dependencies": [
                {"name": "nmp-native-runtime", "kind": "dev", "optional": false},
                {"name": "nmp-ffi", "kind": null, "optional": true}
            ]
        }]
    });
    let findings = forbidden_platform_dep_findings(&packages, &["nmp-defaults"]);
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
    let symbol =
        exported_nmp_app_symbol("pub extern \"C\" fn nmp_app_custom_shortcut(app: *mut NmpApp) {}");
    assert_eq!(symbol, Some("nmp_app_custom_shortcut"));
    assert!(
        !allowed_nmp_ffi_symbols().contains(symbol.unwrap()),
        "negative fixture symbol must not be allowlisted"
    );
}
