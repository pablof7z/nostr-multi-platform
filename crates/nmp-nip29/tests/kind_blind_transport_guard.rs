//! Doctrine guard: `nmp-nip29` is **kind-blind transport** (#2513, codifying
//! the #2504/#2505 owner correction).
//!
//! NIP-29 owns the `h` / `previous` / host-pin *envelope* only. Its single
//! write surface is the kind-agnostic `nmp.nip29.publish_group_event`. It must
//! NOT:
//!   - ship per-kind named convenience actions (`react`/`unreact`/`share`/
//!     `repost`-in-group),
//!   - name a foreign-NIP event kind (kind:7 reactions, kind:5 deletions,
//!     kind:16 reposts) — those belong to the owning NIP crate (`nmp-nip25`,
//!     `nmp-nip18`) or the app/component layer,
//!   - re-introduce a per-kind *classification* of routed group events
//!     (chat-vs-thread, known-vs-unknown). A routed group event is one class:
//!     `KindClass::GroupEvent`. The owning NIP — never NIP-29 — defines what
//!     the kind means.
//!
//! This test scans the crate's own non-test source so a future patch that
//! re-introduces a per-kind action, a foreign-NIP kind constant, or a per-kind
//! group-event classification fails CI — exactly the regression that slipped in
//! via #2505 and the over-broad classification model #2530 had to delete.

use std::path::{Path, PathBuf};

/// Action namespaces that must never exist in `nmp-nip29`: each builds a
/// specific event kind, which is the owning NIP's (or the app's) job.
const BANNED_NAMESPACES: &[&str] = &[
    "nmp.nip29.react_in_group",
    "nmp.nip29.unreact_in_group",
    "nmp.nip29.share_event_in_group",
    "nmp.nip29.repost_in_group",
];

/// Foreign-NIP / per-kind constant identifiers that must never be (re-)declared
/// here. `REACTION_KIND` / `DELETE_KIND` / `REPOST_KIND` name kind:7 / kind:5 /
/// kind:16 (owned by nip25 / nip18). `KIND_CHAT_MESSAGE` /
/// `KIND_DISCUSSION_OR_ARTIFACT` are the kind:9 / kind:11 constants the
/// kind-blind simplification deleted: NIP-29 must not single out "chat" or
/// "thread" kinds — every routed kind is just a `GroupEvent`.
const BANNED_KIND_CONSTS: &[&str] = &[
    "REACTION_KIND",
    "DELETE_KIND",
    "REPOST_KIND",
    "KIND_CHAT_MESSAGE",
    "KIND_DISCUSSION_OR_ARTIFACT",
];

/// Per-kind action type names that must never reappear.
const BANNED_ACTION_TYPES: &[&str] = &[
    "ReactInGroupAction",
    "UnreactInGroupAction",
    "ShareEventInGroupAction",
    "RepostInGroupAction",
];

/// Per-kind group-event *classification* types the kind-blind simplification
/// removed. A routed group event has exactly one class (`KindClass::GroupEvent`);
/// NIP-29 must not re-introduce a known-vs-unknown / chat-vs-thread split.
const BANNED_CLASSIFICATION_TYPES: &[&str] =
    &["KnownGroupEvent", "UnknownGroupEvent", "GroupEventClass"];

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Is this a test-only source file (a `*tests*.rs` sibling)? Such files name the
/// banned tokens in fixtures/assertions and are excluded; we scan production
/// source only.
fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|f| f.to_str())
        .is_some_and(|f| f.contains("test"))
}

#[test]
fn nip29_ships_no_per_kind_action_foreign_kind_or_classification() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_sources(&crate_root.join("src"), &mut files);
    assert!(!files.is_empty(), "expected to find nip29 source files");

    let mut violations: Vec<String> = Vec::new();

    for path in &files {
        if is_test_file(path) {
            continue;
        }
        let contents = std::fs::read_to_string(path).expect("read source");
        // Production code only: drop trailing `#[cfg(test)] mod tests { … }`.
        let production = match contents.find("#[cfg(test)]") {
            Some(idx) => &contents[..idx],
            None => &contents[..],
        };
        let rel = path.strip_prefix(&crate_root).unwrap_or(path).display();

        for ns in BANNED_NAMESPACES {
            if production.contains(ns) {
                violations.push(format!("{rel}: banned per-kind namespace `{ns}`"));
            }
        }
        for ident in BANNED_KIND_CONSTS {
            if production.contains(ident) {
                violations.push(format!(
                    "{rel}: per-kind / foreign-NIP kind constant `{ident}` — kind:7/5/16 belong to nip25/nip18; chat/thread kinds must not be singled out"
                ));
            }
        }
        for ty in BANNED_ACTION_TYPES {
            if production.contains(ty) {
                violations.push(format!("{rel}: banned per-kind action type `{ty}`"));
            }
        }
        for ty in BANNED_CLASSIFICATION_TYPES {
            if production.contains(ty) {
                violations.push(format!(
                    "{rel}: banned per-kind group-event classification `{ty}` — every routed kind is one `KindClass::GroupEvent`"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "nmp-nip29 must stay kind-blind transport (#2513); found:\n{}",
        violations.join("\n")
    );
}
