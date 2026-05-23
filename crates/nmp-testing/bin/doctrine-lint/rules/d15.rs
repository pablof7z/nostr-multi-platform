//! D15 — host-supplied closures invoked through the kernel MUST be wrapped
//! in `catch_unwind` (or the equivalent `guard_ffi_callback`).
//!
//! # Why this rule exists
//!
//! `nmp-core` holds a small but growing set of registration seams that let a
//! host plug behaviour into the kernel — action validators / executors / result
//! observers, event observers, raw-event taps, snapshot projections, capability
//! handlers, …. Every one of these surfaces ends up invoking an
//! `Arc<dyn …>` or `Box<dyn Fn…>` value the kernel did not author.
//!
//! Those invocations are the only sites where an *unguarded* panic can kill
//! the kernel: the actor thread's outer [`catch_unwind`] only wraps the
//! relay-event lane, command drain panics are intentionally loud, and FFI
//! callbacks crossing the C-ABI raise undefined behaviour. The fix is the
//! same in every case — wrap the call site in [`catch_unwind`] (Rust
//! observers) or [`guard_ffi_callback`] (C-ABI callbacks).
//!
//! D15 flags the inverse of that pattern: an invocation of a host-supplied
//! closure that is NOT lexically contained in either guard. Catching a
//! new registration seam at code-review time is much cheaper than waiting
//! for the next codex panic-isolation audit.
//!
//! # Scope
//!
//! Only `crates/nmp-core/src/` — host-closure registration seams live in the
//! kernel substrate; protocol and app crates consume the FFI surface and have
//! no untrusted closures of their own to invoke.
//!
//! # What this catches
//!
//! Two invocation shapes are recognised:
//!
//! * **A boxed-closure call through parens** — `(self.validate)(...)`,
//!   `(self.callback)(...)`, `(*ptr)(...)`. The leading parenthesised name
//!   plus `(` is unambiguously a function-pointer / `Fn` call.
//! * **A bare `observer(` / `callback(` / `projection(` invocation** when
//!   the binding name is `observer`, `callback`, or `projection` — the
//!   codebase convention for the host-closure binding extracted under the
//!   slot lock.
//!
//! An invocation is **guarded** when the same line contains
//! `catch_unwind(` / `guard_ffi_callback(`, OR the invocation sits inside an
//! enclosing block whose opening line did. The guard window closes when the
//! file's brace depth returns to the depth recorded at the opening line.
//!
//! # Allowed exemptions
//!
//! 1. **Comment lines** — line, block, or doc-comments never fire.
//! 2. **`#[cfg(test)]` blocks** — fixture observers in tests legitimately
//!    panic without guards (the test harness *is* the guard).
//! 3. **The actor command drain** — `actor/mod.rs:885-887` deliberately
//!    leaves command-drain panics loud (commands are internally generated;
//!    a panic there is a genuine bug that must stay visible). The allowlist
//!    catches this site by path + token pair.
//! 4. **`// doctrine-allow: D15 — reason`** per-line opt-out.

use std::path::Path;

pub const ID: &str = "D15";

/// True iff the file lives under `crates/nmp-core/src/`. Host-closure
/// registration seams are an nmp-core substrate concern; protocol and app
/// crates have no equivalent registries.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    let in_core =
        s.contains("/crates/nmp-core/src/") || s.starts_with("crates/nmp-core/src/");
    if !in_core {
        return false;
    }
    // The bin's own rule source is exempt — its body contains the very
    // invocation tokens this rule scans for as identifiers / docs.
    if s.contains("/doctrine-lint/") {
        return false;
    }
    true
}

/// Tokens that mark a line as opening a panic-guard scope. When any of these
/// substrings appear on a line, the rest of the brace block that line opens
/// is considered guarded and any host-closure invocation inside it is allowed.
const GUARD_TOKENS: &[&str] = &["catch_unwind(", "guard_ffi_callback("];

/// Bare-name invocation patterns we recognise as host-supplied closures.
/// Each entry is the prefix before the call-site `(`. The matcher additionally
/// requires the character preceding the match to be a non-identifier byte
/// (so `my_observer(` doesn't false-fire on `observer(`).
const INVOCATION_NAMES: &[&str] = &["observer", "callback", "projection"];

/// True iff the byte at `idx-1` (or the start-of-line) is a token boundary —
/// i.e. NOT an alphanumeric / underscore. We use this to make the bare-name
/// patterns (`observer(`, `callback(`, `projection(`) match only when the
/// matched span is a standalone identifier, not the tail of a longer name
/// like `my_observer(` or `event_callback(`.
fn is_token_boundary_before(line: &str, idx: usize) -> bool {
    if idx == 0 {
        return true;
    }
    let prev = line.as_bytes()[idx - 1];
    !(prev.is_ascii_alphanumeric() || prev == b'_')
}

/// Per-file scanner state. Two pieces of bookkeeping cross line boundaries:
///
/// * `paren_depth` — running `(` / `)` count, string-aware (the shared
///   `crate::braces` counter handles strings but we recount parens here
///   since it tracks braces only).
/// * `guard_stack` — stack of paren depths at which a guard scope opened.
///   While non-empty the cursor is inside a `catch_unwind(...)` /
///   `guard_ffi_callback(...)` call; an invocation on such a line is
///   allowed.
///
/// Why paren depth and not brace depth? Guard sites are paren-delimited
/// (`catch_unwind(...)`); the body MAY also contain `|| { ... }` braces but
/// those are not what closes the guard scope. Tracking parens makes the
/// multi-line shape `guard_ffi_callback(` (line N) … `)` (line M) Just Work.
///
/// Pop semantics: when `paren_depth` falls back to (or below) the top of
/// the stack, the guard scope has closed. Nested guards stack naturally.
#[derive(Default)]
pub struct State {
    paren_depth: i32,
    guard_stack: Vec<i32>,
}

impl State {
    /// True while the cursor is lexically inside a `catch_unwind(` /
    /// `guard_ffi_callback(` block.
    pub fn in_guard(&self) -> bool {
        !self.guard_stack.is_empty()
    }
}

/// Check one line. The caller supplies the path so allowlist sites can be
/// gated by file. Returns `(col, message, suggested)` per finding.
///
/// `state` must be the same instance across every line of a single file so
/// brace depth and guard stack track correctly.
pub fn check(
    state: &mut State,
    path: &Path,
    line: &str,
    is_comment: bool,
) -> Vec<(usize, String, String)> {
    // The pre-line state IS the state at the start of this line. Snapshot
    // guard membership BEFORE we apply this line's paren deltas, so the
    // same line that opens a guard is itself counted as guarded (the entire
    // `catch_unwind(...)` call).
    let guarded_at_line_start = state.in_guard();

    // Compute paren deltas for this line (skip when it's purely a comment).
    let (opens, closes) = if is_comment {
        (0, 0)
    } else {
        crate::braces::count_parens_ignoring_strings(line)
    };

    // Detect a guard scope opening on this line. We scan with a
    // string-aware matcher so a guard token inside a string literal does
    // NOT register a guard scope — same robustness as `count_parens_ignoring_strings`.
    let opened_guard_on_this_line = !is_comment
        && GUARD_TOKENS.iter().any(|tok| contains_outside_strings(line, tok));
    if opened_guard_on_this_line {
        // Record the depth BEFORE this line's opens are applied — that is
        // the baseline we return to when the guard's outermost `)` closes.
        // `state.in_guard()` reports `true` from this point until the pop.
        state.guard_stack.push(state.paren_depth);
    }

    // Apply this line's paren deltas.
    state.paren_depth += opens as i32;
    state.paren_depth -= closes as i32;
    // Pop any guard frames whose scope has closed. We pop when the depth
    // strictly drops BELOW the baseline so the line that opens a guard
    // and closes it on the same line stays counted as guarded — without
    // this we'd pop too eagerly when `guard_ffi_callback(...)` is fully
    // expressed on the opener line.
    while let Some(&top) = state.guard_stack.last() {
        if state.paren_depth < top + 1 && !opened_guard_on_this_line {
            // The baseline-or-below pop is correct for guards opened on a
            // PRIOR line. For a guard opened on THIS line we want it to
            // stay registered through the end-of-line check below; the
            // `!opened_guard_on_this_line` guard handles that.
            state.guard_stack.pop();
        } else if state.paren_depth <= top && opened_guard_on_this_line {
            // Same-line guard fully closed (`catch_unwind(|| body())`):
            // the guard contained the call so we keep the per-line
            // `guarded_same_line` flag set below, but the guard frame
            // itself can come off the stack now.
            state.guard_stack.pop();
        } else {
            break;
        }
    }

    if is_comment {
        return Vec::new();
    }

    // Same-line guard: a line containing both an invocation and a guard token
    // (e.g. `catch_unwind(AssertUnwindSafe(|| observer(result)))`) is allowed
    // even if the guard frame closes again on the same line.
    let guarded_same_line = opened_guard_on_this_line;
    // The line is allowed if the start-of-line state was already inside a
    // guard, OR a guard token opens on this line.
    let allowed = guarded_at_line_start || guarded_same_line;

    // Actor-command-drain allowlist. The drain at `actor/mod.rs:885-887`
    // deliberately stays loud (internally generated commands; a panic there
    // is a genuine bug). The path + token match keeps the allowlist tight.
    if is_command_drain_site(path) {
        return Vec::new();
    }

    let mut hits = Vec::new();
    // 1. `(<expr>)(` — parens-wrapped closure call. We look for `)(`
    //    (an open paren immediately following a close paren). To avoid
    //    flagging ordinary `f(a)(b)` curried-call shapes — which are
    //    unusual but possible — require the inner parens to enclose a
    //    name-shaped expression (an identifier or `*identifier`).
    for (idx, _) in line.match_indices(")(") {
        // Look back to find the matching `(`. Trivial: walk back counting
        // parens. We only need to handle small spans here; this is a
        // line-local check.
        let Some(open_idx) = matching_open_paren(line, idx) else {
            continue;
        };
        // The text between `(` and `)` (the function expression).
        let expr = &line[open_idx + 1..idx];
        if !is_closure_invocation_expr(expr) {
            continue;
        }
        if allowed || allow_line(line) {
            continue;
        }
        hits.push((
            open_idx + 1,
            format!(
                "host-closure invocation `({expr})(...)` is not wrapped in \
                 `catch_unwind` / `guard_ffi_callback` — D15 requires every \
                 host-supplied closure call to be guarded"
            ),
            "wrap the call in `catch_unwind(AssertUnwindSafe(|| (...)(...)))` \
             for Rust closures, or `guard_ffi_callback(\"<site>\", || (...))` \
             for C-ABI fn pointers"
                .to_string(),
        ));
    }
    // 2. Bare `observer(` / `callback(` / `projection(` — the codebase
    //    convention for the binding name extracted from a registry under
    //    its slot lock. The bare-name match must reject:
    //    * function DEFINITIONS — `fn observer(...)`, `extern "C" fn observer(...)`
    //    * METHOD definitions — `fn observer(&self, ...)`
    //    Both shapes are detected by a `fn ` prefix preceding the name.
    for name in INVOCATION_NAMES {
        let pat_open = format!("{name}(");
        let mut search_from = 0usize;
        while let Some(rel) = line[search_from..].find(&pat_open) {
            let abs = search_from + rel;
            search_from = abs + pat_open.len();
            if !is_token_boundary_before(line, abs) {
                continue;
            }
            if is_fn_definition_prefix(line, abs) {
                continue;
            }
            if allowed || allow_line(line) {
                continue;
            }
            hits.push((
                abs + 1,
                format!(
                    "host-closure invocation `{name}(...)` is not wrapped in \
                     `catch_unwind` / `guard_ffi_callback` — D15 requires every \
                     host-supplied closure call to be guarded"
                ),
                format!(
                    "wrap the call in `catch_unwind(AssertUnwindSafe(|| {name}(...)))` \
                     (Rust observer) or `guard_ffi_callback(\"<site>\", || {name}(...))` \
                     (C-ABI fn pointer)"
                ),
            ));
        }
    }
    hits
}

/// True if `line` contains a `// doctrine-allow: D15` opt-out. The driver
/// also runs its own `allow::line_allows` check; this local one keeps the
/// rule self-contained for unit tests.
fn allow_line(line: &str) -> bool {
    line.contains("doctrine-allow: D15")
        || line.contains("doctrine-allow:D15")
}

/// True iff `path` is the actor command drain site whose panics are
/// intentionally loud (D15 allowlist). The path match is exact-suffix so a
/// stray `actor/mod.rs` in another crate cannot accidentally trip the
/// exemption.
fn is_command_drain_site(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.ends_with("nmp-core/src/actor/mod.rs")
}

/// Walk back from `close_paren_idx` (which points at the `)` in `)(`) to
/// find the matching `(`. Returns `None` if no balanced match exists on the
/// same line (the call-site detection is best-effort line-local).
fn matching_open_paren(line: &str, close_paren_idx: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut depth = 1i32;
    let mut i = close_paren_idx;
    while i > 0 {
        i -= 1;
        let b = bytes[i];
        if b == b')' {
            depth += 1;
        } else if b == b'(' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

/// True iff `expr` looks like a parens-wrapped name expression that names
/// a closure binding — i.e. `self.validate`, `*ptr`, `registration.callback`,
/// or a bare identifier `observer`. Excludes shapes that contain a `(`, `,`,
/// or `;` (those are full call expressions, not closure-name expressions).
fn is_closure_invocation_expr(expr: &str) -> bool {
    let t = expr.trim();
    if t.is_empty() {
        return false;
    }
    // Reject anything with structure that can't be a single name.
    if t.contains('(') || t.contains(',') || t.contains(';') {
        return false;
    }
    // Allow a leading `*` (deref of a function-pointer-shaped value).
    let s = t.trim_start_matches('*').trim_start();
    // Allow chained field access: identifier (. identifier)* — every
    // component must be identifier-shaped.
    s.split('.').all(is_ident_like)
}

/// True iff `needle` appears in `line` outside of any string literal,
/// char literal, or line comment. Mirrors the byte-by-byte skipping rules
/// in `crate::braces::count_parens_ignoring_strings` so a string-embedded
/// guard token (`let s = "catch_unwind("`) does NOT register a guard scope.
fn contains_outside_strings(line: &str, needle: &str) -> bool {
    let needle_bytes = needle.as_bytes();
    if needle_bytes.is_empty() {
        return false;
    }
    let bytes = line.as_bytes();
    let mut in_str = false;
    let mut in_char = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_str {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        // Line-comment cuts the rest of the line.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return false;
        }
        if b == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        if b == b'\'' {
            // Lifetime vs. char literal — same heuristic as the brace counter.
            let rest = &bytes[i + 1..];
            let mut j = 0;
            let mut saw_escape = false;
            let mut closed = false;
            while j < rest.len() && j < 4 {
                if saw_escape {
                    saw_escape = false;
                    j += 1;
                    continue;
                }
                if rest[j] == b'\\' {
                    saw_escape = true;
                    j += 1;
                    continue;
                }
                if rest[j] == b'\'' {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if closed {
                i += j + 2;
                continue;
            }
        }
        // Try to match the needle at this position.
        if i + needle_bytes.len() <= bytes.len()
            && &bytes[i..i + needle_bytes.len()] == needle_bytes
        {
            return true;
        }
        i += 1;
    }
    false
}

/// True iff the bytes immediately before `name_idx` look like a function
/// or method definition prefix — `fn `, possibly with a type-parameter or
/// async-modifier in front. This is the heuristic we use to skip
/// `extern "C" fn observer(...) { ... }` (a definition) while still
/// flagging `observer(payload)` (an invocation). The check is line-local;
/// multi-line `fn` signatures spanning hundreds of bytes are accepted as a
/// known limitation (they almost never apply to the host-closure names we
/// look for).
fn is_fn_definition_prefix(line: &str, name_idx: usize) -> bool {
    let before = &line[..name_idx];
    let trimmed = before.trim_end();
    // `fn ` (with at least one trailing space) is the canonical prefix.
    // Tolerate the trailing space being eaten — the trimmed view drops it,
    // so check `ends_with("fn")` AND that the original `before` had
    // whitespace between `fn` and the name.
    if trimmed.ends_with("fn") {
        // Confirm there is whitespace between `fn` and the binding name —
        // otherwise `myfn` would match, which is wrong.
        if before.len() > 2 && before.as_bytes()[before.len() - 1].is_ascii_whitespace() {
            return true;
        }
    }
    false
}

fn is_ident_like(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    s.chars().enumerate().all(|(i, c)| {
        if i == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_'
        }
    })
}

#[cfg(test)]
#[path = "d15/tests.rs"]
mod tests;
