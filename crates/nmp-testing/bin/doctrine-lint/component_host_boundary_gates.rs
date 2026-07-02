//! Component-host boundary gates (#2257).
//!
//! Registry UI components are app-owned source packages. They may render and
//! report visible claim/release lifecycle through the app-level host/provider,
//! but they must not import platform runtimes, C ABI/JNI/WASM workers, or kernel
//! handles directly.

use std::path::{Path, PathBuf};

use super::workspace_root;

const FORBIDDEN_COMPONENT_TOKENS: &[(&str, &str)] = &[
    ("nmp-ffi", "C ABI crate"),
    ("nmp_ffi", "C ABI crate/module"),
    ("nmp_app_", "C ABI symbol"),
    ("nmp-native-runtime", "native runtime crate"),
    ("nmp_native_runtime", "native runtime module"),
    ("nmp-browser-runtime", "browser runtime crate"),
    ("nmp_browser_runtime", "browser runtime module"),
    ("nmp-wasm", "WASM protocol/runtime crate"),
    ("nmp_wasm", "WASM protocol/runtime module"),
    ("@nmpis/runtime-web", "runtime-web Worker bridge package"),
    ("@nmpis/browser-runtime", "browser runtime package"),
    ("wasmBridge", "browser Worker bridge handle"),
    ("new Worker", "browser Worker handle"),
    ("Worker(", "browser Worker constructor"),
    ("KernelBridge", "app kernel bridge"),
    ("GalleryKernelBridge", "gallery kernel bridge"),
    ("NmpAppBuilder", "kernel app builder"),
    ("NmpApp", "kernel app handle"),
    ("JNIEnv", "JNI boundary"),
    ("external fun native", "JNI/native boundary"),
];

const FORBIDDEN_WEB_DEPS: &[&str] = &[
    "@nmpis/runtime-web",
    "@nmpis/browser-runtime",
    "nmp-browser-runtime",
    "nmp-wasm",
    "nmp-ffi",
    "nmp-native-runtime",
    "nostr-tools",
];

#[test]
fn native_registry_components_do_not_import_runtime_abi_or_kernel_internals() {
    let root = workspace_root();
    let mut files = Vec::new();
    for dir in [
        root.join("crates/nmp-component-registry/registry/swiftui"),
        root.join("crates/nmp-component-registry/registry/compose"),
    ] {
        collect_component_files(&dir, &mut files);
    }

    assert!(
        !files.is_empty(),
        "native registry component files must exist"
    );
    let violations = boundary_violations(&root, &files);
    assert!(
        violations.is_empty(),
        "SwiftUI/Compose registry components must consume the app-level \
         component host/provider only. Runtime, ABI, worker, JNI, and kernel \
         handles belong in app roots or Rust crates:\n{}",
        violations.join("\n")
    );
}

#[test]
fn web_component_package_does_not_import_runtime_worker_or_kernel_internals() {
    let root = workspace_root();
    let src_root = root.join("web/packages/components-web/src");
    let mut files = Vec::new();
    collect_component_files(&src_root, &mut files);

    assert!(!files.is_empty(), "components-web source files must exist");
    let violations = boundary_violations(&root, &files);
    assert!(
        violations.is_empty(),
        "web/packages/components-web must stay pure component source over \
         NmpComponentHostProvider. Runtime-web, workers, ABI crates, and kernel \
         handles belong in the app runtime package:\n{}",
        violations.join("\n")
    );
}

#[test]
fn web_component_package_dependencies_stay_component_only() {
    let root = workspace_root();
    let package_json = root.join("web/packages/components-web/package.json");
    let body = std::fs::read_to_string(&package_json)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", package_json.display()));
    let manifest: serde_json::Value =
        serde_json::from_str(&body).expect("components-web package.json must parse");

    let mut violations = Vec::new();
    for field in ["dependencies", "peerDependencies", "devDependencies"] {
        let Some(deps) = manifest[field].as_object() else {
            continue;
        };
        for name in deps.keys() {
            if FORBIDDEN_WEB_DEPS.contains(&name.as_str()) {
                violations.push(format!("{field}.{name}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "components-web must not depend on runtime, ABI, worker, or protocol \
         policy packages:\n{}",
        violations.join("\n")
    );
}

#[test]
fn component_boundary_checker_flags_obvious_violations() {
    let findings = boundary_token_findings("import { runtime } from \"@nmpis/runtime-web\";");
    assert!(
        findings.iter().any(|f| f.contains("runtime-web")),
        "runtime-web import must be flagged: {findings:?}"
    );

    let findings = boundary_token_findings("let app: NmpApp? = nil");
    assert!(
        findings.iter().any(|f| f.contains("kernel app handle")),
        "direct NmpApp handle must be flagged: {findings:?}"
    );

    let findings = boundary_token_findings("NmpComponentHostProvider(profileHost = host) { }");
    assert!(
        findings.is_empty(),
        "component host provider itself must stay allowed: {findings:?}"
    );
}

fn collect_component_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_component_files(&path, out);
        } else if is_component_source(&path) && !is_component_test(&path) {
            out.push(path);
        }
    }
}

fn is_component_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("swift" | "kt" | "ts" | "tsx")
    )
}

fn is_component_test(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(".test.") || name.contains(".spec."))
}

fn boundary_violations(root: &Path, files: &[PathBuf]) -> Vec<String> {
    let mut violations = Vec::new();
    for path in files {
        let body = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_block = false;
        for (idx, line) in body.lines().enumerate() {
            let live = strip_comments(line, &mut in_block);
            for finding in boundary_token_findings(&live) {
                violations.push(format!(
                    "{}:{} {}",
                    path.strip_prefix(root).unwrap_or(path).display(),
                    idx + 1,
                    finding
                ));
            }
        }
    }
    violations
}

fn boundary_token_findings(line: &str) -> Vec<String> {
    FORBIDDEN_COMPONENT_TOKENS
        .iter()
        .filter_map(|(token, reason)| {
            if line.contains(token) {
                Some(format!(
                    "`{token}` violates component-host boundary: {reason}"
                ))
            } else {
                None
            }
        })
        .collect()
}

fn strip_comments(line: &str, in_block: &mut bool) -> String {
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
            append_until_block_comment(&rest[..line_comment], &mut out, in_block);
            break;
        } else {
            append_until_block_comment(rest, &mut out, in_block);
            break;
        }
    }
    out
}

fn append_until_block_comment(segment: &str, out: &mut String, in_block: &mut bool) {
    if let Some(start) = segment.find("/*") {
        out.push_str(&segment[..start]);
        *in_block = true;
    } else {
        out.push_str(segment);
    }
}
