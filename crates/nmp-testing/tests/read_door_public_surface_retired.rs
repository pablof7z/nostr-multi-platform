//! #2418 — search/notification/pointer-source read doors are typed or internal.
//!
//! This grep gate protects the slice boundary: the app-facing runtime surface
//! must not re-expose the old raw read doors after typed descriptor/handle
//! wrappers have taken ownership of the lifecycle.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}

fn code_only(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return "";
    }
    line
}

#[test]
fn retired_app_visible_read_doors_are_not_public_defs() {
    let root = repo_root();
    let checks = [
        (
            "crates/nmp-native-runtime/src/search.rs",
            &[
                "pub fn open_search(",
                "pub fn open_search_session(",
                "pub fn close_search(",
                "pub fn close_search_session(",
                "pub fn search_snapshot_bytes(",
                "pub fn search_session_snapshot_bytes(",
                "pub fn parse_search_request(",
                "pub struct Nip50SearchHandle",
                "pub struct Nip50SearchSession",
            ][..],
        ),
        (
            "crates/nmp-browser-runtime/src/runtime/search.rs",
            &["pub fn open_search(", "pub(crate) fn open_search("][..],
        ),
        (
            "crates/nmp-browser-runtime/src/runtime/notifications/mod.rs",
            &[
                "pub fn open_notifications(",
                "pub(crate) fn open_notifications(",
                "pub fn mark_notifications_read(",
                "pub(crate) fn mark_notifications_read(",
                "pub fn close_notifications(",
                "pub(crate) fn close_notifications(",
            ][..],
        ),
        (
            "crates/nmp-native-runtime/src/lib.rs",
            &[
                "pub mod op_pointer_source",
                "pub use nmp_nip50::SearchRequest",
                "Nip50SearchHandle",
                "Nip50SearchSession",
                "parse_search_request",
            ][..],
        ),
        (
            "crates/nmp-native-runtime/src/op_pointer_source/mod.rs",
            &["pub fn open_pointer_source("][..],
        ),
    ];

    let mut violations = Vec::new();
    for (relative, needles) in checks {
        let path = root.join(relative);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{} must be readable: {err}", path.display()));
        for (line_number, line) in text.lines().enumerate() {
            let code = code_only(line);
            for needle in needles {
                if code.contains(needle) {
                    violations.push(format!("{}:{}: {}", relative, line_number + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "retired app-visible read-door definitions reappeared:\n{}",
        violations.join("\n")
    );
}
