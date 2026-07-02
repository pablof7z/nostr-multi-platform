//! D11 — no bypass of the one event-producing doorway.
//!
//! PR-F deleted the bespoke event-producing `extern "C"` publish surface —
//! `nmp_app_publish_signed_event`, `nmp_app_publish_signed_event_to`, and
//! `nmp_app_publish_unsigned_event` are gone. ADR-0071 replaced that C-ABI
//! shape with a typed byte-transport doorway: the sole native surface is now
//! UniFFI, and the sole publish door is `NmpApp::dispatch_action` (the
//! `#[uniffi::export] impl NmpApp` method in `crates/nmp-uniffi/src/lib.rs`,
//! which forwards to `nmp_uniffi_support::dispatch_action_vec`). D11 guards
//! that doorway.
//!
//! D11 prevents the doorway from being bypassed. A bypass is a bespoke
//! `#[uniffi::export]`-attributed method (or free function) whose body itself
//! constructs a publish command — i.e. any `#[uniffi::export]` surface, other
//! than `dispatch_action` itself, that sends
//! `ActorCommand::Publish(PublishCommand::SignedEvent { ... })` or
//! `ActorCommand::Publish(PublishCommand::UnsignedEvent { ... })` (or the
//! split-construction bare-variant loophole) is a regression: every publish
//! must funnel through the one typed byte-transport door, not a new
//! special-purpose UniFFI method.
//!
//! A bare `#[no_mangle] extern "C" fn nmp_app_publish_*` symbol is also
//! flagged unconditionally — this is a cheap tombstone guard against the
//! deleted C-ABI publish doors being resurrected; the live surface is UniFFI,
//! not C-ABI, so this shape should never reappear at all.
//!
//! ## What this catches
//!
//! A function signature whose symbol starts with `nmp_app_publish_` inside an
//! `extern "C"` block is flagged unconditionally (the C-ABI tombstone).
//! Inside the body of any `#[uniffi::export]`-attributed `impl` block or
//! free function, a line that mentions
//! `ActorCommand::Publish(PublishCommand::SignedEvent` or
//! `ActorCommand::Publish(PublishCommand::UnsignedEvent` (or
//! `ActorCommand::PublishUnsignedEvent`) is flagged. Split-construction
//! bypass (a bare `PublishCommand::SignedEvent` / `PublishCommand::UnsignedEvent`
//! assigned to a local before being wrapped) is also caught, closing the
//! two-line split-assignment loophole.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D11 — reason` opt-out (the standard
//!   doctrine escape hatch — same shape as D0/D6/D8/D9).
//!
//! ## Known imprecision: attribute detection is not comment-aware
//!
//! [`FnTracker`] looks for the literal substring `#[uniffi::export]` on any
//! line, including inside a `//`/`///`/`//!` doc comment — the same
//! precedent the deleted `extern "C" fn nmp_app_*` tracker set (it matched
//! `extern "C"` + `nmp_app_` textually too). A module doc comment that
//! *mentions* the attribute (several exist in `crates/nmp-uniffi/src/`,
//! e.g. `//! ... adds a `#[uniffi::export] impl NmpApp` block.`) can park
//! the pending flag early and promote it on the next real `impl`/`fn`
//! opener even if that opener is not actually attributed. This only makes
//! the rule *more* conservative (a wider "in scope" window), never less —
//! for a doorway-bypass guard that is the safe direction to err in.
//!
//! ## Scope
//!
//! The driver runs D11 on every file the rest of doctrine-lint visits (no
//! separate path scoping). In practice every offending callsite must live in
//! `crates/nmp-uniffi/src/` — that is the only crate whose source carries
//! `#[uniffi::export]`.

pub const ID: &str = "D11";

/// Banned `ActorCommand::*` patterns that must not appear inside a
/// `#[uniffi::export]`-attributed body.
///
/// Each entry is `(match_substr, display_name)`. `match_substr` is the
/// literal substring searched in the source line; `display_name` is the
/// token emitted in the diagnostic message (for stable test assertions
/// and readable output independent of the sub-enum nesting depth).
///
/// After the ADR-0071 sub-enum collapse the on-disk tokens are
/// `ActorCommand::Publish(PublishCommand::SignedEvent {` and
/// `ActorCommand::Publish(PublishCommand::UnsignedEvent` / `UnsignedEventToRelays`
/// — the old flat variants are gone. The display names are kept stable so
/// that existing diagnostic-string assertions do not need changing.
const BANNED_VARIANTS: &[(&str, &str)] = &[
    (
        "ActorCommand::Publish(PublishCommand::SignedEvent",
        "ActorCommand::PublishSignedEvent",
    ),
    (
        "ActorCommand::PublishUnsignedEvent",
        "ActorCommand::PublishUnsignedEvent",
    ),
    (
        "ActorCommand::Publish(PublishCommand::UnsignedEvent",
        "ActorCommand::PublishUnsignedEvent",
    ),
    // Split-construction bypass: bare variant assigned to a local before
    // being wrapped in ActorCommand::Publish(). Flagging the bare
    // PublishCommand::* occurrence closes the two-line loophole.
    (
        "PublishCommand::SignedEvent",
        "ActorCommand::PublishSignedEvent",
    ),
    (
        "PublishCommand::UnsignedEvent",
        "ActorCommand::PublishUnsignedEvent",
    ),
];

/// Per-line check.
///
/// `in_uniffi_export_scope` says whether the cursor is currently inside the
/// body of a `#[uniffi::export]`-attributed `impl` block or free function.
/// The caller advances the per-file [`FnTracker`] before invoking `check`
/// (same shape as the D8 hot-path tracker). When the cursor is outside such
/// a scope, the `BANNED_VARIANTS` half of D11 is a no-op — the bare C-ABI
/// tombstone check still runs unconditionally.
pub fn check(
    line: &str,
    is_comment: bool,
    in_uniffi_export_scope: bool,
) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();
    if let Some((col, symbol)) = find_banned_publish_symbol(line) {
        hits.push((
            col,
            format!(
                "`{symbol}` violates D11 — bespoke `nmp_app_publish_*` C-ABI doors \
                 are deleted; route through `NmpApp::dispatch_action` (the sole \
                 UniFFI publish doorway)"
            ),
            "delete the publish-specific C symbol; expose publish through the \
             typed action namespace instead"
                .to_string(),
        ));
    }
    if !in_uniffi_export_scope {
        return hits;
    }
    // Track matched byte ranges so that a shorter sub-pattern (e.g. the bare
    // `PublishCommand::SignedEvent` fallback) is not reported twice when the
    // longer inline pattern (`ActorCommand::Publish(PublishCommand::SignedEvent`)
    // already matched and covers the same substring.
    let mut matched_ranges: Vec<(usize, usize)> = Vec::new();
    for (pattern, display) in BANNED_VARIANTS {
        if let Some(rel) = line.find(pattern) {
            let end = rel + pattern.len();
            // Skip if this match's start falls within an already-reported span.
            if matched_ranges.iter().any(|&(s, e)| rel >= s && rel < e) {
                continue;
            }
            matched_ranges.push((rel, end));
            hits.push((
                rel + 1, // 1-indexed columns for clippy compatibility
                format!(
                    "`{}` inside a `#[uniffi::export]` body violates D11 — bespoke \
                     event-producing UniFFI methods bypass the one publish doorway; \
                     route through `NmpApp::dispatch_action(\"nmp.publish\", ...)` instead",
                    display
                ),
                "remove the bespoke publish construction; let host callers dispatch \
                 through the generic action seam (see `crates/nmp-core/src/substrate/action.rs` \
                 Theme A discriminator)"
                    .to_string(),
            ));
        }
    }
    hits
}

fn find_banned_publish_symbol(line: &str) -> Option<(usize, String)> {
    if !line.contains("extern \"C\"") || !line.contains("nmp_app_publish_") {
        return None;
    }
    let idx = line.find("nmp_app_publish_")?;
    let symbol = parse_nmp_app_verb(&line[idx..])?;
    if symbol.starts_with("nmp_app_publish_") {
        Some((idx + 1, symbol))
    } else {
        None
    }
}

/// Per-file tracker — same shape as the analogous D8 hot-path tracker used to
/// have before the `extern "C"` surface was deleted, generalised to recognise
/// the live `#[uniffi::export]` doorway shape instead.
///
/// Walks the brace structure of the file. When a line contains the literal
/// `#[uniffi::export]` attribute, a pending flag is parked (surviving
/// intervening doc-comment / other-attribute lines, since the attributed
/// item is not always on the very next line). The next line that *opens* an
/// `impl ... {` block or a `fn ...(...) {` — same-line brace only, mirroring
/// every other doctrine-lint tracker's simplifying assumption — promotes the
/// pending flag to a real stack frame; the frame (and everything nested
/// inside it, since `#[uniffi::export]` on an `impl` covers every method in
/// the block) stays "in scope" until the brace closes.
#[derive(Default)]
pub struct FnTracker {
    /// Brace depth across the file (all `{` minus all `}`).
    cur_depth: i32,
    /// Stack: one entry (its `open_depth`) per open `#[uniffi::export]`-
    /// attributed scope. When `cur_depth` drops back to `open_depth`, pop.
    fn_stack: Vec<i32>,
    /// True once a `#[uniffi::export]` attribute line has been seen and no
    /// matching opener has promoted it yet. Cleared on promotion.
    pending_export: bool,
}

impl FnTracker {
    /// True iff the *current* line is inside a `#[uniffi::export]`-attributed
    /// scope. Caller invokes [`Self::observe_line`] after reading this value
    /// to advance the tracker.
    pub fn in_uniffi_export_scope(&self) -> bool {
        !self.fn_stack.is_empty()
    }

    /// Advance the tracker by one line of file text.
    ///
    /// `starts_in_block_comment` short-circuits a body-of-`/* ... */` line
    /// — the walker's brace counter ignores those, so D11's mirror counter
    /// must too, lest the two disagree and the stack drift.
    pub fn observe_line(&mut self, line: &str, starts_in_block_comment: bool) {
        if starts_in_block_comment {
            return;
        }
        let (opens, closes) = count_braces_ignoring_strings(line);

        if line.contains("#[uniffi::export]") || line.contains("#[uniffi::export(") {
            self.pending_export = true;
        } else if self.pending_export && opens_export_scope_with_brace(line) {
            self.fn_stack.push(self.cur_depth);
            self.pending_export = false;
        }

        // Apply the brace delta.
        self.cur_depth += opens as i32;
        self.cur_depth -= closes as i32;

        // Pop any frames whose open_depth is ≥ cur_depth.
        while let Some(&open_depth) = self.fn_stack.last() {
            if self.cur_depth <= open_depth {
                self.fn_stack.pop();
            } else {
                break;
            }
        }
    }
}

/// True iff `line` opens an `impl ... {` block or a `fn ...(...) {` with the
/// brace on this same line — the shapes a `#[uniffi::export]` attribute
/// decorates. Deliberately same-line-only, matching the simplifying
/// assumption every other doctrine-lint per-file tracker makes (wrapped
/// multi-line signatures are rare for the UniFFI surface this rule targets).
fn opens_export_scope_with_brace(line: &str) -> bool {
    if !line.contains('{') {
        return false;
    }
    let trimmed = line.trim_start();
    trimmed.starts_with("impl ") || trimmed.starts_with("fn ") || line.contains(" fn ")
}

/// Given a slice starting at the verb identifier (e.g. `nmp_app_foo(...)`),
/// extract the full identifier as a `String`. Returns `None` if the slice
/// does not start with `nmp_app_` (defensive).
fn parse_nmp_app_verb(s: &str) -> Option<String> {
    if !s.starts_with("nmp_app_") {
        return None;
    }
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    Some(s[..end].to_string())
}

/// Count `{` and `}` characters, ignoring those inside `"..."` string
/// literals (with `\"` escape handling) and `//` line comments. A copy of
/// `walker::count_braces_ignoring_strings` — duplicated to keep the rule
/// self-contained for the LOC budget, matching D8's pattern.
fn count_braces_ignoring_strings(line: &str) -> (usize, usize) {
    let bytes = line.as_bytes();
    let mut opens = 0;
    let mut closes = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // rest of line is a // comment
        } else if b == b'{' {
            opens += 1;
        } else if b == b'}' {
            closes += 1;
        }
        i += 1;
    }
    (opens, closes)
}

#[cfg(test)]
#[path = "d11/tests.rs"]
mod tests;
