//! D23 — single accepted-event store-insert chokepoint (event-flow PR1 lock).
//!
//! The event-flow spine landed a SINGLE accepted-event persistence path: the
//! kernel's `verify_and_persist` (`crates/nmp-core/src/kernel/ingest/mod.rs`)
//! owns the `sig-verify → store.insert → NIP-parser dispatch → observer
//! fan-out` sequence (ADR-0057). Every event that enters the store does so
//! through that one door. A NEW ingest ladder — a second function that writes
//! to the `EventStore` directly — would re-fragment the spine: it would bypass
//! provenance accounting, the raw-tap, TTL stamping, and the dispatcher, and
//! reintroduce exactly the dual-ladder the unification deleted.
//!
//! D23 makes that permanent. It bans a `store.insert(` call anywhere in
//! `nmp-core/src/` EXCEPT the chokepoint file. The store *implementations*
//! (`crates/nmp-store/`) and the in-memory/LMDB backends naturally fall outside
//! the `nmp-core` scope, and tests are exempt by the standard mechanism.
//!
//! ## What this bans
//!
//! A `store.insert(` token (the `EventStore::insert` call shape) outside the
//! chokepoint. The match is **left-boundary anchored**: the character before
//! `store` must not be an identifier character, so `keystore.insert(`,
//! `restore.insert(`, `datastore.insert(`, and `event_store.insert(` (a longer
//! identifier ENDING in `store`) do NOT false-positive — only the canonical
//! field/binding access `self.store.insert(` / `kernel.store.insert(` /
//! bareword `store.insert(` fires. This is the `unregister_raw_event_observer`
//! substring-false-positive class the prior reviews flagged.
//!
//! Note the production chokepoint itself writes the call rustfmt-split across
//! lines (`self\n.store\n.insert(...)`), so the contiguous `store.insert(`
//! token does not even appear there; the file allowlist is defensive (a future
//! single-line refactor of the chokepoint stays legal) and keeps the gate
//! green regardless of formatting.
//!
//! ## Scope (`file_in_scope`)
//!
//! `crates/nmp-core/src/` only — the crate that hosts the kernel ingest spine
//! and where a new ladder would be written. The store implementations live in
//! `crates/nmp-store/` (a different crate, naturally out of scope = "the store
//! implementations are the legal `.insert` impl site"). The chokepoint file
//! `kernel/ingest/mod.rs` is excluded from scope (allowlisted).
//!
//! ## Exemptions
//!
//! - The chokepoint file `crates/nmp-core/src/kernel/ingest/mod.rs`.
//! - Doc/line comments (`is_comment`) — skipped.
//! - `#[cfg(test)]` bodies (`in_test_cfg`) and test-only files (`d6_test_file`,
//!   handled in the driver) — fixtures freely seed the store.
//! - Per-line `// doctrine-allow: D23 — reason` opt-out (standard mechanism).
//! - The doctrine-lint binary's own source tree (string constants contain the
//!   banned token — meta-false-positives on broad sweeps).

use std::path::Path;

pub const ID: &str = "D23";

/// The chokepoint file: the single accepted-event persistence path
/// (`verify_and_persist`). The only legal `store.insert` site in `nmp-core`.
const CHOKEPOINT_FILE: &str = "crates/nmp-core/src/kernel/ingest/mod.rs";

/// True iff D23 should scan `path`: it lives inside `crates/nmp-core/src/`, is
/// not the chokepoint file, and is not the doctrine-lint binary itself.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    if s.ends_with(CHOKEPOINT_FILE) || s == CHOKEPOINT_FILE {
        return false;
    }
    s.contains("crates/nmp-core/src/")
}

/// True iff the byte at `idx-1` in `bytes` is an identifier char (`[A-Za-z0-9_]`).
/// Used to reject `store` appearing as the tail of a longer identifier
/// (`keystore`, `restore`, `event_store`).
fn preceded_by_ident_char(bytes: &[u8], idx: usize) -> bool {
    idx > 0 && {
        let p = bytes[idx - 1];
        p.is_ascii_alphanumeric() || p == b'_'
    }
}

/// Returns `(col, message, suggested)` for each banned `store.insert(` on
/// `line`. `is_comment` and `in_test_cfg` suppress the scan.
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let needle = "store.insert(";
    let bytes = line.as_bytes();
    let mut hits = Vec::new();
    let mut start = 0;
    while let Some(rel) = line[start..].find(needle) {
        let abs = start + rel;
        start = abs + needle.len();
        // Left-boundary: skip when `store` is the tail of a longer identifier.
        if preceded_by_ident_char(bytes, abs) {
            continue;
        }
        let col = abs + 1; // 1-indexed at the `store` token.
        hits.push((
            col,
            "`store.insert(` outside the accepted-event chokepoint violates D23 \
             (event-flow PR1 lock). The kernel's `verify_and_persist` \
             (kernel/ingest/mod.rs) is the SINGLE accepted-event persistence \
             path (sig-verify → store.insert → parser dispatch → observer \
             fan-out, ADR-0057). A second store-insert site is a new ingest \
             ladder that bypasses provenance accounting, the raw-tap, TTL \
             stamping, and the dispatcher"
                .to_string(),
            "route the event through the unified ingest chokepoint \
             (`Kernel::verify_and_persist`) instead of inserting into the store \
             directly; the store implementations live in `crates/nmp-store/`"
                .to_string(),
        ));
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_self_store_insert() {
        let hits = check("        self.store.insert(verified, &p, ts);", false, false);
        assert_eq!(hits.len(), 1, "self.store.insert( must fire");
        assert!(hits[0].1.contains("D23"));
        assert!(hits[0].1.contains("verify_and_persist"));
    }

    #[test]
    fn flags_kernel_store_insert() {
        let hits = check("    kernel.store.insert(v, &r, 0);", false, false);
        assert_eq!(hits.len(), 1, "kernel.store.insert( must fire");
    }

    #[test]
    fn flags_bareword_store_insert() {
        let hits = check("    store.insert(ev, &url, ms);", false, false);
        assert_eq!(hits.len(), 1, "bareword store.insert( must fire");
        // column is 1-indexed at the `store` token (5 leading spaces → col 5).
        assert_eq!(hits[0].0, 5);
    }

    #[test]
    fn does_not_flag_keystore_substring() {
        // `keystore.insert(` — `store` is the tail of a longer identifier.
        let hits = check("    keystore.insert(k, v);", false, false);
        assert!(hits.is_empty(), "keystore.insert( must NOT fire");
    }

    #[test]
    fn does_not_flag_restore_substring() {
        let hits = check("    restore.insert(x);", false, false);
        assert!(hits.is_empty(), "restore.insert( must NOT fire");
    }

    #[test]
    fn does_not_flag_underscored_store_identifier() {
        // `event_store.insert(` — `_store` is part of a longer identifier; the
        // left-boundary rule (no preceding ident char) excludes it.
        let hits = check("    event_store.insert(e);", false, false);
        assert!(hits.is_empty(), "event_store.insert( must NOT fire");
    }

    #[test]
    fn does_not_flag_hashmap_insert() {
        let hits = check("    map.insert(key, value);", false, false);
        assert!(hits.is_empty(), "plain map.insert( must NOT fire");
    }

    #[test]
    fn does_not_flag_comment() {
        let hits = check("// self.store.insert(...) is the chokepoint", true, false);
        assert!(hits.is_empty(), "comment lines must not fire");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check("self.store.insert(v, &r, 0);", false, true);
        assert!(hits.is_empty(), "#[cfg(test)] bodies must not fire");
    }

    #[test]
    fn chokepoint_file_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/ingest/mod.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "/abs/crates/nmp-core/src/kernel/ingest/mod.rs"
        )));
    }

    #[test]
    fn other_nmp_core_files_in_scope() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/cache_serve/mod.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/actor/commands/identity.rs"
        )));
    }

    #[test]
    fn store_crate_out_of_scope() {
        // The store implementations (nmp-store) are the legal .insert impl site.
        assert!(!file_in_scope(Path::new("crates/nmp-store/src/mem/mod.rs")));
        assert!(!file_in_scope(Path::new("crates/nmp-store/src/lmdb/gc.rs")));
    }

    #[test]
    fn doctrine_lint_source_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d23.rs"
        )));
    }
}
