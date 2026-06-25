//! nmp-wasm ABI-only gates (#2064).
//!
//! These are cargo/source smoke gates for the boundary that doctrine-lint runs
//! in every `doctrine_lint_smoke` pass. They deliberately enforce only the
//! narrow facts that should be mechanically stable:
//! - `nmp-wasm` must not depend on app crates, router/default composition, or
//!   signer implementation crates.
//! - the hidden raw adapter must not expose feature/composition methods as
//!   public Rust API.

use std::process::Command;

use super::workspace_root;

const ALLOWED_WASM_DEPS: &[&str] = &[
    "nmp-core",
    "nmp-feed",
    "nmp-network",
    "nmp-nip18",
    "nmp-signer-iface",
    "nmp-store",
    "js-sys",
    "serde",
    "serde_json",
    "wasm-bindgen",
    "wasm-bindgen-test",
    "console_error_panic_hook",
    "gloo-timers",
    "nostr",
];

#[test]
fn nmp_wasm_dependencies_stay_abi_only() {
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
        "nmp-wasm must remain ABI glue. Add browser/runtime composition to \
         nmp-browser-runtime or lower crates, not nmp-wasm. Forbidden deps:\n{}",
        violations.join("\n")
    );
}

#[test]
fn nmp_wasm_raw_adapter_public_methods_are_abi_only() {
    let root = workspace_root();
    let runtime_files = [
        "crates/nmp-wasm/src/runtime.rs",
        "crates/nmp-wasm/src/runtime/actions.rs",
        "crates/nmp-wasm/src/runtime/default.rs",
        "crates/nmp-wasm/src/runtime/diagnostics.rs",
        "crates/nmp-wasm/src/runtime/dispatch.rs",
        "crates/nmp-wasm/src/runtime/feed.rs",
        "crates/nmp-wasm/src/runtime/lifecycle.rs",
        "crates/nmp-wasm/src/runtime/signer.rs",
        "crates/nmp-wasm/src/runtime/test_support.rs",
    ];

    let mut violations = Vec::new();
    for relative in runtime_files {
        let path = root.join(relative);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        for (idx, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with("pub fn ") {
                continue;
            }
            let name = public_fn_name(trimmed).unwrap_or("<unknown>");
            if public_adapter_method_allowed(relative, name) {
                continue;
            }
            violations.push(format!("{}:{} pub fn {name}", relative, idx + 1));
        }
    }

    assert!(
        violations.is_empty(),
        "RawWasmAbiAdapter may expose only ABI-neutral methods (`new`, `handle`, \
         `dispatch_bytes`) plus native `*_for_test` helpers. Move composition \
         hooks to nmp-browser-runtime or keep them crate-private:\n{}",
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
        lib.contains("#[doc(hidden)]\npub use runtime::RawWasmAbiAdapter;"),
        "nmp-wasm must expose RawWasmAbiAdapter only as a hidden internal ABI adapter"
    );
}

fn public_fn_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("pub fn ")?;
    rest.split(|c| c == '(' || c == '<').next()
}

fn public_adapter_method_allowed(relative: &str, name: &str) -> bool {
    match relative {
        "crates/nmp-wasm/src/runtime.rs" => matches!(name, "new" | "handle"),
        "crates/nmp-wasm/src/runtime/dispatch.rs" => name == "dispatch_bytes",
        "crates/nmp-wasm/src/runtime/test_support.rs" => name.ends_with("_for_test"),
        _ => false,
    }
}
