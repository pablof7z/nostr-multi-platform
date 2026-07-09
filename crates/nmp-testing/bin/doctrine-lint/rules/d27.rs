//! D27 — banned display helpers in projection / snapshot / FFI serialization.
//!
//! ADR-0072 §3 deferred this lint (see §"What this ADR does *not* do").
//! Regression history: pubkey-to-npub and "ago" formatting leaked into the
//! snapshot wire format twice (#1099 signer-label, #623 wallet-status).
//!
//! ## What this catches
//!
//! **Part A — banned display-helper calls** in projection / snapshot / FFI
//! serialization code in `nmp-core` (projection paths only), `nmp-nip*`, and
//! `nmp-marmot`:
//! - `short_npub(` — bech32 abbreviation of a pubkey
//! - `to_npub(` — full bech32 encoding of a pubkey
//! - `short_hex(` — hex abbreviation (first8…last8)
//! - `avatar_initials(` — 2-char avatar seed derived from an npub
//! - `display_name_initials(` — 2-char initials from a display name
//! - `avatar_color_hex(` — DJB2-derived avatar background colour
//! - `format_ago_secs(` — relative-time "5m ago" string
//!
//! **Part B — precomputed `*_label` / `*_display` String struct fields** in
//! the same paths. These bake English display strings into the projection wire
//! format — the shape that regressed in #1099 (`signer_label`) and #623
//! (`status_label`).
//!
//! ## Scope
//!
//! - `crates/nmp-nip*/src/` — entire protocol-crate source trees.
//! - `crates/nmp-marmot/src/` — entire marmot source (minus
//!   `projection/display.rs`, which ADR-0072 explicitly permits as free-form
//!   metadata fallbacks for the MarmotGroupRow name field).
//! - `crates/nmp-core/src/` — projection-specific paths only to avoid false
//!   positives in the logging / debug subtrees:
//!   - `kernel/update/`
//!   - `kernel/typed_projections/`
//!   - `actor/typed_projections/`
//!   - `kernel/types.rs`
//!   - `kernel/publish_outbox.rs`
//!
//! ## Exemptions
//!
//! - Presentation shells: `nmp-cli` (the CLI is the correct place for display
//!   formatting). `apps/chirp` was extracted to its own repository (see
//!   `tests_d16_workspace.rs::apps_chirp_directory_does_not_exist`) — it no
//!   longer exists in this monorepo, so its former special-case here was
//!   dead code and was removed (#2761 / #2769 item 9).
//! - `#[cfg(test)]` module bodies (`in_test_cfg`).
//! - Test-only files (`d6::file_is_test_only` in the driver).
//! - Generated files (`**/generated/**`).
//! - The doctrine-lint binary's own source (contains the banned tokens as
//!   string constants — meta-false-positives on broad sweeps).
//! - `nmp-marmot/src/projection/display.rs` — free-form name fallbacks
//!   explicitly permitted by ADR-0072 (not pubkey/timestamp formatters).
//!
//! ## Per-line opt-out
//!
//! `// doctrine-allow: D27 — reason` suppresses the finding on that line. An
//! allow that suppresses nothing is itself a finding ([`findings_for_line`] /
//! [`stale_allow`], #1712) so dead markers can't silently rot.

use std::path::Path;

pub const ID: &str = "D27";

/// Part A — banned display-helper call tokens.
/// Each entry: (token, short violation tag for the message).
const BANNED_CALLS: &[(&str, &str)] = &[
    ("short_npub(", "short_npub"),
    ("to_npub(", "to_npub"),
    ("short_hex(", "short_hex"),
    ("avatar_initials(", "avatar_initials"),
    ("display_name_initials(", "display_name_initials"),
    ("avatar_color_hex(", "avatar_color_hex"),
    ("format_ago_secs(", "format_ago_secs"),
];

/// Part B — precomputed-field suffix patterns.
const PRECOMPUTED_SUFFIXES: &[&str] = &["_label:", "_display:"];

/// Type-annotation prefixes that follow `_label:` / `_display:` in a struct
/// field declaration. A match here distinguishes a field type annotation from
/// a struct-construction value (`r.kinds_label().to_string()` does not start
/// with any of these).
const STRING_TYPE_PREFIXES: &[&str] = &["String", "Option<String", "Vec<String"];

const CALL_SUGGESTED: &str =
    "send raw protocol data from NMP (hex pubkey, Unix-second u64, machine token); \
     format for display in the shell using its own locale/TZ/bech32 helpers";

const FIELD_SUGGESTED: &str =
    "remove the precomputed `_label`/`_display` field; send the raw underlying \
     value and let the shell derive the display string on the host side";

/// Message for a stale `// doctrine-allow: D27` marker (a marker on a line that
/// carries no D27 violation to suppress — see #1712).
const STALE_ALLOW_MSG: &str =
    "stale `// doctrine-allow: D27` marker — this line has no D27 finding to \
     suppress, so the escape silences nothing. A relocation PR likely removed \
     the projection-label / display-helper it once covered, leaving the marker \
     to rot";

const STALE_ALLOW_SUGGESTED: &str =
    "delete the now-unused `// doctrine-allow: D27` comment; an allow must \
     always sit on a line that genuinely fires D27";

/// Returns `true` if D27 should scan `path`.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");

    // Never fire in doctrine-lint's own source (meta-false-positives).
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    // Never fire in generated directories.
    if s.contains("/generated/") {
        return false;
    }
    // Allowlisted presentation shell — the CORRECT place for display
    // formatting. `apps/chirp` was extracted to its own repository and no
    // longer exists in this monorepo; its former exemption clauses here were
    // removed (#2761 / #2769 item 9).
    if s.contains("/crates/nmp-cli/") || s.ends_with("/nmp-cli") {
        return false;
    }

    // nmp-nip* protocol crates: entire src/ tree.
    if (s.contains("/crates/nmp-nip") || s.contains("crates/nmp-nip")) && s.contains("/src/") {
        return true;
    }

    // nmp-marmot: entire src/, but exempt the ADR-0072-permitted display module
    // (free-form name fallbacks — not pubkey/timestamp formatters).
    if s.contains("/crates/nmp-marmot/src/") || s.contains("crates/nmp-marmot/src/") {
        // projection/display.rs is explicitly permitted by ADR-0072.
        if s.ends_with("/projection/display.rs") || s.ends_with("projection/display.rs") {
            return false;
        }
        return true;
    }

    // nmp-core: only the projection-specific paths (not logging/debug subtrees
    // such as kernel/status.rs, kernel/ingest/, kernel/requests/ where
    // `short_hex(` is legitimately used for debug log formatting).
    let nmp_core_dirs = [
        "/crates/nmp-core/src/kernel/update/",
        "/crates/nmp-core/src/kernel/typed_projections/",
        "/crates/nmp-core/src/actor/typed_projections/",
        "crates/nmp-core/src/kernel/update/",
        "crates/nmp-core/src/kernel/typed_projections/",
        "crates/nmp-core/src/actor/typed_projections/",
    ];
    let nmp_core_files = [
        "crates/nmp-core/src/kernel/types.rs",
        "crates/nmp-core/src/kernel/publish_outbox.rs",
    ];
    if nmp_core_dirs.iter().any(|p| s.contains(p)) {
        return true;
    }
    if nmp_core_files.iter().any(|p| s.contains(p)) {
        return true;
    }

    // Gallery app crate's UniFFI snapshot/wire adapters (#3098): the #3095
    // scanner fix (#3104) widened doctrine-lint's walk to `apps/*`, but this
    // allowlist still excluded `apps/*` entirely — `snapshot_json.rs` baked
    // `npub`/`npub_short` straight into the UniFFI wire (short_npub(),
    // to_npub()) and went uncaught. Any file under an
    // `apps/nmp-gallery/crates/*/src/` tree is a projection/FFI-serialization
    // adapter in the same sense as the nmp-core paths above.
    if s.contains("apps/nmp-gallery/crates/") && s.contains("/src/") {
        return true;
    }

    false
}

/// Returns `(col, message, suggested)` for each D27 finding on `line`.
///
/// `is_comment` and `in_test_cfg` suppress the entire scan (doc comments +
/// `#[cfg(test)]` module bodies are exempt).
pub fn check(line: &str, is_comment: bool, in_test_cfg: bool) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();

    // ── Part A: banned display-helper function calls ──────────────────────
    for (token, name) in BANNED_CALLS {
        let mut start = 0;
        while let Some(rel) = line[start..].find(token) {
            let col = start + rel + 1; // 1-indexed
            hits.push((
                col,
                format!(
                    "`{name}(` called in a projection / snapshot / FFI serialization \
                     path violates ADR-0072 (D27): projection code must emit raw \
                     protocol data; display formatting belongs in the shell"
                ),
                CALL_SUGGESTED.to_string(),
            ));
            start += rel + token.len();
        }
    }

    // ── Part B: precomputed *_label / *_display String struct fields ──────
    // Skip local `let` / `let mut` bindings — they are not field declarations.
    let trimmed = line.trim_start();
    if !trimmed.starts_with("let ") && !trimmed.starts_with("let mut ") {
        for suffix in PRECOMPUTED_SUFFIXES {
            if let Some(pos) = line.find(suffix) {
                // The colon is the last char of `suffix`; look at what follows.
                let after_colon = line[pos + suffix.len()..].trim_start();
                let is_string_type = STRING_TYPE_PREFIXES
                    .iter()
                    .any(|t| after_colon.starts_with(t));
                if is_string_type {
                    // Extract a human-readable field name from the line for the
                    // message (best-effort — not a full parser).
                    // `pos` points at the first char of `suffix` (the `_`);
                    // stop just before the trailing `:` to get the full name.
                    let field_end = pos + suffix.len() - 1; // exclusive: stop before ':'
                    let field_start = line[..field_end]
                        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    let field_name = &line[field_start..field_end];
                    hits.push((
                        pos + 1,
                        format!(
                            "precomputed `{field_name}:` `String` field in a projection \
                             type violates ADR-0072 (D27): projection structs must carry \
                             raw protocol data; the shell derives display strings on the \
                             host side (see regressions #1099 and #623)"
                        ),
                        FIELD_SUGGESTED.to_string(),
                    ));
                }
            }
        }
    }

    hits
}

/// All D27 findings for one source line, given whether it carries a
/// `doctrine-allow: D27` marker (`allowed`): `!allowed` → every real violation
/// [`check`] finds; `allowed` + none on a real (non-comment, non-test) line →
/// the stale-marker finding (#1712); otherwise (legit suppression) → nothing.
pub fn findings_for_line(
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
    allowed: bool,
) -> Vec<(usize, String, String)> {
    let hits = check(line, is_comment, in_test_cfg);
    if !allowed {
        return hits;
    }
    if hits.is_empty() && !is_comment && !in_test_cfg {
        return stale_allow(line).into_iter().collect();
    }
    Vec::new()
}

/// Build the stale-`doctrine-allow: D27` finding for `line`.
///
/// The driver calls this only when it has already determined that the line
/// carries a `// doctrine-allow: D27` marker **and** [`check`] produced no
/// finding on that line — i.e. the marker suppresses nothing and has rotted
/// (#1712). Returns `(col, message, suggested)` pointing at the marker, or
/// `None` if the marker text cannot be located (defensive — the caller
/// guarantees it is present).
pub fn stale_allow(line: &str) -> Option<(usize, String, String)> {
    let pos = line.find("// doctrine-allow:")?;
    Some((
        pos + 1, // 1-indexed column of the marker
        STALE_ALLOW_MSG.to_string(),
        STALE_ALLOW_SUGGESTED.to_string(),
    ))
}

#[cfg(test)]
#[path = "d27/tests.rs"]
mod tests;
