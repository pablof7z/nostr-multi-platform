//! Browser runtime boundary smoke gates (#2082).
//!
//! `nmp-browser-runtime` is the Rust composition root for browser platforms.
//! `web/packages/runtime-web` is only TypeScript ABI/Worker glue. These tests
//! keep that split visible in doctrine CI.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::{run_lint, workspace_root};

const RUNTIME_WEB_ALLOWED_DEPS: &[&str] = &["flatbuffers"];

const TS_BOUNDARY_TOKENS: &[(&str, &str)] = &[
    (
        "setInterval(",
        "polling timer; runtime-web must be message/callback driven",
    ),
    (
        "setTimeout(",
        "sleep/check timer; use Worker messages or the Rust runtime wake path",
    ),
    (
        "requestAnimationFrame(",
        "frame polling; UI rendering belongs outside runtime-web",
    ),
    (
        "window.nostr",
        "direct signer access; broker signer capabilities through Rust-owned flow",
    ),
    (
        ".signEvent(",
        "direct event signing; runtime-web must not own signing policy",
    ),
    (
        "getPublicKey(",
        "identity-provider policy; host supplies raw capability results",
    ),
    (
        "getRelays(",
        "relay-provider policy; Rust canonicalizes relay permissions",
    ),
    ("nostr-tools", "Nostr protocol library in TypeScript glue"),
    (
        "SimplePool",
        "relay routing/client policy in TypeScript glue",
    ),
    (
        "RelayPool",
        "relay routing/client policy in TypeScript glue",
    ),
    (
        "new WebSocket(",
        "browser relay transport belongs in Rust runtime",
    ),
    ("localStorage", "durable browser storage from runtime-web"),
    ("sessionStorage", "durable browser storage from runtime-web"),
    ("indexedDB", "durable browser storage from runtime-web"),
    ("caches.open(", "durable browser storage from runtime-web"),
    ("console.log(", "debug retention/logging in runtime-web"),
    ("console.debug(", "debug retention/logging in runtime-web"),
    ("privateKey", "raw private-key material in TypeScript glue"),
    ("secretKey", "raw private-key material in TypeScript glue"),
    ("nsec", "raw secret material in TypeScript glue"),
];

const BROWSER_LOCAL_FEED_SESSION_MODEL_DEFS: &[&str] =
    &["struct FeedParams", "struct FeedSessionRegistry"];
const GALLERY_DEMAND_RETRY_TOKENS: &[(&str, &str)] = &[
    (
        "setInterval(",
        "gallery must not poll/retry runtime demand from the shell",
    ),
    (
        "runtime.releaseEvent(",
        "gallery must not release/reclaim refs to force fetch retry",
    ),
];

#[test]
fn nmp_browser_runtime_is_direct_doctrine_lint_clean() {
    let (code, stdout, stderr) = run_lint(&["--path", "crates/nmp-browser-runtime/src"]);
    assert_eq!(
        code, 0,
        "nmp-browser-runtime must stay clean when CI scans it directly; \
         stdout:\n{}\nstderr:\n{}",
        stdout, stderr
    );
}

#[test]
fn browser_production_start_does_not_use_hidden_defaults_preset() {
    let root = workspace_root();
    let files = [
        root.join("crates/nmp-browser-runtime/src/builder.rs"),
        root.join("crates/nmp-browser-runtime/src/builder/composition.rs"),
    ];

    let mut violations = Vec::new();
    for file in files {
        let body = std::fs::read_to_string(&file)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", file.display()));
        let mut in_block_comment = false;
        for (idx, line) in body.lines().enumerate() {
            let live = strip_line_comments(line, &mut in_block_comment);
            for token in [
                "nmp_defaults::register_defaults(",
                "nmp_defaults::register_defaults_with(",
                "nmp_defaults::register_defaults_with_handles(",
            ] {
                if live.contains(token) {
                    violations.push(format!(
                        "{}:{} hidden `{token}` call",
                        relative_to(&root, &file).display(),
                        idx + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "browser production composition must name substrate/protocol/runtime \
         installers explicitly instead of hiding behind register_defaults:\n{}",
        violations.join("\n")
    );
}

#[test]
fn browser_production_composition_names_owner_installers() {
    let root = workspace_root();
    let composition = root.join("crates/nmp-browser-runtime/src/builder/composition.rs");
    let body = std::fs::read_to_string(&composition)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", composition.display()));

    for token in [
        "nmp_substrate::install",
        "nmp_nip50::register",
        "nmp_nip02::register",
        "nmp_replies::register",
        "nmp_nip25::register",
        "nmp_nip18::register",
        "nmp_nip84::register",
        "nmp_nip29::register",
        "nmp_wot::register",
        "nmp_nip51::register",
        "nmp_nip22::register",
        "nmp_nip17::register",
        "nmp_nip23::register",
    ] {
        assert!(
            body.contains(token),
            "browser production composition must name owner installers directly; \
             missing `{token}`"
        );
    }
}

#[test]
fn browser_runtime_search_concept_is_feature_gated() {
    let findings = browser_runtime_non_optional_dependency_findings(&["nmp-nip50"]);

    assert!(
        findings.is_empty(),
        "#2797: NIP-50 search is concept-owned composition. Keep the browser \
         runtime SearchHost implementation and worker search dispatch behind \
         the nmp-browser-runtime `search` feature:\n{}",
        findings.join("\n")
    );
}

fn browser_runtime_non_optional_dependency_findings(names: &[&str]) -> Vec<String> {
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
    let packages = metadata["packages"]
        .as_array()
        .expect("cargo metadata packages must be an array");
    let runtime = packages
        .iter()
        .find(|pkg| pkg["name"] == "nmp-browser-runtime")
        .expect("nmp-browser-runtime package must be in cargo metadata");
    let dependencies = runtime["dependencies"]
        .as_array()
        .expect("package dependencies must be an array");

    dependencies
        .iter()
        .filter_map(|dependency| {
            let name = dependency["name"].as_str().unwrap_or_default();
            if !names.contains(&name) {
                return None;
            }
            let kind = dependency["kind"].as_str().unwrap_or("normal");
            let optional = dependency["optional"].as_bool().unwrap_or(false);
            (!optional && kind != "dev").then(|| format!("nmp-browser-runtime -> {name} ({kind})"))
        })
        .collect()
}

#[test]
fn browser_runtime_uses_nmp_feed_session_model() {
    let root = workspace_root();
    let runtime_src = root.join("crates/nmp-browser-runtime/src");
    let mut files = Vec::new();
    collect_rs_files(&runtime_src, &mut files);
    assert!(!files.is_empty(), "browser runtime Rust sources must exist");

    let mut violations = Vec::new();
    for path in files {
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_block_comment = false;
        for (idx, line) in body.lines().enumerate() {
            let live = strip_line_comments(line, &mut in_block_comment);
            for token in BROWSER_LOCAL_FEED_SESSION_MODEL_DEFS {
                if live.contains(token) {
                    violations.push(format!(
                        "{}:{} local `{token}` definition",
                        relative_to(&root, &path).display(),
                        idx + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "browser-runtime must use `nmp_feed::FeedParams` and \
         `nmp_feed::FeedSessionRegistry` as the public feed session model; \
         local browser-only model definitions are forbidden:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_web_package_dependencies_stay_abi_only() {
    let root = workspace_root();
    let package_json = root.join("web/packages/runtime-web/package.json");
    let body = std::fs::read_to_string(&package_json)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", package_json.display()));
    let manifest: serde_json::Value =
        serde_json::from_str(&body).expect("runtime-web package.json must parse");
    let deps = manifest["dependencies"]
        .as_object()
        .expect("runtime-web dependencies must be an object");

    let mut violations = Vec::new();
    for name in deps.keys() {
        if !RUNTIME_WEB_ALLOWED_DEPS.contains(&name.as_str()) {
            violations.push(name.clone());
        }
    }

    assert!(
        violations.is_empty(),
        "runtime-web must stay ABI/Worker glue. Protocol, relay, signer, \
         storage, or app policy dependencies belong in Rust or app shells, not \
         this package:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_web_sources_have_no_policy_polling_or_secret_retention() {
    let root = workspace_root();
    let src_root = root.join("web/packages/runtime-web/src");
    let mut files = Vec::new();
    collect_ts_files(&src_root, &mut files);
    assert!(!files.is_empty(), "runtime-web TS sources must exist");

    let mut violations = Vec::new();
    for path in files {
        if should_skip_runtime_web_source(&path) {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_block_comment = false;
        for (idx, line) in body.lines().enumerate() {
            let live = strip_line_comments(line, &mut in_block_comment);
            for finding in ts_boundary_findings(&live) {
                violations.push(format!(
                    "{}:{} {}",
                    relative_to(&root, &path).display(),
                    idx + 1,
                    finding
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "runtime-web must not grow protocol/routing/signing policy, polling, \
         durable storage, or raw secret/debug retention:\n{}",
        violations.join("\n")
    );
}

#[test]
fn runtime_web_boundary_checker_flags_obvious_violations() {
    for (line, needle) in [
        ("setInterval(() => poll(), 1000);", "polling"),
        ("window.nostr.signEvent(event);", "signing"),
        (
            "import { SimplePool } from \"nostr-tools\";",
            "Nostr protocol",
        ),
        ("localStorage.setItem(\"nsec\", value);", "storage"),
    ] {
        let findings = ts_boundary_findings(line);
        assert!(
            findings.iter().any(|f| f.contains(needle)),
            "expected `{line}` to produce a finding containing `{needle}`; got {findings:?}"
        );
    }

    for line in [
        "scope.postMessage({ type: \"update_bytes\", bytes });",
        "const routing = decodeDispatchEnvelopeRouting(request.bytes);",
    ] {
        let findings = ts_boundary_findings(line);
        assert!(
            findings.is_empty(),
            "expected `{line}` to stay clean; got {findings:?}"
        );
    }
}

#[test]
fn gallery_app_does_not_retry_runtime_demand_from_shell() {
    let root = workspace_root();
    let src_root = root.join("web/nmp-gallery/src");
    let mut files = Vec::new();
    collect_ts_files(&src_root, &mut files);
    assert!(!files.is_empty(), "nmp-gallery TS sources must exist");

    let mut violations = Vec::new();
    for path in files {
        if should_skip_gallery_source(&path) {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_block_comment = false;
        for (idx, line) in body.lines().enumerate() {
            let live = strip_line_comments(line, &mut in_block_comment);
            for (token, message) in GALLERY_DEMAND_RETRY_TOKENS {
                if live.contains(token) {
                    violations.push(format!(
                        "{}:{} `{token}` {message}",
                        relative_to(&root, &path).display(),
                        idx + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "nmp-gallery renders components and declares demand; runtime/kernel own \
         ref fetch mechanics, retry/wake behavior, and transport buffering:\n{}",
        violations.join("\n")
    );
}

fn collect_ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_ts_files(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts" | "tsx")
        ) {
            out.push(path);
        }
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("rs")) {
            out.push(path);
        }
    }
}

fn should_skip_runtime_web_source(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    name.ends_with(".generated.ts") || name.ends_with(".test.ts")
}

fn should_skip_gallery_source(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| part == "generated")
    }) || should_skip_runtime_web_source(path)
}

fn ts_boundary_findings(line: &str) -> Vec<String> {
    let mut findings = Vec::new();
    for (token, message) in TS_BOUNDARY_TOKENS {
        if line.contains(token) {
            findings.push(format!("`{token}` violates browser boundary: {message}"));
        }
    }
    findings
}

fn strip_line_comments(line: &str, in_block_comment: &mut bool) -> String {
    let mut out = String::new();
    let mut rest = line;
    loop {
        if *in_block_comment {
            if let Some(end) = rest.find("*/") {
                rest = &rest[end + 2..];
                *in_block_comment = false;
            } else {
                break;
            }
        } else if let Some(line_comment) = rest.find("//") {
            let before = &rest[..line_comment];
            append_until_block_comment(before, &mut out, in_block_comment);
            break;
        } else {
            append_until_block_comment(rest, &mut out, in_block_comment);
            break;
        }
    }
    out
}

fn append_until_block_comment(segment: &str, out: &mut String, in_block_comment: &mut bool) {
    if let Some(start) = segment.find("/*") {
        out.push_str(&segment[..start]);
        *in_block_comment = true;
    } else {
        out.push_str(segment);
    }
}

fn relative_to(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}
