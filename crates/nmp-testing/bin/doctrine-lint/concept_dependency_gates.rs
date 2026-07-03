//! #2899 concept-crate dependency gate (binding/codegen layer).
//!
//! Extends #2797's dependency ratchet one layer up: the binding-support crate
//! `nmp-uniffi-support` and the codegen crate `nmp-codegen` MUST NOT take any
//! production dependency on a `nmp-nip*` protocol crate or a concept-owned read
//! crate (`nmp-replies`/`nmp-reactions`/`nmp-reposts`/`nmp-zaps`). The whole
//! point of `nmp gen concept-reads` is that codegen emits TEXT naming
//! `nmp_replies::...` etc. and never links the concept crate; the generated
//! facade slice compiles inside the app facade crate, which already depends on
//! exactly the concepts it composes. If either crate ever grew such an edge,
//! the "no recentralized concept deps" acceptance criterion (#2899) would be
//! silently violated. This is allowlist-only (there is NO allowed concept
//! edge).
//!
//! Split out of `native_runtime_boundary_gates.rs` to keep that file under the
//! 500-LOC hard ceiling (AGENTS.md / V-12).

use std::process::Command;

use crate::workspace_root;

/// The concept-owned read crates whose exports `nmp gen concept-reads`
/// generates (as TEXT) but must never be linked by the binding/codegen layer.
const CONCEPT_READ_CRATES: &[&str] = &["nmp-replies", "nmp-reactions", "nmp-reposts", "nmp-zaps"];

fn cargo_metadata() -> serde_json::Value {
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

/// Every production (non-dev, non-optional) `nmp-nip*`/concept-crate
/// dependency of `package_name`, formatted as findings.
fn concept_or_nip_dependency_findings(package_name: &str) -> Vec<String> {
    let metadata = cargo_metadata();
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let package = packages
        .iter()
        .find(|pkg| pkg["name"] == package_name)
        .unwrap_or_else(|| panic!("{package_name} package must be in cargo metadata"));
    let dependencies = package["dependencies"]
        .as_array()
        .expect("package dependencies must be an array");

    dependencies
        .iter()
        .filter_map(|dependency| {
            let name = dependency["name"].as_str().unwrap_or_default();
            let is_nip = name.starts_with("nmp-nip");
            let is_concept = CONCEPT_READ_CRATES.contains(&name);
            if !is_nip && !is_concept {
                return None;
            }
            let kind = dependency["kind"].as_str().unwrap_or("normal");
            let optional = dependency["optional"].as_bool().unwrap_or(false);
            (!optional && kind != "dev").then(|| format!("{package_name} -> {name} ({kind})"))
        })
        .collect()
}

#[test]
fn nmp_uniffi_support_has_no_concept_or_nip_dependency() {
    let findings = concept_or_nip_dependency_findings("nmp-uniffi-support");
    assert!(
        findings.is_empty(),
        "#2899: nmp-uniffi-support is the shared binding-support layer; it must \
         NOT link any nmp-nip*/concept-owned read crate. `nmp gen concept-reads` \
         emits the concept-read facade slice as TEXT inside each app facade crate \
         (which already composes those concepts), so the binding-support layer \
         never re-centralizes a concept dependency:\n{}",
        findings.join("\n")
    );
}

#[test]
fn nmp_codegen_has_no_concept_or_nip_dependency() {
    let findings = concept_or_nip_dependency_findings("nmp-codegen");
    assert!(
        findings.is_empty(),
        "#2899: nmp-codegen must NOT link any nmp-nip*/concept-owned read crate. \
         The concept-read registry names `nmp_replies::...` etc. as generated \
         TEXT (like ACTION_BUILDERS names NIP-51 builders without depending on \
         nmp-nip51); a real cargo edge would recentralize the concept dep the \
         whole design avoids:\n{}",
        findings.join("\n")
    );
}

#[test]
fn native_runtime_does_not_link_concept_read_crates_for_the_binding() {
    // #2899 acceptance names nmp-native-runtime specifically. Unlike
    // nmp-browser-runtime — a §10a composition-root delivery surface that
    // composes `nmp-replies` via `register()` like a leaf app runtime, an
    // established pre-#2899 edge governed by #2797 — the native runtime does
    // not compose the concept-owned reads itself, so it must carry NONE of the
    // four concept-read crates as a production dependency. The concept-read
    // doors reach a native app only through the generated per-app facade slice,
    // never by linking the concept crate into the shared native runtime.
    let findings: Vec<String> = concept_or_nip_dependency_findings("nmp-native-runtime")
        .into_iter()
        .filter(|f| CONCEPT_READ_CRATES.iter().any(|c| f.contains(c)))
        .collect();
    assert!(
        findings.is_empty(),
        "#2899: nmp-native-runtime must not take a production dependency on a \
         concept-owned read crate (nmp-replies/nmp-reactions/nmp-reposts/\
         nmp-zaps); those reads reach native apps via the generated facade \
         slice:\n{}",
        findings.join("\n")
    );
}

#[test]
fn concept_dependency_gate_negative_fixture_fires() {
    // Prove the gate trips on the bad case: a synthetic package that DOES
    // depend on a concept crate must be caught by the same filter.
    let packages = serde_json::json!({
        "packages": [{
            "name": "nmp-uniffi-support",
            "dependencies": [{
                "name": "nmp-replies",
                "kind": null,
                "optional": false
            }]
        }]
    });
    // Reuse the shared filter shape used by the live gate by inlining the check
    // against this fixture (cargo_metadata is not overridable).
    let findings: Vec<String> = packages["packages"][0]["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|dependency| {
            let name = dependency["name"].as_str().unwrap_or_default();
            let is_nip = name.starts_with("nmp-nip");
            let is_concept = CONCEPT_READ_CRATES.contains(&name);
            if !is_nip && !is_concept {
                return None;
            }
            let optional = dependency["optional"].as_bool().unwrap_or(false);
            (!optional).then(|| format!("nmp-uniffi-support -> {name}"))
        })
        .collect();
    assert!(
        findings.iter().any(|f| f.contains("nmp-replies")),
        "negative fixture should flag a concept-crate dependency; got {findings:?}"
    );
}
