//! D11 — one door per publish capability.
//!
//! PR-F deleted the bespoke event-producing `extern "C"` publish surface —
//! `nmp_app_publish_signed_event`, `nmp_app_publish_signed_event_to`, and
//! `nmp_app_publish_unsigned_event` are gone. Every user / app-authored
//! publish-engine event now goes through the single
//! `nmp_app_dispatch_action(app, "nmp.publish", ...)` door (Theme A — see
//! `crates/nmp-core/src/substrate/action.rs` module docs).
//!
//! D11 prevents that door from being silently re-opened. A new
//! `#[no_mangle] extern "C" fn nmp_app_publish_*(...)` is a regression even
//! before its body is inspected. A new `#[no_mangle] extern "C" fn
//! nmp_app_<verb>(...)` whose body sends `ActorCommand::PublishSignedEvent {
//! ... }` or `ActorCommand::PublishUnsignedEvent(...)` is also a regression.
//!
//! ## What this catches
//!
//! A function signature whose symbol starts with `nmp_app_publish_` is flagged.
//! Inside any other function whose signature is
//! `[pub] extern "C" fn nmp_app_<verb>(...)` (the FFI prefix; D11 does not
//! fire inside Rust-only helpers), a line that mentions
//! `ActorCommand::PublishSignedEvent` or `ActorCommand::PublishUnsignedEvent`
//! is flagged. The substring match is deliberately strict — it requires the
//! fully-qualified path component (`ActorCommand::`) so an unrelated local
//! type named `PublishSignedEvent` cannot trip it.
//!
//! ## Whitelist (explicit per PR-F task)
//!
//! Two `nmp_app_*` symbols are publish-lifecycle control-plane (they
//! address an already-queued publish handle, never produce events):
//!
//! - `nmp_app_retry_publish`
//! - `nmp_app_cancel_publish`
//!
//! Their bodies send `ActorCommand::RetryPublish` / `CancelPublish`, not
//! the banned variants — so today they would not fire D11 anyway. The
//! whitelist still exists as a forward guarantee: if a future change
//! incidentally needed to construct a banned variant inside one of these
//! two symbols (which is the wrong design but worth surfacing as the
//! single allowed escape hatch), the lint stays out of the way.
//!
//! ## Allowed exemptions
//!
//! - Comment lines (any of `//`, `///`, `//!`, inside `/* */`).
//! - Per-line `// doctrine-allow: D11 — reason` opt-out (the standard
//!   doctrine escape hatch — same shape as D0/D6/D8/D9).
//! - Whitelisted symbols (above) — their bodies are ignored.
//!
//! ## Scope
//!
//! The driver runs D11 on every file the rest of the doctrine-lint visits
//! (no separate path scoping). In practice every offending callsite must
//! live in `crates/nmp-core/src/ffi/` (that is the only place the FFI
//! prefix is `#[no_mangle] extern "C"`-exported), so the lint only ever
//! fires there in real code.

pub const ID: &str = "D11";

/// Banned `ActorCommand::*` substrings that must not appear inside an
/// `extern "C" fn nmp_app_*` body (outside the whitelist).
const BANNED_VARIANTS: &[&str] = &[
    "ActorCommand::PublishSignedEvent",
    "ActorCommand::PublishUnsignedEvent",
];

/// Whitelisted `nmp_app_*` symbol names whose bodies are not scanned. Per
/// the PR-F task: retry / cancel address a publish handle, never produce
/// events, and have no `dispatch_action` equivalent.
const WHITELISTED_SYMBOLS: &[&str] = &["nmp_app_retry_publish", "nmp_app_cancel_publish"];

/// Per-line check.
///
/// `in_nmp_app_extern_fn` says whether the cursor is currently inside the
/// body of a non-whitelisted `extern "C" fn nmp_app_*`. The caller advances
/// the per-file [`FnTracker`] before invoking `check` (same shape as the D8
/// hot-path tracker). When the cursor is outside such a function, D11 is a
/// no-op.
pub fn check(
    line: &str,
    is_comment: bool,
    in_nmp_app_extern_fn: bool,
) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let mut hits = Vec::new();
    if let Some((col, symbol)) = find_banned_publish_symbol(line) {
        hits.push((
            col,
            format!(
                "`{symbol}` violates D11 — bespoke `nmp_app_publish_*` FFI doors \
                 are deleted; route through `nmp_app_dispatch_action(\"nmp.publish\", ...)`"
            ),
            "delete the publish-specific C symbol; expose publish through the \
             typed action namespace instead"
                .to_string(),
        ));
    }
    if !in_nmp_app_extern_fn {
        return hits;
    }
    for variant in BANNED_VARIANTS {
        if let Some(rel) = line.find(variant) {
            hits.push((
                rel + 1, // 1-indexed columns for clippy compatibility
                format!(
                    "`{}` inside an `extern \"C\" fn nmp_app_*` body violates D11 — \
                     bespoke event-producing FFI was deleted in PR-F; route through \
                     `nmp_app_dispatch_action(\"nmp.publish\", ...)` instead",
                    variant
                ),
                "remove the bespoke FFI symbol; let host callers dispatch through the \
                 generic action seam (see `crates/nmp-core/src/substrate/action.rs` \
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

/// Per-file tracker — same shape as [`super::d8::HotPathTracker`], with
/// extra state for wrapped (multi-line) FFI signatures.
///
/// Walks the brace structure of the file, records when an
/// `extern "C" fn nmp_app_<verb>` opens, and pops the stack when the body
/// closes. Two opener shapes are handled:
///
/// 1. Same-line signature + `{` (the common case):
///    `pub extern "C" fn nmp_app_foo(app: *mut NmpApp) {`
/// 2. Wrapped multi-line signature where `{` lives on a later line. We
///    detect the `extern "C" fn nmp_app_<verb>(` opener and remember the
///    verb in `pending_opener`. Once a subsequent line introduces the
///    matching `{` (the brace delta of the line is ≥ 1, no other
///    same-line `extern "C" fn` opener was seen), we push the stack
///    frame.
///
/// The whitelist is consulted at push time; whitelisted frames flow
/// through `in_nmp_app_extern_fn() == false` so their bodies are not
/// scanned.
#[derive(Default)]
pub struct FnTracker {
    /// Brace depth across the file (all `{` minus all `}`).
    cur_depth: i32,
    /// Stack: one entry per open `extern "C" fn nmp_app_<verb> { ... }`.
    /// `(open_depth, is_whitelisted)`. When `cur_depth` drops back to
    /// `open_depth`, pop. `is_whitelisted = true` means the body is exempt;
    /// `in_nmp_app_extern_fn()` ignores those frames.
    fn_stack: Vec<(i32, bool)>,
    /// Wrapped-signature staging: when an `extern "C" fn nmp_app_<verb>(`
    /// opener is detected without a same-line `{`, the parsed verb is
    /// parked here. The next line whose net brace delta is ≥ 1 promotes
    /// the pending verb to a real `fn_stack` frame. Cleared on promotion
    /// or when a same-line opener with `{` is seen (the latter wins).
    pending_opener: Option<String>,
}

impl FnTracker {
    /// True iff the *current* line is inside a non-whitelisted
    /// `extern "C" fn nmp_app_*` body. Caller invokes [`Self::observe_line`]
    /// after reading this value to advance the tracker.
    pub fn in_nmp_app_extern_fn(&self) -> bool {
        self.fn_stack.iter().any(|(_, whitelisted)| !*whitelisted)
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

        // Same-line opener takes priority over a wrapped pending opener
        // (the wrapped one would have been cleared by now if it had
        // resolved cleanly; an unresolved one was a parse glitch and the
        // same-line shape is authoritative).
        let same_line_verb = find_nmp_app_extern_fn_opener_with_brace(line)
            .and_then(|verb_start| parse_nmp_app_verb(&line[verb_start..]));
        if let Some(verb) = same_line_verb {
            let whitelisted = WHITELISTED_SYMBOLS.contains(&verb.as_str());
            // Push BEFORE applying the brace delta so `open_depth` is the
            // pre-open depth.
            self.fn_stack.push((self.cur_depth, whitelisted));
            self.pending_opener = None;
        } else if let Some(verb) = find_wrapped_nmp_app_extern_fn_opener(line) {
            // Wrapped opener — `extern "C" fn nmp_app_<verb>(` with no
            // same-line `{`. Park the verb; the next net-positive brace
            // delta promotes it.
            self.pending_opener = Some(verb);
        } else if let Some(verb) = self.pending_opener.take() {
            // Continuation of a previously-parked wrapped opener. If this
            // line introduces at least one open brace, promote.
            let net = opens as i32 - closes as i32;
            if net >= 1 {
                let whitelisted = WHITELISTED_SYMBOLS.contains(&verb.as_str());
                self.fn_stack.push((self.cur_depth, whitelisted));
            } else {
                // Still inside the parameter list — keep parking.
                self.pending_opener = Some(verb);
            }
        }

        // Apply the brace delta.
        self.cur_depth += opens as i32;
        self.cur_depth -= closes as i32;

        // Pop any fns whose open_depth is ≥ cur_depth.
        while let Some(&(open_depth, _)) = self.fn_stack.last() {
            if self.cur_depth <= open_depth {
                self.fn_stack.pop();
            } else {
                break;
            }
        }
    }
}

/// Detect a wrapped-signature opener: `extern "C" fn nmp_app_<verb>(` with
/// no same-line `{` (the `{` is on a later line). Returns the parsed verb
/// (e.g. `"nmp_app_create_new_account"`) when matched.
fn find_wrapped_nmp_app_extern_fn_opener(line: &str) -> Option<String> {
    if !line.contains("extern \"C\"") || !line.contains("nmp_app_") {
        return None;
    }
    if line.contains('{') {
        // Same-line opener handled separately.
        return None;
    }
    let extern_pos = line.find("extern \"C\"")?;
    let after_extern = &line[extern_pos..];
    let fn_rel = after_extern.find(" fn ")?;
    let fn_abs = extern_pos + fn_rel + 1;
    let after_fn = &line[fn_abs + 3..];
    let trimmed = after_fn.trim_start();
    if !trimmed.starts_with("nmp_app_") {
        return None;
    }
    parse_nmp_app_verb(trimmed)
}

/// Returns the byte offset of `fn` in a line that opens an
/// `extern "C" fn nmp_app_<verb>(...)` signature with a same-line `{`.
///
/// Accepts the standard FFI shape:
///
/// ```ignore
/// #[no_mangle]
/// pub extern "C" fn nmp_app_foo(app: *mut NmpApp, ...) {
/// ```
///
/// The `#[no_mangle]` attribute lives on a separate line — we don't require
/// it here. The visibility modifier (`pub`, `pub(crate)`) is also optional.
/// The decisive markers are `extern "C" fn ` and the literal token
/// `nmp_app_` that follows.
///
/// Returns `None` when the line does not open such a function or its `{` is
/// on a later line.
fn find_nmp_app_extern_fn_opener_with_brace(line: &str) -> Option<usize> {
    // Cheap reject for the vast majority of lines.
    if !line.contains("extern \"C\"") || !line.contains("nmp_app_") {
        return None;
    }
    // Must also open the body on this line.
    if !line.contains('{') {
        return None;
    }
    // Locate the `fn nmp_app_` token (allowing whitespace between `fn` and
    // the identifier). The simplest way: find `fn ` after `extern "C"`, then
    // verify the identifier that follows starts with `nmp_app_`.
    let extern_pos = line.find("extern \"C\"")?;
    let after_extern = &line[extern_pos..];
    let fn_rel = after_extern.find(" fn ")?;
    let fn_abs = extern_pos + fn_rel + 1; // skip the leading space
    let after_fn = &line[fn_abs + 3..]; // skip "fn "
    let trimmed = after_fn.trim_start();
    if trimmed.starts_with("nmp_app_") {
        let trim_len = after_fn.len() - trimmed.len();
        Some(fn_abs + 3 + trim_len)
    } else {
        None
    }
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
