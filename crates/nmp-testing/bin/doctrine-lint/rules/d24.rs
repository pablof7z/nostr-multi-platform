//! D24 — single post-store observer fan-out seam (event-flow lock).
//!
//! After an event is accepted and persisted, the kernel notifies app-facing
//! event observers from ONE place: the shared `project_accepted_event`
//! (`crates/nmp-core/src/kernel/ingest/mod.rs`), which calls
//! `notify_event_observers` exactly once per accepted event. The cache-serve
//! replay seam (`crates/nmp-core/src/kernel/cache_serve/`) reaches observers
//! through that same `project_accepted_event` door, so store-first and
//! network-first events fan out identically (ADR-0045 single-mechanism
//! cache-serve).
//!
//! A NEW scattered `notify_event_observers` call — a feature path that decides
//! on its own to re-notify observers — would re-fragment the fan-out: observers
//! would fire from two unsynchronized sites, breaking the "every accepted event
//! reaches the host exactly once, through the post-store seam" invariant. D24
//! makes that permanent: only the fan-out seam, its definition, and the
//! cache-serve replay module may name the call.
//!
//! ## What this bans
//!
//! A `notify_event_observers(` token outside the allowlisted seam files. The
//! match is **left-boundary anchored**: the character before
//! `notify_event_observers` must not be an identifier character, so a longer
//! identifier ending in that name (e.g. a hypothetical
//! `force_notify_event_observers(`) does NOT false-positive. (The `un`-prefixed
//! `unregister_raw_event_observer` substring class the prior reviews flagged is
//! a different token and never matches this needle.)
//!
//! ## Scope (`file_in_scope`)
//!
//! `crates/nmp-core/src/` only — `notify_event_observers` is `pub(in
//! crate::kernel)`, so only kernel code can call it. The allowlisted seam
//! files (excluded from scope) are:
//! - `kernel/ingest/mod.rs` — the `project_accepted_event` post-store fan-out,
//! - `kernel/event_observer.rs` — the `notify_event_observers` definition,
//! - `kernel/cache_serve/**` — the single-mechanism cache-serve replay seam.
//!
//! ## Exemptions
//!
//! - The three allowlisted seam paths above.
//! - Doc/line comments (`is_comment`) — skipped.
//! - `#[cfg(test)]` bodies (`in_test_cfg`) and test-only files (`d6_test_file`,
//!   handled in the driver) — e.g. `test_support.rs` fires the seam directly to
//!   stage observer state.
//! - Per-line `// doctrine-allow: D24 — reason` opt-out (standard mechanism).
//! - The doctrine-lint binary's own source tree (string constants contain the
//!   banned token — meta-false-positives on broad sweeps).

use std::path::Path;

pub const ID: &str = "D24";

/// Seam files where `notify_event_observers` is legal: the post-store fan-out
/// (`project_accepted_event`), the call's own definition, and the cache-serve
/// replay seam. Matched as path suffixes / fragments.
const SEAM_FILES: &[&str] = &[
    "crates/nmp-core/src/kernel/ingest/mod.rs",
    "crates/nmp-core/src/kernel/event_observer.rs",
];

/// Path fragment for the cache-serve replay seam directory (every file under
/// it reaches observers through `project_accepted_event` and may name the seam).
const CACHE_SERVE_DIR: &str = "crates/nmp-core/src/kernel/cache_serve/";

/// True iff D24 should scan `path`: it lives inside `crates/nmp-core/src/`, is
/// not one of the allowlisted seam files / cache-serve dir, and is not the
/// doctrine-lint binary itself.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    if SEAM_FILES.iter().any(|f| s.ends_with(f) || s == *f) {
        return false;
    }
    if s.contains(CACHE_SERVE_DIR) {
        return false;
    }
    s.contains("crates/nmp-core/src/")
}

/// True iff the byte at `idx-1` in `bytes` is an identifier char (`[A-Za-z0-9_]`).
fn preceded_by_ident_char(bytes: &[u8], idx: usize) -> bool {
    idx > 0 && {
        let p = bytes[idx - 1];
        p.is_ascii_alphanumeric() || p == b'_'
    }
}

/// Returns `(col, message, suggested)` for each banned `notify_event_observers(`
/// on `line`. `is_comment` and `in_test_cfg` suppress the scan.
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let needle = "notify_event_observers(";
    let bytes = line.as_bytes();
    let mut hits = Vec::new();
    let mut start = 0;
    while let Some(rel) = line[start..].find(needle) {
        let abs = start + rel;
        start = abs + needle.len();
        // Left-boundary: skip when the token is the tail of a longer identifier.
        if preceded_by_ident_char(bytes, abs) {
            continue;
        }
        let col = abs + 1; // 1-indexed at the `notify_event_observers` token.
        hits.push((
            col,
            "`notify_event_observers(` outside the post-store fan-out seam \
             violates D24 (event-flow lock). The kernel fans out to app-facing \
             observers from ONE place — `project_accepted_event` \
             (kernel/ingest/mod.rs) — and the cache-serve replay seam \
             (kernel/cache_serve/) reaches observers through that same door \
             (ADR-0045 single-mechanism cache-serve). A scattered observer \
             notify fires the host twice and breaks the once-per-accepted-event \
             invariant"
                .to_string(),
            "let the event reach observers through `project_accepted_event` \
             (the single post-store fan-out), or through the cache-serve replay \
             seam — do not call `notify_event_observers` from a feature path"
                .to_string(),
        ));
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_scattered_notify() {
        let hits = check("        self.notify_event_observers(&ev);", false, false);
        assert_eq!(hits.len(), 1, "scattered notify must fire");
        assert!(hits[0].1.contains("D24"));
        assert!(hits[0].1.contains("project_accepted_event"));
    }

    #[test]
    fn flags_bareword_notify() {
        let hits = check("    notify_event_observers(e);", false, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 5, "column 1-indexed at the token");
    }

    #[test]
    fn does_not_flag_longer_identifier() {
        // A longer identifier ending in the token must not false-positive.
        let hits = check("    force_notify_event_observers(e);", false, false);
        assert!(hits.is_empty(), "longer identifier must NOT fire");
    }

    #[test]
    fn does_not_flag_unregister_observer_class() {
        // The substring-false-positive class prior reviews flagged: a different
        // observer token must never match this needle.
        let hits = check("    self.unregister_raw_event_observer(id);", false, false);
        assert!(hits.is_empty(), "unregister_raw_event_observer must NOT fire");
    }

    #[test]
    fn does_not_flag_comment() {
        let hits = check("// calls notify_event_observers(&ev) once", true, false);
        assert!(hits.is_empty(), "comment lines must not fire");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check("self.notify_event_observers(&ev);", false, true);
        assert!(hits.is_empty(), "#[cfg(test)] bodies must not fire");
    }

    #[test]
    fn seam_files_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/ingest/mod.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/event_observer.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/cache_serve/queries.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "/abs/crates/nmp-core/src/kernel/cache_serve/mod.rs"
        )));
    }

    #[test]
    fn other_nmp_core_files_in_scope() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/actor/commands/dm.rs"
        )));
    }

    #[test]
    fn non_nmp_core_out_of_scope() {
        assert!(!file_in_scope(Path::new("crates/nmp-nip17/src/inbox.rs")));
    }

    #[test]
    fn doctrine_lint_source_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d24.rs"
        )));
    }
}
