//! D23 — single accepted-event store-insert chokepoint (event-flow PR1 lock).
//!
//! The event-flow spine landed a SINGLE accepted-event persistence path: the
//! kernel's `verify_and_persist` (`crates/nmp-core/src/kernel/ingest/persistence.rs`)
//! owns the `sig-verify → store.insert → raw-tap → provenance → TTL` sequence
//! (ADR-0070). Every event that enters the store does so
//! through that one door. A NEW ingest ladder — a second function that writes
//! to the `EventStore` directly — would re-fragment the spine: it would bypass
//! provenance accounting, the raw-tap, TTL stamping, and the dispatcher, and
//! reintroduce exactly the dual-ladder the unification deleted.
//!
//! D23 makes that permanent. It bans an `EventStore::insert` call anywhere in
//! `nmp-core/src/` EXCEPT the chokepoint file. The store *implementations*
//! (`crates/nmp-store/`) and the in-memory/LMDB backends naturally fall outside
//! the `nmp-core` scope, and tests are exempt by the standard mechanism.
//!
//! ## What this bans (two shapes — the matcher is newline-tolerant)
//!
//! The production chokepoint writes the call rustfmt-SPLIT across lines —
//! `match self` / `.store` / `.insert(...)` (kernel/ingest/persistence.rs). A
//! second store-write would be written the same way, so a single-line
//! contiguous match would leave an evasion hole. D23 catches BOTH:
//!
//! 1. **Contiguous** — `store.insert(` on one line.
//! 2. **Split chain** — a code line whose trimmed tail is the `store` token
//!    (`.store` or a bareword `store`) followed by a subsequent code line whose
//!    trimmed head is `.insert(`. This is the rustfmt method-chain shape and the
//!    exact pattern the chokepoint itself uses, so the gate catches the very
//!    shape it bans.
//!
//! Both forms are **left-boundary anchored** on the `store` token: the
//! character before `store` must not be an identifier character, so
//! `keystore.insert(`, `restore.insert(`, `datastore.insert(`, and
//! `event_store.insert(` (a longer identifier ENDING in `store`) do NOT
//! false-positive — only the canonical field/binding access fires. This is the
//! `unregister_raw_event_observer` substring-false-positive class prior reviews
//! flagged.
//!
//! ## Scope (`file_in_scope`)
//!
//! `crates/nmp-core/src/` only — the crate that hosts the kernel ingest spine.
//! The store implementations live in `crates/nmp-store/` (a different crate,
//! naturally out of scope). The chokepoint file `kernel/ingest/persistence.rs` is
//! excluded from scope (allowlisted).
//!
//! ## State (split-chain detection)
//!
//! Split-chain detection needs to remember whether the previous CODE line ended
//! with the `store` token, so D23 carries a [`State`]. Comment lines leave the
//! tracker unchanged (rustfmt does not interleave comments into a chain, and
//! linking across one is the stricter choice); `#[cfg(test)]` lines reset it.
//!
//! ## Exemptions
//!
//! - The chokepoint file `crates/nmp-core/src/kernel/ingest/persistence.rs`.
//! - Doc/line comments (`is_comment`) — skipped (do not fire, do not advance).
//! - `#[cfg(test)]` bodies (`in_test_cfg`) and test-only files (`d6_test_file`,
//!   handled in the driver) — fixtures freely seed the store.
//! - Per-line `// doctrine-allow: D23 — reason` opt-out, REASON-REQUIRED (the
//!   D10/D21 tightened idiom; a bare `// doctrine-allow: D23` does not silence).
//! - The doctrine-lint binary's own source tree (string constants contain the
//!   banned token — meta-false-positives on broad sweeps).
//!
//! ## Heuristic scope (regression backstop, NOT a formal proof)
//!
//! D23 is a formatting-heuristic regression BACKSTOP, not a soundness proof. It
//! catches the normal, rustfmt-split (`.store` / `.insert(`), and
//! trailing-comment (`.store // …` / `.insert(`) forms of a store insert. A
//! deliberately-obfuscated write — built through a macro, aliased through a
//! re-export, or assigned to an intermediate binding — is OUT OF SCOPE and is a
//! code-review concern, not something a line-based lint chases.

use std::path::Path;

pub const ID: &str = "D23";

/// The chokepoint file: the single accepted-event persistence path
/// (`verify_and_persist`). The only legal `store.insert` site in `nmp-core`.
const CHOKEPOINT_FILE: &str = "crates/nmp-core/src/kernel/ingest/persistence.rs";

/// Cross-line tracker for split-chain detection: did the previous CODE line end
/// with the `store` token?
#[derive(Default)]
pub struct State {
    prev_line_ends_with_store: bool,
}

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

/// True iff `line`'s code tail is the `store` token — i.e. after stripping any
/// trailing `//` line comment and whitespace it ends with `store`, and the char
/// before that `store` is not an identifier char (so `.store` / bareword
/// `store` match, but `keystore` / `event_store` do not). Stripping the comment
/// first closes the `.store // foo`\n`.insert(` trailing-comment evasion.
fn line_ends_with_store_token(line: &str) -> bool {
    let code = match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    };
    let t = code.trim_end();
    if !t.ends_with("store") {
        return false;
    }
    let idx = t.len() - "store".len();
    !preceded_by_ident_char(t.as_bytes(), idx)
}

fn message() -> String {
    "store insert outside the accepted-event chokepoint violates D23 \
     (event-flow PR1 lock). The kernel's `verify_and_persist` \
     (kernel/ingest/persistence.rs) is the SINGLE accepted-event persistence path \
     (sig-verify → store.insert → raw-tap → provenance → TTL, \
     ADR-0070). A second store-insert site is a new ingest ladder that bypasses \
     provenance accounting, the raw-tap, TTL stamping, and the dispatcher"
        .to_string()
}

fn suggested() -> String {
    "route the event through the unified ingest chokepoint \
     (`Kernel::verify_and_persist`) instead of inserting into the store \
     directly; the store implementations live in `crates/nmp-store/`"
        .to_string()
}

/// Returns `(col, message, suggested)` for each banned store-insert on `line`,
/// catching both the contiguous and the rustfmt-split-chain shapes. `state`
/// carries the cross-line tracker; `is_comment` and `in_test_cfg` suppress.
pub fn check(
    state: &mut State,
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<(usize, String, String)> {
    // `#[cfg(test)]` bodies are exempt; reset the chain tracker (a test block
    // breaks any straddling chain) and emit nothing.
    if in_test_cfg {
        state.prev_line_ends_with_store = false;
        return Vec::new();
    }
    // Comment lines neither fire nor advance the tracker (the previous code
    // line's tail still links to the next code line).
    if is_comment {
        return Vec::new();
    }

    let bytes = line.as_bytes();
    let mut hits = Vec::new();

    // (1) Contiguous `store.insert(` (left-boundary anchored).
    let needle = "store.insert(";
    let mut start = 0;
    while let Some(rel) = line[start..].find(needle) {
        let abs = start + rel;
        start = abs + needle.len();
        if preceded_by_ident_char(bytes, abs) {
            continue;
        }
        hits.push((abs + 1, message(), suggested()));
    }

    // (2) Split chain: previous code line ended with the `store` token and this
    // line's trimmed head is `.insert(`.
    let trimmed = line.trim_start();
    if state.prev_line_ends_with_store && trimmed.starts_with(".insert(") {
        let col = line.len() - trimmed.len() + 1; // 1-indexed at the `.insert(`.
        hits.push((col, message(), suggested()));
    }

    // Advance the tracker for the next line.
    state.prev_line_ends_with_store = line_ends_with_store_token(line);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> usize {
        let mut state = State::default();
        let mut n = 0;
        for l in lines {
            n += check(&mut state, l, false, false).len();
        }
        n
    }

    #[test]
    fn flags_self_store_insert_contiguous() {
        let mut s = State::default();
        let hits = check(
            &mut s,
            "        self.store.insert(verified, &p, ts);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "contiguous self.store.insert( must fire");
        assert!(hits[0].1.contains("D23"));
        assert!(hits[0].1.contains("verify_and_persist"));
    }

    #[test]
    fn flags_bareword_store_insert_contiguous() {
        let mut s = State::default();
        let hits = check(&mut s, "    store.insert(ev, &url, ms);", false, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 5, "column 1-indexed at the `store` token");
    }

    #[test]
    fn flags_rustfmt_split_chain() {
        // The chokepoint's exact shape — `.store` then `.insert(` on the next
        // line. This is the evasion hole a single-line matcher leaves open.
        let n = run(&[
            "        match self",
            "            .store",
            "            .insert(verified, &provenance, self.ingest_received_at_ms())",
        ]);
        assert_eq!(n, 1, "split .store / .insert( chain must fire exactly once");
    }

    #[test]
    fn flags_two_line_split_chain() {
        let n = run(&["        self.store", "            .insert(v, &r, 0);"]);
        assert_eq!(n, 1, "two-line self.store / .insert( split must fire");
    }

    #[test]
    fn flags_split_chain_with_trailing_comment_on_store_line() {
        // `.store // comment` then `.insert(` — the trailing-comment evasion.
        // The comment must be stripped before the suffix check.
        let n = run(&[
            "        self.store // fetch the event store",
            "            .insert(v, &r, 0);",
        ]);
        assert_eq!(
            n, 1,
            "trailing comment on the .store line must not evade D23"
        );
    }

    #[test]
    fn split_reports_on_insert_line() {
        let mut s = State::default();
        assert!(check(&mut s, "    .store", false, false).is_empty());
        let hits = check(&mut s, "        .insert(v);", false, false);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 9, "column 1-indexed at the `.insert(`");
    }

    #[test]
    fn does_not_flag_keystore_split() {
        // `.keystore` then `.insert(` — `store` is the tail of a longer
        // identifier; the left-boundary rule excludes it.
        let n = run(&["    self.keystore", "        .insert(k, v);"]);
        assert_eq!(n, 0, "keystore split must NOT fire");
    }

    #[test]
    fn does_not_flag_event_store_split() {
        let n = run(&["    self.event_store", "        .insert(e);"]);
        assert_eq!(n, 0, "event_store split must NOT fire");
    }

    #[test]
    fn does_not_flag_store_followed_by_other_method() {
        // `.store` then a NON-insert method must not fire (only `.insert(`).
        let n = run(&["    self.store", "        .len();"]);
        assert_eq!(n, 0, ".store followed by .len() must NOT fire");
    }

    #[test]
    fn blank_line_breaks_the_chain() {
        let n = run(&["    self.store", "", "        .insert(v);"]);
        assert_eq!(
            n, 0,
            "a blank line between .store and .insert( breaks the chain"
        );
    }

    #[test]
    fn does_not_flag_keystore_contiguous() {
        let mut s = State::default();
        assert!(check(&mut s, "    keystore.insert(k, v);", false, false).is_empty());
    }

    #[test]
    fn does_not_flag_hashmap_insert() {
        let mut s = State::default();
        assert!(check(&mut s, "    map.insert(key, value);", false, false).is_empty());
    }

    #[test]
    fn does_not_flag_comment() {
        let mut s = State::default();
        assert!(check(
            &mut s,
            "// self.store.insert(...) is the chokepoint",
            true,
            false
        )
        .is_empty());
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let mut s = State::default();
        assert!(check(&mut s, "self.store.insert(v, &r, 0);", false, true).is_empty());
    }

    #[test]
    fn chokepoint_file_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/ingest/persistence.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "/abs/crates/nmp-core/src/kernel/ingest/persistence.rs"
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
