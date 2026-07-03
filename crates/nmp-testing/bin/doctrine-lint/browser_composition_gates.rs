//! Browser app-composition boundary gates (#2907).

use std::path::{Path, PathBuf};
use std::process::Command;

use super::workspace_root;

const APP_OWNED_CONCEPT_REGISTERS: &[&str] = &[
    "nmp_nip02::register",
    "nmp_nip17::register",
    "nmp_nip18::register",
    "nmp_nip22::register",
    "nmp_nip23::register",
    "nmp_nip25::register",
    "nmp_nip29::register",
    "nmp_nip50::register",
    "nmp_nip51::register",
    "nmp_nip84::register",
    "nmp_replies::register",
    "nmp_wot::register",
];

#[test]
fn browser_runtime_source_does_not_register_app_owned_concepts() {
    let root = workspace_root();
    let runtime_src = root.join("crates/nmp-browser-runtime/src");
    let mut files = Vec::new();
    collect_rs_files(&runtime_src, &mut files);
    assert!(!files.is_empty(), "browser runtime Rust sources must exist");

    let mut violations = Vec::new();
    for path in files {
        if path.to_string_lossy().contains("/runtime/tests/") {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_block_comment = false;
        for (idx, line) in body.lines().enumerate() {
            let live = strip_line_comments(line, &mut in_block_comment);
            for token in APP_OWNED_CONCEPT_REGISTERS {
                if live.contains(token) {
                    violations.push(format!(
                        "{}:{} production `{token}` call",
                        relative_to(&root, &path).display(),
                        idx + 1
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "#2907: browser runtime must not bundle app-owned concept/protocol \
         composition; leaf app/test composition roots register concepts:\n{}",
        violations.join("\n")
    );
}

#[test]
fn browser_runtime_substrate_floor_names_only_substrate_installer() {
    let root = workspace_root();
    let composition = root.join("crates/nmp-browser-runtime/src/builder/composition.rs");
    let body = std::fs::read_to_string(&composition)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", composition.display()));

    assert!(
        body.contains("nmp_substrate::install"),
        "browser runtime floor must install the substrate floor explicitly"
    );
    for token in APP_OWNED_CONCEPT_REGISTERS {
        assert!(
            !body.contains(token),
            "browser runtime floor must not register app-owned concepts; found `{token}`"
        );
    }
}

#[test]
fn browser_runtime_concept_installers_are_feature_gated() {
    let findings = browser_runtime_non_optional_dependency_findings(&[
        "nmp-nip02",
        "nmp-nip17",
        "nmp-nip18",
        "nmp-nip22",
        "nmp-nip23",
        "nmp-nip25",
        "nmp-nip29",
        "nmp-nip50",
        "nmp-nip51",
        "nmp-nip57",
        "nmp-nip84",
        "nmp-replies",
        "nmp-wot",
    ]);

    assert!(
        findings.is_empty(),
        "#2907: concept installers are app-owned composition. Browser runtime \
         may keep optional feature adapters, but must not bundle default \
         concept dependencies:\n{}",
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
