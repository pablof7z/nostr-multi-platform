//! nmp-wasm protocol-only gates (#2064 / #2202).
//!
//! These are cargo/source smoke gates for the boundary that doctrine-lint runs
//! in every `doctrine_lint_smoke` pass. They deliberately enforce only the
//! narrow facts that should be mechanically stable:
//! - `nmp-wasm` must not depend on runtime, app, router/default composition, or
//!   signer implementation crates.
//! - it must build as an rlib-only Rust crate, not as a browser wasm artifact.
//! - the retired raw adapter path must stay deleted.

use std::process::Command;

use super::workspace_root;

const ALLOWED_WASM_DEPS: &[&str] = &["serde", "serde_json"];

#[test]
fn nmp_wasm_dependencies_stay_protocol_only() {
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
    let package = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array")
        .iter()
        .find(|package| package["name"] == "nmp-wasm")
        .expect("nmp-wasm package must exist");

    let dependencies = package["dependencies"]
        .as_array()
        .expect("nmp-wasm dependencies must be an array");
    let mut violations = Vec::new();
    for dependency in dependencies {
        let dep_name = dependency["name"].as_str().unwrap_or("<unnamed>");
        if ALLOWED_WASM_DEPS.contains(&dep_name) {
            continue;
        }
        violations.push(dep_name.to_string());
    }

    assert!(
        violations.is_empty(),
        "nmp-wasm must remain protocol types only. Add browser/runtime composition to \
         nmp-browser-runtime or lower crates, not nmp-wasm. Forbidden deps:\n{}",
        violations.join("\n")
    );

    let targets = package["targets"]
        .as_array()
        .expect("nmp-wasm targets must be an array");
    let lib_target = targets
        .iter()
        .find(|target| target["name"].as_str() == Some("nmp_wasm"))
        .expect("nmp-wasm must have a lib target");
    let crate_types: Vec<&str> = lib_target["crate_types"]
        .as_array()
        .expect("nmp-wasm crate_types must be an array")
        .iter()
        .filter_map(|ty| ty.as_str())
        .collect();
    assert_eq!(
        crate_types,
        ["rlib"],
        "nmp-wasm is not a browser artifact crate; nmp-browser-runtime owns wasm-bindgen exports"
    );
}

#[test]
fn nmp_wasm_raw_adapter_path_is_retired() {
    let root = workspace_root();
    let retired_paths = [
        "crates/nmp-wasm/src/runtime.rs",
        "crates/nmp-wasm/src/runtime/actions.rs",
        "crates/nmp-wasm/src/runtime/default.rs",
        "crates/nmp-wasm/src/runtime/diagnostics.rs",
        "crates/nmp-wasm/src/runtime/dispatch.rs",
        "crates/nmp-wasm/src/runtime/feed.rs",
        "crates/nmp-wasm/src/runtime/lifecycle.rs",
        "crates/nmp-wasm/src/runtime/signer.rs",
        "crates/nmp-wasm/src/runtime/test_support.rs",
        "crates/nmp-wasm/src/relay_pool.rs",
        "crates/nmp-wasm/src/relay_plan.rs",
        "crates/nmp-wasm/src/dispatch_routing.rs",
        "crates/nmp-wasm/src/signer_slot.rs",
        "crates/nmp-wasm/src/snapshot.rs",
        "crates/nmp-wasm/src/tick.rs",
    ];

    let mut violations = Vec::new();
    for relative in retired_paths {
        if root.join(relative).exists() {
            violations.push(relative);
        }
    }

    assert!(
        violations.is_empty(),
        "legacy nmp-wasm runtime/adapter files must stay retired; browser \
         runtime behavior belongs in nmp-browser-runtime:\n{}",
        violations.join("\n")
    );
}

#[test]
fn nmp_wasm_exports_hidden_raw_adapter_not_runtime_facade() {
    let root = workspace_root();
    let lib = std::fs::read_to_string(root.join("crates/nmp-wasm/src/lib.rs"))
        .expect("nmp-wasm lib.rs must read");

    assert!(
        !lib.contains("pub use runtime::{WasmRuntime"),
        "nmp-wasm must not re-export the old public WasmRuntime facade"
    );
    assert!(
        !lib.contains("RawWasmAbiAdapter"),
        "nmp-wasm must not expose the retired RawWasmAbiAdapter"
    );
}
