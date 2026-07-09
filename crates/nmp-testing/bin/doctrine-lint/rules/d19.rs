//! D19 — presentation-formatting banned from kernel projection/error
//! producers; canonical bech32 codecs are exempt.
//!
//! ADR-0072 (raw-data projection doctrine, V-115): projection builders in
//! `kernel/update/`, `kernel/types.rs`, and `kernel/publish_outbox.rs` must
//! send raw protocol data to shells. Presentation-formatting helpers in
//! `crate::display::` — truncation, initials, avatar tint, relative-time
//! strings — and `format_timestamp` are banned in those files. Kernel/core
//! error producers must also emit `UiToken`s, not direct English-only
//! `set_last_error_toast(Some(...))` calls.
//!
//! ### Codec vs. presentation (#3113, [ADR-0077](../../../../../docs/decisions/0077-doctrines-are-guardrails-not-dogma.md))
//!
//! `crate::display::` mixes two different things under one module:
//! - **Canonical codec** — `to_npub` (and any future `to_bech32`/`to_note`/
//!   `to_nevent`/`to_nprofile`/`to_naddr`): deterministic, lossless,
//!   context-free hex↔bech32 conversion. This is TYPE conversion, not
//!   display, and calling it from a projection builder is legitimate —
//!   banning it would force every shell (native, wasm, TS) to reimplement
//!   the same codec, the exact SSOT violation this rule's siblings exist to
//!   prevent. **Not banned.**
//! - **Presentation formatting** — `short_npub` (truncation), `short_hex`,
//!   `avatar_initials`, `display_name_initials`, `avatar_color_hex`,
//!   `format_ago_secs`: lossy, context-dependent presentation decisions that
//!   belong to the shell. **Banned.**
//!
//! ## What this catches
//!
//! - `crate::display::short_npub(`, `crate::display::short_hex(`,
//!   `crate::display::avatar_initials(`,
//!   `crate::display::display_name_initials(`,
//!   `crate::display::avatar_color_hex(`, `crate::display::format_ago_secs(`
//!   — the presentation-formatting entry points of the display module.
//!   Calling them in projection code bakes locale-specific English into the
//!   wire format, violating ADR-0072.
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

/// Presentation-formatting entry points of `crate::display::` banned in
/// projection/error producer files. Canonical bech32 codec helpers
/// (`to_npub`, and any future `to_bech32`/`to_note`/`to_nevent`/
/// `to_nprofile`/`to_naddr`) are deliberately absent from this list — hex↔
/// bech32 conversion is deterministic, lossless, context-free TYPE
/// conversion, not display formatting (#3113, ADR-0077). Each entry is
/// `(token, message, suggested)`.
const BANNED_DISPLAY_HELPERS: &[(&str, &str, &str)] = &[
    (
        "crate::display::short_npub(",
        "`crate::display::short_npub(` (truncation) called in a kernel projection \
         builder violates ADR-0072 (V-115): projections must send raw data; \
         shells format for display. This is presentation formatting, not the \
         canonical `to_npub` bech32 codec — the codec is exempt, the truncation \
         is not (#3113, ADR-0077)",
        "send the raw `pubkey: String` (hex); the shell converts to bech32 and \
         truncates on the host side",
    ),
    (
        "crate::display::short_hex(",
        "`crate::display::short_hex(` (truncation) called in a kernel projection \
         builder violates ADR-0072 (V-115): projections must send raw data; \
         shells format for display",
        "send the raw hex `String`; the shell abbreviates on the host side",
    ),
    (
        "crate::display::avatar_initials(",
        "`crate::display::avatar_initials(` called in a kernel projection builder \
         violates ADR-0072 (V-115): projections must send raw data; shells derive \
         presentation initials themselves",
        "send the raw `pubkey: String` (hex); the shell derives initials on the \
         host side",
    ),
    (
        "crate::display::display_name_initials(",
        "`crate::display::display_name_initials(` called in a kernel projection \
         builder violates ADR-0072 (V-115): projections must send raw data; \
         shells derive presentation initials themselves",
        "send the raw display-name `String`; the shell derives initials on the \
         host side",
    ),
    (
        "crate::display::avatar_color_hex(",
        "`crate::display::avatar_color_hex(` called in a kernel projection builder \
         violates ADR-0072 (V-115): projections must send raw data; shells derive \
         presentation tint themselves",
        "send the raw `pubkey: String` (hex); the shell derives avatar tint on the \
         host side",
    ),
    (
        "crate::display::format_ago_secs(",
        "`crate::display::format_ago_secs(` called in a kernel projection builder \
         violates ADR-0072 (V-115): projections must send raw data; shells format \
         relative time themselves",
        "send the raw Unix-seconds `u64`; the shell formats relative time on the \
         host side",
    ),
];

/// Banned tokens in projection/error producer files unrelated to
/// `crate::display::`. Each entry is `(token, message, suggested)`.
const BANNED: &[(&str, &str, &str)] = &[
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

/// Returns `(col, message, suggested)` for each banned token on `line`.
/// `is_comment` and `in_test_cfg` suppress the scan. Canonical bech32 codec
/// calls (`crate::display::to_npub(` and friends) are never in either banned
/// list — see [`BANNED_DISPLAY_HELPERS`]'s doc comment (#3113, ADR-0077).
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (token, message, suggested) in BANNED_DISPLAY_HELPERS.iter().chain(BANNED) {
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
#[path = "d19/tests.rs"]
mod tests;
