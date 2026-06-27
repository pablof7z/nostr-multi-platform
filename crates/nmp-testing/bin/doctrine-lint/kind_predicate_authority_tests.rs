//! D4 gate: kind classification predicates live in `nmp-kinds`.
//!
//! This guards against reintroducing local literal-range predicates for the
//! replaceable, ephemeral, and addressable Nostr kind classes in production
//! crates. Consumers should call `nmp_kinds::{is_replaceable, is_ephemeral,
//! is_addressable}` instead.

use std::fs;
use std::path::{Path, PathBuf};

use super::workspace_root;

const BANNED_RANGE_PREDICATES: &[&str] = &[
    "(10_000..20_000).contains",
    "(10000..20000).contains",
    "(20_000..30_000).contains",
    "(20000..30000).contains",
    "(30_000..40_000).contains",
    "(30000..40000).contains",
];

#[test]
fn production_crates_do_not_hand_roll_kind_range_predicates() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);

    let mut findings = Vec::new();
    for path in files {
        let rel = path.strip_prefix(&root).unwrap_or(&path);
        if out_of_scope(rel) {
            continue;
        }
        let body = fs::read_to_string(&path).expect("read Rust source");
        for (idx, line) in body.lines().enumerate() {
            if is_comment(line) {
                continue;
            }
            if BANNED_RANGE_PREDICATES.iter().any(|pat| line.contains(pat)) {
                findings.push(format!("{}:{}", rel.display(), idx + 1));
            }
        }
    }

    assert!(
        findings.is_empty(),
        "kind class range predicates belong in nmp-kinds; call the canonical \
         predicate instead. Findings:\n{}",
        findings.join("\n")
    );
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn out_of_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    !s.contains("/src/")
        || s.contains("crates/nmp-kinds/src/")
        || s.contains("crates/nmp-testing/bin/doctrine-lint/")
}

fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with('*')
}
