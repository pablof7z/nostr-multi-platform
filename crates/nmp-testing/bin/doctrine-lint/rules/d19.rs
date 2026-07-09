//! D19 — display formatting banned from kernel projection/error producers.
//!
//! ADR-0072 (raw-data projection doctrine, V-115): projection builders in
//! `kernel/update/`, `kernel/types.rs`, and `kernel/publish_outbox.rs` must
//! send raw protocol data to shells. Display-formatting helpers —
//! `crate::display::` and `format_timestamp` — are banned in those files.
//! Kernel/core error producers must also emit `UiToken`s, not direct
//! English-only `set_last_error_toast(Some(...))` calls.
//!
//! ## What this catches
//!
//! - `crate::display::` — the display-formatting module inside `nmp-core`.
//!   Its entry points encode bech32 npubs, abbreviate hex, format timestamps,
//!   etc. Calling them in projection code bakes locale-specific English into
//!   the wire format, violating ADR-0072.
//! - `format_timestamp(` — the same violation via a direct call to the
//!   `format_timestamp` helper (which lives in `kernel/nostr.rs` and
//!   historically leaked into `publish_outbox.rs`).
//! - `set_last_error_toast(Some(` in core producer files — the legacy
//!   English-only toast path has no stable machine code for shells to localize.
//!
//! ## Scope
//!
//! Fires in:
//! - `crates/nmp-core/src/kernel/update/` — the snapshot-projection builders
//!   (`projections.rs`, `views.rs`).
//! - `crates/nmp-core/src/kernel/types.rs` — the `ProfileCard` /
//!   `PublishOutboxItem` DTO definitions.
//! - `crates/nmp-core/src/kernel/publish_outbox.rs` — the outbox projection
//!   builder.
//! - Core error producer files under `actor/commands/`,
//!   `actor/dispatch/cmd_publish.rs`, `actor/loop_context.rs`, and
//!   `kernel/publish_*.rs`. Boundary forwarding paths (`ShowToast`,
//!   protocol adapters, capability trait defaults) are intentionally out of
//!   scope until their owning protocol crates define token codes.
//!
//! ## Exemptions
//!
//! - Doc-comment lines (`//`, `///`, `//!`, inside `/* */`) — skipped via the
//!   `is_comment` flag passed by the walker.
//! - Test-only files (`*_tests.rs`, `tests.rs`, …) — handled via
//!   `d6::file_is_test_only` in the `main.rs` driver block.
//! - `#[cfg(test)]` module bodies — the caller's `in_test_cfg` flag.
//! - Per-line `// doctrine-allow: D19 — reason` opt-out (standard mechanism).
//! - The doctrine-lint binary's own source tree (its string constants contain
//!   the banned tokens — meta-false-positives on broad sweeps).
//!
//! ## Per-line opt-out
//!
//! `// doctrine-allow: D19 — reason` on the offending line suppresses the
//! finding.

use std::path::Path;

pub const ID: &str = "D19";

/// Banned tokens in projection/error producer files. Each entry is
/// `(token, message, suggested)`.
const BANNED: &[(&str, &str, &str)] = &[
    (
        "crate::display::",
        "`crate::display::*` called in a kernel projection builder violates \
         ADR-0072 (V-115): projections must send raw data; shells format for display",
        "send raw `pubkey: String` (hex) and `created_at: u64` (Unix secs); \
         shell converts to bech32 / locale-formatted time on the host side",
    ),
    (
        "format_timestamp(",
        "`format_timestamp` called in a kernel projection builder violates \
         ADR-0072 (V-115): send raw Unix-seconds `u64`; shells format with their \
         own locale/TZ",
        "send raw `pubkey: String` (hex) and `created_at: u64` (Unix secs); \
         shell converts to bech32 / locale-formatted time on the host side",
    ),
    (
        "set_last_error_toast(Some(",
        "`set_last_error_toast(Some(...))` emits English-only error prose with \
         no stable UiToken code; kernel/core producers must use \
         `set_last_error_token(&UiToken::error(...))`",
        "emit a UiToken with a stable code, fallback prose, and raw detail via \
         `with_detail(...)` when the message is derived from an upstream error",
    ),
];

/// True iff the file is a kernel projection builder that D19 guards.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Never fire in the doctrine-lint binary itself (meta-false-positives).
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }

    // Projection-builder paths within nmp-core.
    let is_projection_file = s.contains("/crates/nmp-core/src/kernel/update/")
        || s.contains("crates/nmp-core/src/kernel/update/")
        || s.ends_with("/crates/nmp-core/src/kernel/types.rs")
        || s.contains("crates/nmp-core/src/kernel/types.rs")
        || s.ends_with("/crates/nmp-core/src/kernel/publish_outbox.rs")
        || s.contains("crates/nmp-core/src/kernel/publish_outbox.rs");

    let is_error_producer_file = s.contains("/crates/nmp-core/src/actor/commands/")
        || s.contains("crates/nmp-core/src/actor/commands/")
        || s.ends_with("/crates/nmp-core/src/actor/dispatch/cmd_publish.rs")
        || s.contains("crates/nmp-core/src/actor/dispatch/cmd_publish.rs")
        || s.ends_with("/crates/nmp-core/src/actor/loop_context.rs")
        || s.contains("crates/nmp-core/src/actor/loop_context.rs")
        || s.contains("/crates/nmp-core/src/kernel/publish_")
        || s.contains("crates/nmp-core/src/kernel/publish_");

    // Gallery app crate's UniFFI snapshot JSON adapter (#3098): the #3095
    // scanner fix (#3104) widened doctrine-lint's walk to `apps/*`, but this
    // allowlist still excluded `apps/*` entirely, so `snapshot_json.rs`
    // baking display fields into the UniFFI wire went uncaught. Any file
    // under an `apps/nmp-gallery/crates/*/src/` tree is a projection/wire
    // adapter in the same sense as the nmp-core paths above.
    let is_gallery_app_crate_file =
        s.contains("apps/nmp-gallery/crates/") && s.contains("/src/");

    is_projection_file || is_error_producer_file || is_gallery_app_crate_file
}

/// Returns `(col, message, suggested)` for each banned display token on `line`.
/// `is_comment` and `in_test_cfg` suppress the scan.
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (token, message, suggested) in BANNED {
        let mut start = 0;
        while let Some(rel) = line[start..].find(token) {
            let col = start + rel + 1; // 1-indexed
            hits.push((col, message.to_string(), suggested.to_string()));
            start += rel + token.len();
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- check() unit tests ---------------------------------------------------

    #[test]
    fn flags_crate_display_in_prod() {
        let hits = check("    let npub = crate::display::to_npub(pk);", false, false);
        assert_eq!(hits.len(), 1, "must flag crate::display:: in prod code");
        assert!(
            hits[0].1.contains("ADR-0072"),
            "message must reference ADR-0072; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn flags_format_timestamp_in_prod() {
        let hits = check(
            "    let ts = format_timestamp(row.created_at);",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "must flag format_timestamp in prod code");
        assert!(
            hits[0].1.contains("ADR-0072"),
            "message must reference ADR-0072"
        );
    }

    #[test]
    fn does_not_flag_comment_lines() {
        let hits = check("// crate::display:: is banned here", true, false);
        assert!(hits.is_empty(), "comment lines must not be flagged");
    }

    #[test]
    fn does_not_flag_in_test_cfg() {
        let hits = check("    let npub = crate::display::to_npub(pk);", false, true);
        assert!(
            hits.is_empty(),
            "#[cfg(test)] bodies must not be flagged by D19"
        );
    }

    #[test]
    fn col_is_1_indexed() {
        let line = "let x = crate::display::to_npub(pk);";
        let hits = check(line, false, false);
        assert_eq!(hits.len(), 1);
        // "crate::display::" starts at byte offset 8 (0-indexed).
        assert_eq!(hits[0].0, 9, "column must be 1-indexed");
    }

    #[test]
    fn flags_two_occurrences_same_line() {
        let hits = check(
            "a(crate::display::to_npub(x), crate::display::short(y))",
            false,
            false,
        );
        assert_eq!(
            hits.len(),
            2,
            "both occurrences on same line must be flagged"
        );
    }

    #[test]
    fn flags_legacy_error_toast_in_prod() {
        let hits = check(
            "    kernel.set_last_error_toast(Some(\"boom\".to_string()));",
            false,
            false,
        );
        assert_eq!(hits.len(), 1, "must flag English-only error toasts");
        assert!(
            hits[0].1.contains("UiToken"),
            "message must point to UiToken; got: {}",
            hits[0].1
        );
    }

    // -- file_in_scope unit tests ---------------------------------------------

    #[test]
    fn projection_files_are_in_scope() {
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/update/projections.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/update/views.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/types.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/publish_outbox.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/actor/commands/identity/account_ops.rs"
        )));
        assert!(file_in_scope(Path::new(
            "crates/nmp-core/src/actor/dispatch/cmd_publish.rs"
        )));
        // Absolute path variant.
        assert!(file_in_scope(Path::new(
            "/abs/path/crates/nmp-core/src/kernel/update/projections.rs"
        )));
    }

    #[test]
    fn non_projection_files_are_out_of_scope() {
        // nostr.rs (where format_timestamp lives) is NOT a projection builder.
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/nostr.rs"
        )));
        // display module itself is out of scope.
        assert!(!file_in_scope(Path::new("crates/nmp-core/src/display.rs")));
        // Other kernel files.
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/kernel/mod.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/actor/dispatch/mod.rs"
        )));
        assert!(!file_in_scope(Path::new(
            "crates/nmp-core/src/substrate/protocol/capabilities.rs"
        )));
        // Protocol crates.
        assert!(!file_in_scope(Path::new("crates/nmp-nip17/src/lib.rs")));
    }

    #[test]
    fn doctrine_lint_source_is_out_of_scope() {
        assert!(!file_in_scope(Path::new(
            "crates/nmp-testing/bin/doctrine-lint/rules/d19.rs"
        )));
    }

    /// #3098 — the gallery app crate's UniFFI snapshot adapter must be in
    /// scope so a re-introduced `crate::display::`/`format_timestamp(` bake
    /// into that wire red-fails CI going forward.
    #[test]
    fn gallery_app_crate_is_in_scope() {
        assert!(file_in_scope(Path::new(
            "apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
        )));
        assert!(file_in_scope(Path::new(
            "/abs/path/apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
        )));
    }
}
