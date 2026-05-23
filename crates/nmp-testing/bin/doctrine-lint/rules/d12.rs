//! D12 — async-completing action modules must record stages.
//!
//! Per PR-G, `ActionModule::is_async_completing()` is a registry marker
//! returning `false` by default. A module that overrides it to `true` is
//! declaring a contract with the host: the action will produce a
//! lifecycle observable through `projections["action_stages"]`, and the
//! module MUST record stage transitions via
//! [`Kernel::record_action_stage`] (or any of its callers) so the mirror
//! reflects reality. A module that flips the marker without ever
//! recording a stage ships an empty stage seam — a silent host-visible
//! bug: the host's progress indicator never updates.
//!
//! ## What this catches
//!
//! Per-file scan. A file declaring a `fn is_async_completing(` body that
//! returns `true` (i.e. the line contains both `is_async_completing` and a
//! `true` literal) MUST also contain at least one of:
//!
//!   * `record_action_stage(` — the kernel-side recorder
//!   * `record_action_terminal_failure(` — the engine-side sibling already
//!     covered by the publish path
//!   * `record_action_failure(` — the kernel wrapper that fans both
//!
//! If none appears in the same file the rule fires on the
//! `is_async_completing` line. This is intentionally grep-level (matching
//! D8/D9): a richer AST check is a future-work item documented inline.
//!
//! ## Scope
//!
//! Protocol/substrate crates only — every `crates/nmp-*/src/` tree, plus
//! app-layer crates under `apps/<app>/`. The `nmp-testing` crate is
//! exempt (it hosts negative-example fixtures for this rule).
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D12 — reason` opt-out on the
//!   `is_async_completing` declaration.
//!
//! ## Known false-negative: cross-file declaration / recording
//!
//! The grep-level scan is per-FILE. A module whose `is_async_completing`
//! declaration lives in one file (the `impl ActionModule for FooModule`
//! block) but whose `record_action_stage` calls live in a sibling
//! (engine, executor, actor dispatch handler) slips through — exactly
//! the shape `nmp-core`'s `PublishModule` has. Tightening the rule to
//! span files would require crate-wide cross-file scanning, the
//! AST-level check the PR-G spec explicitly deferred. The runtime
//! coverage is by `kernel/action_stages_tests.rs` — a recording-missing
//! regression is caught there, just not at lint time. Documented on
//! `PublishModule::is_async_completing` so a reader sees the pointer
//! to the real call sites.

use std::path::Path;

pub const ID: &str = "D12";

/// True iff D12 should scan `path`. Production protocol + app crates
/// scoped; `nmp-testing` is exempt (fixture host with negative examples).
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    let in_crates = s.contains("/crates/nmp-") || s.starts_with("crates/nmp-");
    let in_apps = s.contains("/apps/") || s.starts_with("apps/");
    if !(in_crates || in_apps) {
        return false;
    }
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    true
}

/// Per-file finding type: the line number of the offending
/// `is_async_completing` declaration plus a rendered message. The driver
/// re-uses this to emit a standard `Finding`.
#[derive(Debug)]
pub struct AsyncMarkerHit {
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub suggested: String,
}

/// Scan a whole file's contents in one pass: collect every
/// `is_async_completing` → `true` declaration (single-line OR multi-line
/// body) and every stage-recording call site; emit a finding for each
/// marker that has no recording sibling. Returns an empty `Vec` for files
/// that either don't declare the marker or do declare it AND record stages.
///
/// Two cheap loops over the file lines:
///   1. find every `fn is_async_completing(` declaration; for each, scan
///      the function body across newlines (tracking `{` / `}` depth)
///      looking for a `true` literal. Both single-line and multi-line
///      shapes match — the multi-line scan was added in PR-G2 to cover
///      `PublishModule`'s declaration shape and prevent trivial formatting
///      bypass for future modules.
///   2. find any line that contains a recording call.
///
/// `is_comment` is computed by the caller (the walker tracks comment
/// state across multi-line `/* */` blocks). The driver also honours
/// `// doctrine-allow: D12 — ...` on the marker line.
pub fn scan_file(text: &str, line_is_comment: &[bool]) -> Vec<AsyncMarkerHit> {
    let lines: Vec<&str> = text.lines().collect();
    let mut markers: Vec<(usize, usize)> = Vec::new();
    let mut has_recording_call = false;

    for (idx, line) in lines.iter().enumerate() {
        let is_comment = line_is_comment.get(idx).copied().unwrap_or(false);
        if !is_comment {
            // The declaration line is where the finding is anchored.
            // `body_returns_true` widens the scan across newlines if the
            // body is multi-line.
            if line_declares_async_marker(line) && body_returns_true(&lines, line_is_comment, idx)
            {
                let col = line.find("is_async_completing").unwrap_or(0) + 1;
                markers.push((idx + 1, col));
            }
            if line_records_action_stage(line) {
                has_recording_call = true;
            }
        }
    }

    if has_recording_call {
        return Vec::new();
    }
    markers
        .into_iter()
        .map(|(line, col)| AsyncMarkerHit {
            line,
            col,
            message: "`is_async_completing` returns `true` but this file never calls \
                 `record_action_stage` — D12 requires async-completing modules \
                 to record stage transitions so the `action_stages` mirror \
                 reflects reality"
                .to_string(),
            suggested:
                "call `kernel.record_action_stage(correlation_id, stage, detail)` \
                 from the module's executor or actor handler — see \
                 `crates/nmp-core/src/kernel/publish_engine.rs` for the canonical \
                 publish-path consumer"
                    .to_string(),
        })
        .collect()
}

/// True iff `line` is the *declaration* line of an `is_async_completing`
/// function — i.e. contains both `fn ` and the identifier. The body
/// inspection (does it return `true`?) is split out into
/// [`body_returns_true`] so multi-line bodies are covered.
///
/// The `fn` prefix prevents false positives from a doc-comment or method
/// CALL — `is_async_completing()` invocations don't have `fn` before
/// them. The comment / `doctrine-allow` filtering is the caller's job.
fn line_declares_async_marker(line: &str) -> bool {
    line.contains("is_async_completing") && line.contains("fn ")
}

/// True iff the function body whose declaration starts at `lines[start]`
/// contains a `true` literal anywhere (with `false` literals also present
/// only ruling it out when NO `true` is present). Handles both shapes the
/// codebase uses:
///
///   1. one-liner with body on the declaration line:
///      `fn is_async_completing() -> bool { true }`
///   2. multi-line body — the PR-G2 case `PublishModule` exemplifies:
///      ```ignore
///      fn is_async_completing() -> bool {
///          true
///      }
///      ```
///
/// Algorithm: locate the first `{` on or after the declaration line,
/// then walk forward tracking brace depth. Inside the body (depth ≥ 1)
/// collect every `true` literal that is NOT in a comment. Return `true`
/// when we found at least one such literal before the matching closing
/// brace.
///
/// `line_is_comment` carries the walker's per-line comment mask so a
/// `true` inside a doc-comment inside the body cannot satisfy the rule.
/// Comments INSIDE source lines (after `//`) are stripped via
/// [`strip_line_comment`] so a trailing `// always true` doesn't make a
/// `false`-returning body look async-completing.
fn body_returns_true(lines: &[&str], line_is_comment: &[bool], start: usize) -> bool {
    // 1. Find the opening `{` of the function body. It is either on the
    //    declaration line OR on a following non-comment line. If we ever
    //    hit a `;` (a trait method DECLARATION with no body — e.g. inside
    //    a `trait` block) before the `{`, there is no body to scan.
    let mut idx = start;
    let mut found_open = false;
    while idx < lines.len() {
        let is_comment = line_is_comment.get(idx).copied().unwrap_or(false);
        if is_comment {
            idx += 1;
            continue;
        }
        let code = strip_line_comment(lines[idx]);
        // Stop at trait-method declaration with no body.
        if let Some(semi_pos) = code.find(';') {
            // A `;` BEFORE the first `{` means no body.
            if !code[..semi_pos].contains('{') {
                return false;
            }
        }
        if code.contains('{') {
            found_open = true;
            break;
        }
        idx += 1;
    }
    if !found_open {
        return false;
    }

    // 2. Walk forward from `idx`, tracking brace depth across lines.
    //    `depth` starts at 0; the first `{` we encounter on `lines[idx]`
    //    bumps it to 1 (we are now inside the function body).
    //
    //    For single-line bodies (`{ true }`) depth ends the line at 0 but
    //    a `true` literal between `{` and `}` is still inside the body.
    //    Track depth char-by-char and check `true` only when at depth ≥ 1
    //    at that point in the line — the single-line and multi-line shapes
    //    both fall through this branch identically.
    let mut depth: i32 = 0;
    let mut saw_true = false;
    for (cursor, line) in lines.iter().enumerate().skip(idx) {
        let is_comment = line_is_comment.get(cursor).copied().unwrap_or(false);
        if is_comment {
            continue;
        }
        let code = strip_line_comment(line);
        // Char-by-char walk:
        //   - `{` opens a scope (depth += 1)
        //   - `}` closes a scope; if depth → 0 the function body ended
        //   - inside the body (depth ≥ 1) check for `true` at the current
        //     prefix position. The boundary check in `contains_word_true`
        //     prevents false positives from `truely` / `intrue` /
        //     `_true_count`.
        let bytes = code.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        // End of body — return what we found.
                        return saw_true;
                    }
                }
                _ => {
                    // Cheap check: are we inside the body and looking at the
                    // start of `true`? Avoids re-scanning the whole line.
                    if depth >= 1
                        && b == b't'
                        && bytes.get(i + 1).copied() == Some(b'r')
                        && bytes.get(i + 2).copied() == Some(b'u')
                        && bytes.get(i + 3).copied() == Some(b'e')
                    {
                        let left_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                        let right_ok =
                            i + 4 >= bytes.len() || !is_ident_char(bytes[i + 4]);
                        if left_ok && right_ok {
                            saw_true = true;
                            i += 3; // skip past the literal
                        }
                    }
                }
            }
            i += 1;
        }
    }
    // Unbalanced braces (malformed source) — be conservative and report
    // what we saw. A file we can't parse cleanly is a file the next pass
    // should re-scan.
    saw_true
}

/// Return `line` with any trailing `// …` line-comment stripped. A bare
/// occurrence of `//` outside of a string literal is treated as the
/// comment opener — the codebase's source files never put a `//` inside
/// a double-quoted string on a D12-relevant declaration line (the
/// declarations live in `impl ActionModule for …` blocks). A more
/// thorough implementation would lex Rust; the cost-vs-benefit here
/// favours the trivial check the rest of doctrine-lint uses.
fn strip_line_comment(line: &str) -> &str {
    if let Some(pos) = line.find("//") {
        &line[..pos]
    } else {
        line
    }
}

/// True iff `line` contains a `record_action_stage(`, `record_action_failure(`,
/// or `record_action_terminal_failure(` call. The match is plain substring —
/// false positives (a doc comment naming one of these symbols) are filtered
/// by the `is_comment` gate, and a recording call inside a real `#[cfg(test)]`
/// fn still counts (a test exercising the stage seam IS proof the module
/// records).
fn line_records_action_stage(line: &str) -> bool {
    line.contains("record_action_stage(")
        || line.contains("record_action_failure(")
        || line.contains("record_action_terminal_failure(")
}

/// True iff `line` contains the keyword `true` as a standalone token (not
/// e.g. `truely` or `intrue`). Cheap left-and-right boundary check — Rust
/// keywords cannot abut identifier characters in a code-position context.
///
/// PR-G2: kept under `#[cfg(test)]` for the boundary-check unit test. The
/// production scan path (`body_returns_true`) walks bytes directly and
/// inlines the same boundary check; this helper survives only as the
/// targeted unit-test seam for the boundary semantics.
#[cfg(test)]
fn contains_word_true(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(rel) = line[start..].find("true") {
        let pos = start + rel;
        let left_ok = pos == 0 || !is_ident_char(bytes[pos - 1]);
        let right_ok = pos + 4 >= bytes.len() || !is_ident_char(bytes[pos + 4]);
        if left_ok && right_ok {
            return true;
        }
        start = pos + 4;
    }
    false
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
#[path = "d12/tests.rs"]
mod tests;
