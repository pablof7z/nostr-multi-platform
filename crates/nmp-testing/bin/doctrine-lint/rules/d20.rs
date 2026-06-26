//! D20 — no raw `std::time::Instant` / `std::time::SystemTime` on the
//! wasm-compiled path.
//!
//! On `wasm32-unknown-unknown`, `std::time::Instant::now()` and
//! `std::time::SystemTime::now()` **panic at runtime** — the platform has no OS
//! clock. PR #1150 introduced the `crate::time` shim (re-exporting
//! `web_time::{Instant, SystemTime, UNIX_EPOCH}` on wasm32 and `std::time`
//! verbatim on native) and swapped every wasm-reachable kernel import site to
//! use it. D20 makes that discipline automatic: any wasm-reachable crate that
//! imports `Instant` / `SystemTime` directly from `std::time`, or calls
//! `std::time::Instant::now()` / `std::time::SystemTime::now()` inline, is a
//! latent wasm panic.
//!
//! ## What this catches
//!
//! - **Imports** — a line that contains `use std::time::` AND `Instant` or
//!   `SystemTime`. This deliberately matches grouped imports like
//!   `use std::time::{Duration, Instant};` that a single-needle `std::time::Instant`
//!   check would miss (the grouped form is the common real-world shape).
//! - **Inline call sites** — `std::time::Instant::now()` and
//!   `std::time::SystemTime::now()` written fully-qualified, without a `use`.
//!
//! `Duration` is exempt: `std::time::Duration == web_time::Duration` is the
//! same type on both targets, so importing it directly from `std::time` is
//! safe.
//!
//! ## Scope (`file_in_scope`)
//!
//! Only wasm-reachable crates are scanned:
//! `nmp-core`, `nmp-store`, `nmp-network`, `nmp-signers`, `nmp-wasm`,
//! `nmp-browser-runtime`, `nmp-planner`, `nmp-chirp-config`,
//! `nmp-signer-iface` (#1161 added the last three — they pull into the wasm
//! dependency graph transitively; #2082 added the browser runtime crate).
//!
//! Within those crates, three subtrees are excluded because they never compile
//! to `wasm32` (the actor *runtime*, the relay-worker I/O loop, and the LMDB
//! backend are all `native`-gated), so a bare `Instant::now()` there is fine:
//! - `actor/**`
//! - `relay_worker/**`
//! - `nmp-store/src/lmdb/**`
//!
//! The two time shims themselves are exempt (they MUST import `std::time`):
//! - `crates/nmp-core/src/time.rs`
//! - `crates/nmp-store/src/time.rs`
//!
//! ## Exemptions
//!
//! - Doc/line comments (`is_comment`) — skipped.
//! - `#[cfg(test)]` module bodies (`in_test_cfg`) and test-only files
//!   (`d6::file_is_test_only`, handled in the `main.rs` driver) — test builds
//!   never run on wasm32, so a bare `Instant::now()` in a test is fine.
//! - Per-line `// doctrine-allow: D20 — reason` opt-out (standard mechanism).
//!   Used for native-only signer sites not yet wasm-reachable (nip55/nip46)
//!   and for type-only imports that never call `.now()` in production.
//! - The doctrine-lint binary's own source tree (its string constants contain
//!   the banned tokens — meta-false-positives on broad sweeps).

use std::path::Path;

pub const ID: &str = "D20";

/// Wasm-reachable crates D20 guards. A file is in scope iff its path contains
/// `crates/<name>/src/` for one of these names.
const WASM_REACHABLE_CRATES: &[&str] = &[
    "nmp-core",
    "nmp-store",
    "nmp-network",
    "nmp-signers",
    "nmp-wasm",
    "nmp-browser-runtime",
    "nmp-planner",
    "nmp-chirp-config",
    "nmp-signer-iface",
];

/// True iff D20 should scan `path`: it lives inside a wasm-reachable crate's
/// `src/` tree, is not one of the two time shims, and is not in a native-only
/// subtree (`actor/**`, `relay_worker/**`, `nmp-store/src/lmdb/**`).
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Never fire in the doctrine-lint binary itself (meta-false-positives).
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }

    // The two time shims MUST import std::time — they are the abstraction.
    if s.ends_with("/crates/nmp-core/src/time.rs")
        || s.ends_with("crates/nmp-core/src/time.rs")
        || s.ends_with("/crates/nmp-store/src/time.rs")
        || s.ends_with("crates/nmp-store/src/time.rs")
    {
        return false;
    }

    // Native-only subtrees never compile to wasm32 — bare Instant::now() is fine.
    if s.contains("/actor/") || s.contains("/relay_worker/") {
        return false;
    }
    if s.contains("/nmp-store/src/lmdb/") {
        return false;
    }

    // In scope iff inside a wasm-reachable crate's `src/` tree.
    WASM_REACHABLE_CRATES
        .iter()
        .any(|c| s.contains(&format!("crates/{}/src/", c)))
}

/// Returns `(col, message, suggested)` for each banned `std::time` usage on
/// `line`. `is_comment` and `in_test_cfg` suppress the scan.
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();

    // (1) Import detection: `use std::time::` AND (Instant|SystemTime) on the
    // same line. Catches both single (`use std::time::Instant;`) and grouped
    // (`use std::time::{Duration, Instant};`) imports. Report at the column of
    // `std::time::` so the finding points at the offending path.
    if let Some(use_rel) = line.find("use std::time::") {
        let import_tail = &line[use_rel + "use ".len()..];
        let names_instant = import_tail.contains("Instant");
        let names_systemtime = import_tail.contains("SystemTime");
        if names_instant || names_systemtime {
            let token = if names_instant && names_systemtime {
                "Instant`/`SystemTime"
            } else if names_instant {
                "Instant"
            } else {
                "SystemTime"
            };
            let col = use_rel + "use ".len() + 1; // 1-indexed column of std::time::
            hits.push((
                col,
                format!(
                    "`use std::time::{{… {} …}}` on a wasm-reachable path violates D20: \
                     `std::time::Instant::now()`/`SystemTime::now()` PANIC on wasm32. \
                     Import from `crate::time` (web-time shim) instead",
                    token
                ),
                "import from the wasm-safe shim: `use crate::time::Instant;` / \
                 `use crate::time::{SystemTime, UNIX_EPOCH};` — it re-exports `std::time` \
                 verbatim on native (zero-cost) and `web_time` on wasm32"
                    .to_string(),
            ));
        }
    }

    // (2) Inline fully-qualified call sites: `std::time::Instant::now()` and
    // `std::time::SystemTime::now()` written without a `use`.
    for (needle, ty) in [
        ("std::time::Instant::now(", "Instant"),
        ("std::time::SystemTime::now(", "SystemTime"),
    ] {
        let mut start = 0;
        while let Some(rel) = line[start..].find(needle) {
            let abs = start + rel;
            let col = abs + 1; // 1-indexed
            hits.push((
                col,
                format!(
                    "`std::time::{}::now()` on a wasm-reachable path violates D20: \
                     it PANICS on wasm32. Call `crate::time::{}::now()` (web-time shim) instead",
                    ty, ty
                ),
                format!(
                    "use the wasm-safe shim: `crate::time::{}::now()` — re-exports \
                     `std::time` verbatim on native and `web_time` on wasm32",
                    ty
                ),
            ));
            start = abs + needle.len();
        }
    }

    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- check() unit tests ---------------------------------------------------

    #[test]
    fn flags_single_instant_import() {
        let hits = check("use std::time::Instant;", false, false);
        assert_eq!(hits.len(), 1, "must flag a single Instant import");
        assert!(hits[0].1.contains("D20"));
        assert!(hits[0].1.contains("crate::time"));
    }

    #[test]
    fn flags_grouped_import_with_instant() {
        // The headline case: a single-needle `std::time::Instant` check misses
        // this; the `use std::time::` + `Instant` two-token match catches it.
        let hits = check("use std::time::{Duration, Instant};", false, false);
        assert_eq!(hits.len(), 1, "grouped import naming Instant must fire");
    }

    #[test]
    fn flags_grouped_import_with_systemtime() {
        let hits = check("    use std::time::{SystemTime, UNIX_EPOCH};", false, false);
        assert_eq!(hits.len(), 1, "grouped import naming SystemTime must fire");
        assert!(hits[0].1.contains("SystemTime"));
    }

    #[test]
    fn does_not_flag_duration_only_import() {
        // Duration is the same type on both targets — safe from std::time.
        let hits = check("use std::time::Duration;", false, false);
        assert!(hits.is_empty(), "Duration-only import must NOT fire");
    }

    #[test]
    fn flags_inline_instant_now() {
        let hits = check("        let t0 = std::time::Instant::now();", false, false);
        assert_eq!(hits.len(), 1, "inline Instant::now() must fire");
        assert!(hits[0].1.contains("PANICS on wasm32"));
    }

    #[test]
    fn flags_inline_systemtime_now() {
        let hits = check("    let n = std::time::SystemTime::now();", false, false);
        assert_eq!(hits.len(), 1, "inline SystemTime::now() must fire");
    }

    #[test]
    fn does_not_flag_comment_lines() {
        let hits = check("// std::time::Instant::now() is banned here", true, false);
        assert!(hits.is_empty(), "comment lines must not be flagged");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check("use std::time::Instant;", false, true);
        assert!(
            hits.is_empty(),
            "#[cfg(test)] bodies must not be flagged by D20 (tests never run on wasm32)"
        );
    }

    #[test]
    fn col_is_1_indexed_at_std_time_path() {
        // The finding points at the offending `std::time::` path (after the
        // `use ` keyword), 1-indexed. For `"use std::time::Instant;"`, `std`
        // is byte offset 4 → column 5.
        let hits = check("use std::time::Instant;", false, false);
        assert_eq!(
            hits[0].0, 5,
            "column must be 1-indexed at the std::time:: path"
        );
    }

    // -- file_in_scope unit tests ---------------------------------------------

    #[test]
    fn wasm_reachable_crates_are_in_scope() {
        for c in WASM_REACHABLE_CRATES {
            let p = format!("crates/{}/src/lib.rs", c);
            assert!(file_in_scope(Path::new(&p)), "{} src must be in scope", c);
        }
        // Absolute path variant.
        assert!(file_in_scope(Path::new(
            "/abs/crates/nmp-network/src/keepalive.rs"
        )));
    }

    #[test]
    fn time_shims_are_out_of_scope() {
        assert!(!file_in_scope(Path::new("crates/nmp-core/src/time.rs")));
        assert!(!file_in_scope(Path::new("crates/nmp-store/src/time.rs")));
        assert!(!file_in_scope(Path::new(
            "/abs/crates/nmp-core/src/time.rs"
        )));
    }

    #[test]
    fn native_only_subtrees_are_out_of_scope() {
        // actor/** — the actor runtime is native-gated.
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/actor/commands/identity.rs"
        )));
        // relay_worker/** — the relay I/O loop is native-only.
        assert!(!file_in_scope(Path::new(
            "crates/nmp-network/src/relay_worker/mod.rs"
        )));
        // nmp-store/src/lmdb/** — the LMDB backend is native-only.
        assert!(!file_in_scope(Path::new("crates/nmp-store/src/lmdb/gc.rs")));
    }

    #[test]
    fn non_wasm_reachable_crate_is_out_of_scope() {
        // nmp-marmot, nmp-ffi, apps/chirp etc. are not in the wasm-reachable
        // list, so D20 does not scan them.
        assert!(!file_in_scope(Path::new("crates/nmp-marmot/src/lib.rs")));
        assert!(!file_in_scope(Path::new("crates/nmp-ffi/src/lib.rs")));
        assert!(!file_in_scope(Path::new(
            "apps/chirp/crates/nmp-app-chirp/src/lib.rs"
        )));
    }

    #[test]
    fn doctrine_lint_source_is_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d20.rs"
        )));
    }
}
