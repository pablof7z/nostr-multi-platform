//! Trellis public-surface leak ratchets (#2634).
//!
//! Trellis may be used by private NMP internals and validation tests. Public
//! app/native/web surfaces must continue to expose NMP typed sessions,
//! projections, actions, and outputs rather than raw Trellis graph primitives.

use std::path::{Path, PathBuf};

use super::workspace_root;

const RUST_PUBLIC_SURFACE_ROOTS: &[&str] = &[
    "crates/nmp-browser-runtime/src",
    "crates/nmp-feed/src",
    "crates/nmp-native-runtime/src",
    "crates/nmp-uniffi-support/src",
    "crates/nmp-uniffi/src",
];

const BUILDER_SURFACE_ROOTS: &[&str] = &[
    "crates/nmp-example-login-timeline/src",
    "docs/builder-guide",
    "docs/recipes",
    "web/nmp-gallery/src",
    "web/packages/components-web/src",
    "web/packages/runtime-web/src",
];

const ALLOWED_PRIVATE_TRELLIS_PATHS: &[&str] = &[
    "crates/nmp-nip02/src/active_follow_set/reactive_graph.rs",
    "crates/nmp-feed-session/src/trellis_resources.rs",
    "crates/nmp-feed-session/src/trellis_resources_tests.rs",
    "crates/nmp-testing/tests/trellis_read_session_contract.rs",
];

const TRELLIS_TOKENS: &[&str] = &[
    "trellis_core",
    "trellis-core",
    "Graph",
    "DependencyList",
    "InputNode",
    "DerivedNode",
    "ScopeId",
    "ResourceKey",
    "ResourcePlan",
    "ResourceCommand",
    "HostResourceStatus",
    "OutputFrame",
    "OutputFrameKind",
    "MaterializedOutput",
    "TransactionResult",
];

#[test]
fn trellis_primitives_do_not_leak_to_native_web_or_app_facing_rust_surfaces() {
    let root = workspace_root();
    let mut files = Vec::new();
    for rel in RUST_PUBLIC_SURFACE_ROOTS {
        collect_files(&root.join(rel), &mut files, &["rs", "udl"]);
    }

    let violations = scan_files_for_trellis_tokens(&root, &files, SurfaceMode::Rust);
    assert!(
        violations.is_empty(),
        "raw Trellis primitives must stay out of native/web/app-facing Rust APIs; \
         expose NMP typed session/action/projection surfaces instead:\n{}",
        violations.join("\n")
    );
}

#[test]
fn builder_docs_and_examples_do_not_teach_raw_trellis_graph_assembly() {
    let root = workspace_root();
    let mut files = Vec::new();
    for rel in BUILDER_SURFACE_ROOTS {
        collect_files(&root.join(rel), &mut files, &["md", "rs", "ts", "tsx"]);
    }

    let violations = scan_files_for_trellis_tokens(&root, &files, SurfaceMode::DocsAndExamples);
    assert!(
        violations.is_empty(),
        "builder-facing docs/examples must teach NMP typed sessions, not raw \
         Trellis graph/resource/output assembly:\n{}",
        violations.join("\n")
    );
}

#[test]
fn private_trellis_internals_are_not_part_of_the_public_surface_scan() {
    let root = workspace_root();
    let scanned = public_surface_files(&root);
    for rel in ALLOWED_PRIVATE_TRELLIS_PATHS {
        let path = normalize_path(&root.join(rel));
        assert!(
            !scanned.contains(&path),
            "`{rel}` is a private/internal Trellis owner and must not be \
             pulled into the public-surface leak scan"
        );
    }
}

#[test]
fn trellis_token_matcher_flags_raw_primitives_without_matching_longer_names() {
    let hits = trellis_token_hits(
        "pub fn leak() -> trellis_core::Graph<ResourceKey, OutputFrame> { todo!() }",
    );
    assert!(hits.contains(&"trellis_core"));
    assert!(hits.contains(&"Graph"));
    assert!(hits.contains(&"ResourceKey"));
    assert!(hits.contains(&"OutputFrame"));

    let longer_names = trellis_token_hits("SocialGraph ResourceKeyed OutputFrameKindness");
    assert!(
        longer_names.is_empty(),
        "Trellis primitive matching should require identifier boundaries"
    );
}

#[test]
fn rust_surface_scanner_ignores_commented_trellis_mentions() {
    let mut in_block_comment = false;
    let live = strip_line_comments(
        "let visible = 1; // trellis_core::Graph<ResourceKey>",
        &mut in_block_comment,
    );
    assert!(trellis_token_hits(&live).is_empty());

    let mut in_block_comment = false;
    let live = strip_line_comments("let visible = /* Graph */ 1;", &mut in_block_comment);
    assert!(trellis_token_hits(&live).is_empty());
}

fn public_surface_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for rel in RUST_PUBLIC_SURFACE_ROOTS {
        collect_files(&root.join(rel), &mut files, &["rs", "udl"]);
    }
    for rel in BUILDER_SURFACE_ROOTS {
        collect_files(&root.join(rel), &mut files, &["md", "rs", "ts", "tsx"]);
    }
    files
        .into_iter()
        .map(|path| normalize_path(&path))
        .collect()
}

#[derive(Clone, Copy)]
enum SurfaceMode {
    Rust,
    DocsAndExamples,
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, extensions: &[&str]) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|name| name.to_str()) == Some("generated") {
                continue;
            }
            collect_files(&path, out, extensions);
        } else if file_extension_is(&path, extensions) && !is_test_file(&path) {
            out.push(path);
        }
    }
}

fn file_extension_is(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| extensions.contains(&ext))
}

fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.contains("_tests.")
                || name.contains(".test.")
                || name.contains(".spec.")
                || name == "tests.rs"
        })
}

fn scan_files_for_trellis_tokens(root: &Path, files: &[PathBuf], mode: SurfaceMode) -> Vec<String> {
    let mut violations = Vec::new();
    for path in files {
        let body = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_block_comment = false;
        for (idx, line) in body.lines().enumerate() {
            let live = match mode {
                SurfaceMode::Rust => strip_line_comments(line, &mut in_block_comment),
                SurfaceMode::DocsAndExamples => line.to_string(),
            };
            for token in trellis_token_hits(&live) {
                violations.push(format!(
                    "{}:{} raw Trellis token `{token}`",
                    relative_to(root, path).display(),
                    idx + 1
                ));
            }
        }
    }
    violations
}

fn trellis_token_hits(line: &str) -> Vec<&'static str> {
    TRELLIS_TOKENS
        .iter()
        .copied()
        .filter(|token| line_contains_token(line, token))
        .collect()
}

fn line_contains_token(line: &str, token: &str) -> bool {
    if token.contains('-') || token.contains('_') {
        return line.contains(token);
    }
    let mut start = 0;
    while let Some(rel) = line[start..].find(token) {
        let abs = start + rel;
        let before = abs
            .checked_sub(1)
            .and_then(|idx| line.as_bytes().get(idx))
            .copied();
        let after = line.as_bytes().get(abs + token.len()).copied();
        if !is_ident_byte(before) && !is_ident_byte(after) {
            return true;
        }
        start = abs + token.len();
    }
    false
}

fn is_ident_byte(value: Option<u8>) -> bool {
    value.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn strip_line_comments(line: &str, in_block: &mut bool) -> String {
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

fn append_until_block_comment(input: &str, out: &mut String, in_block: &mut bool) {
    let mut rest = input;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        if let Some(end) = rest[start + 2..].find("*/") {
            rest = &rest[start + 2 + end + 2..];
        } else {
            *in_block = true;
            return;
        }
    }
    out.push_str(rest);
}

fn relative_to(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn normalize_path(path: &Path) -> PathBuf {
    path.components().collect()
}
