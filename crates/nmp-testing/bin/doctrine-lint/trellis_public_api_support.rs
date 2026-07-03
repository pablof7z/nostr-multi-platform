//! Shared helpers for Trellis public-surface doctrine gates.

use std::path::{Path, PathBuf};

pub(crate) const RUST_PUBLIC_SURFACE_ROOTS: &[&str] = &[
    "crates/nmp-browser-runtime/src",
    "crates/nmp-feed/src",
    "crates/nmp-native-runtime/src",
    "crates/nmp-uniffi-support/src",
];

pub(crate) const BUILDER_SURFACE_ROOTS: &[&str] = &[
    "crates/nmp-example-login-timeline/src",
    "docs/builder-guide",
    "docs/recipes",
    "web/nmp-gallery/src",
    "web/packages/components-web/src",
    "web/packages/runtime-web/src",
];

pub(crate) const DIAGNOSTIC_TRELLIS_SURFACE_ROOTS: &[&str] = &["crates/nmp-devtools/src"];

pub(crate) const ALLOWED_PRIVATE_TRELLIS_PATHS: &[&str] = &[
    "crates/nmp-nip02/src/active_follow_set/reactive_graph.rs",
    "crates/nmp-feed-session/src/trellis_adapter.rs",
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

#[derive(Clone, Copy)]
pub(crate) enum SurfaceMode {
    Rust,
    DocsAndExamples,
}

pub(crate) fn public_surface_files(root: &Path) -> Vec<PathBuf> {
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

pub(crate) fn collect_files(dir: &Path, out: &mut Vec<PathBuf>, extensions: &[&str]) {
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

pub(crate) fn scan_files_for_trellis_tokens(
    root: &Path,
    files: &[PathBuf],
    mode: SurfaceMode,
) -> Vec<String> {
    let mut violations = Vec::new();
    for path in files {
        if is_diagnostic_trellis_surface(root, path) {
            continue;
        }
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

pub(crate) fn is_diagnostic_trellis_surface(root: &Path, path: &Path) -> bool {
    let normalized = normalize_path(path);
    DIAGNOSTIC_TRELLIS_SURFACE_ROOTS
        .iter()
        .map(|rel| normalize_path(&root.join(rel)))
        .any(|surface_root| normalized.starts_with(surface_root))
}

pub(crate) fn trellis_token_hits(line: &str) -> Vec<&'static str> {
    TRELLIS_TOKENS
        .iter()
        .copied()
        .filter(|token| line_contains_token(line, token))
        .collect()
}

pub(crate) fn strip_line_comments(line: &str, in_block: &mut bool) -> String {
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

pub(crate) fn relative_to(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    path.components().collect()
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
