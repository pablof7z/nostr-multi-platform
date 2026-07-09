//! Phase A Trellis diagnostic-surface gates for #2858.

use std::collections::BTreeSet;

use super::trellis_phase_a_fixture_support::run_receipt_render_fixture;
use super::trellis_phase_a_support::{
    cargo_metadata, dependency_path, release_manifest, trellis_manifest_dependencies,
    workspace_dependency_graph,
};
use super::trellis_public_api_support::DIAGNOSTIC_TRELLIS_SURFACE_ROOTS;
use super::workspace_root;

const EXPECTED_DIAGNOSTIC_SURFACES: &[&str] = &["crates/nmp-devtools/src"];

const ALLOWED_TRELLIS_MANIFEST_DEPS: &[(&str, &str)] = &[
    ("crates/nmp-devtools/Cargo.toml", "trellis-core"),
    ("crates/nmp-feed-session/Cargo.toml", "trellis-core"),
    ("crates/nmp-nip02/Cargo.toml", "trellis-core"),
    ("crates/nmp-nip51/Cargo.toml", "trellis-core"),
    ("crates/nmp-testing/Cargo.toml", "trellis-core"),
    ("crates/nmp-testing/Cargo.toml", "trellis-testing"),
    // #3115/#3116 — owner-directed widening of the Phase A ratchet: "default
    // = migrate every hand-rolled reconciler onto Trellis" is now the
    // posture, and the reusable keyed-reconciler core the migrations share
    // colocates in `nmp-core` (consumed directly by `nmp-read-session`,
    // which applies its `ResourceCommand<C>` output). Both are external-leaf
    // deps, no NMP-graph cycle — see `docs/architecture/crate-boundaries.md`.
    ("crates/nmp-core/Cargo.toml", "trellis-core"),
    ("crates/nmp-read-session/Cargo.toml", "trellis-core"),
    // #3116 settle-gate finding 1 — nmp-nip17's peer relay-list reconciler
    // was the last surviving hand-rolled family-shape reconciler the sweep
    // missed; it applies `ResourceCommand<()>` directly, same shape as the
    // two entries above.
    ("crates/nmp-nip17/Cargo.toml", "trellis-core"),
];

#[test]
fn diagnostic_trellis_surface_allowlist_stays_single_devtools_source_root() {
    assert_eq!(
        DIAGNOSTIC_TRELLIS_SURFACE_ROOTS, EXPECTED_DIAGNOSTIC_SURFACES,
        "#2858 Phase A allows raw Trellis vocabulary only inside the \
         dev-build-only diagnostic crate source root"
    );
}

#[test]
fn nmp_devtools_stays_private_and_out_of_public_release_train() {
    let manifest = release_manifest();
    assert!(
        !manifest.public_crates.contains_key("nmp-devtools"),
        "nmp-devtools is dev-build-only diagnostics and must not be a public release crate"
    );
    assert_eq!(
        manifest
            .private_packages
            .get("nmp-devtools")
            .map(String::as_str),
        Some("crates/nmp-devtools"),
        "nmp-devtools must stay classified as a private package in release/nmp-release.toml"
    );
}

#[test]
fn public_release_crate_graph_does_not_link_nmp_devtools() {
    let metadata = cargo_metadata();
    let release = release_manifest();
    let graph = workspace_dependency_graph(&metadata);
    let mut violations = Vec::new();

    for crate_name in release.public_crates.keys() {
        if let Some(path) = dependency_path(&graph, crate_name, "nmp-devtools") {
            violations.push(path.join(" -> "));
        }
    }

    assert!(
        violations.is_empty(),
        "public release crates must not have a normal/build dependency path to \
         the dev-only nmp-devtools sidecar:\n{}",
        violations.join("\n")
    );
}

#[test]
fn trellis_dependency_allowlist_is_closed_to_existing_private_adapters() {
    let root = workspace_root();
    let actual = trellis_manifest_dependencies(&root);
    let allowed: BTreeSet<_> = ALLOWED_TRELLIS_MANIFEST_DEPS.iter().copied().collect();
    let unexpected: Vec<_> = actual
        .iter()
        .filter(|dep| !allowed.contains(&(dep.manifest.as_str(), dep.dependency.as_str())))
        .map(|dep| format!("{}:{} {}", dep.manifest, dep.line_no, dep.dependency))
        .collect();

    assert!(
        unexpected.is_empty(),
        "new trellis-* Cargo dependency lines are a #2858 ratchet violation; \
         keep Trellis private to the existing adapters/devtools/tests:\n{}",
        unexpected.join("\n")
    );
}

#[test]
fn rendered_devtools_receipts_do_not_leak_raw_trellis_vocabulary() {
    let root = workspace_root();
    run_receipt_render_fixture(&root);
}
