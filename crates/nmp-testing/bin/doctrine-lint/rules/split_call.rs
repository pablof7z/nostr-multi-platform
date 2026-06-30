//! Shared call-token matcher for the event-flow gates D24/D25.
//!
//! Detects a method-call token (`<name>` … `(`) in a way that is tolerant of
//! the formatting variations a line-based lint must survive to stay a useful
//! regression backstop:
//!
//! - contiguous `name(`,
//! - whitespace before the paren `name (`,
//! - a trailing line comment after the name (`name // …` then `(`),
//! - a rustfmt method/paren SPLIT across lines (`…name` on one line, `(` first
//!   on the next non-comment line).
//!
//! Matching is **boundary-anchored on both sides** of `name`: the char before
//! must not be an identifier char (so `force_<name>` / a longer prefix does not
//! fire) and the char after must not be an identifier char (so `<name>X` does
//! not fire). Cross-line detection needs to remember whether the previous code
//! line ended with the bare `name` token, so callers thread a [`State`].
//!
//! This is a formatting heuristic, NOT a formal proof — see the rule modules'
//! doc comments for the documented scope limit.

/// Cross-line tracker: did the previous CODE line end with the bare call name,
/// awaiting an opening `(` on the next line?
#[derive(Default)]
pub struct State {
    name_dangling: bool,
}

/// Strip a trailing `//` line comment, returning the code part (positions
/// before the comment are preserved, so reported columns stay correct).
fn code_part(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// True iff the byte at `idx-1` in `bytes` is an identifier char.
fn preceded_by_ident_char(bytes: &[u8], idx: usize) -> bool {
    idx > 0 && {
        let p = bytes[idx - 1];
        p.is_ascii_alphanumeric() || p == b'_'
    }
}

/// True iff `c` is an identifier char (used for the right-boundary check).
fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True iff `code`'s trimmed tail is the bare `name` token (left-boundary
/// anchored; nothing follows it on the line so the right boundary is implicit).
fn ends_with_name_token(code: &str, name: &str) -> bool {
    let t = code.trim_end();
    if !t.ends_with(name) {
        return false;
    }
    let idx = t.len() - name.len();
    !preceded_by_ident_char(t.as_bytes(), idx)
}

/// Returns the 1-indexed columns of each detected `name( … )` call on this
/// line, catching the contiguous, whitespace, trailing-comment, and
/// method/paren-split shapes. `state` carries the cross-line tracker;
/// `is_comment` / `in_test_cfg` suppress (a comment line neither fires nor
/// advances the tracker; a `#[cfg(test)]` line resets it).
pub fn columns(
    state: &mut State,
    name: &str,
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
) -> Vec<usize> {
    if in_test_cfg {
        state.name_dangling = false;
        return Vec::new();
    }
    if is_comment {
        return Vec::new();
    }

    let code = code_part(line);
    let bytes = code.as_bytes();
    let mut cols = Vec::new();

    // (A) split continuation: the previous code line ended with the bare name
    // and this line opens with `(`.
    let trimmed = code.trim_start();
    if state.name_dangling && trimmed.starts_with('(') {
        cols.push(code.len() - trimmed.len() + 1);
    }

    // (B) same-line occurrences: `name` (both-side boundary) followed by
    // optional whitespace then `(`.
    let mut start = 0;
    while let Some(rel) = code[start..].find(name) {
        let abs = start + rel;
        start = abs + name.len();
        if preceded_by_ident_char(bytes, abs) {
            continue;
        }
        let after = &code[abs + name.len()..];
        // Right boundary: the char immediately after `name` must not be an
        // identifier char (so `name` is not a prefix of a longer identifier).
        if after.chars().next().map(is_ident_char).unwrap_or(false) {
            continue;
        }
        if after.trim_start().starts_with('(') {
            cols.push(abs + 1);
        }
    }

    // Advance the dangling tracker: does the code line end with the bare name?
    state.name_dangling = ends_with_name_token(code, name);
    cols
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: &str = "req_for_relay";

    fn run(lines: &[&str]) -> usize {
        let mut s = State::default();
        let mut n = 0;
        for l in lines {
            n += columns(&mut s, N, l, false, false).len();
        }
        n
    }

    #[test]
    fn contiguous() {
        let mut s = State::default();
        assert_eq!(
            columns(&mut s, N, "    req_for_relay(a);", false, false),
            vec![5]
        );
    }

    #[test]
    fn whitespace_before_paren() {
        let mut s = State::default();
        assert_eq!(
            columns(&mut s, N, "    req_for_relay (a);", false, false).len(),
            1
        );
    }

    #[test]
    fn trailing_comment_then_split_paren() {
        // name dangling with a trailing comment, `(` on the next line.
        let n = run(&["    .req_for_relay // build it", "    (role, url);"]);
        assert_eq!(n, 1, "trailing-comment + split paren must fire");
    }

    #[test]
    fn method_paren_split() {
        let n = run(&["        .req_for_relay", "        (role, url, id);"]);
        assert_eq!(n, 1, "method/paren split must fire");
    }

    #[test]
    fn left_boundary_excludes_longer_prefix() {
        let mut s = State::default();
        assert!(columns(&mut s, N, "    build_req_for_relay(x);", false, false).is_empty());
    }

    #[test]
    fn right_boundary_excludes_longer_suffix() {
        let mut s = State::default();
        assert!(columns(&mut s, N, "    req_for_relay_v2(x);", false, false).is_empty());
    }

    #[test]
    fn comment_line_neither_fires_nor_advances() {
        let mut s = State::default();
        // A `.req_for_relay` line sets dangling; an interposed comment line must
        // not clear it, so the following `(` still fires.
        assert!(columns(&mut s, N, "    .req_for_relay", false, false).is_empty());
        assert!(columns(&mut s, N, "    // a comment", true, false).is_empty());
        assert_eq!(columns(&mut s, N, "    (x);", false, false).len(), 1);
    }

    #[test]
    fn test_cfg_resets_pending() {
        let mut s = State::default();
        assert!(columns(&mut s, N, "    .req_for_relay", false, false).is_empty());
        assert!(columns(&mut s, N, "    (x);", false, true).is_empty());
    }

    #[test]
    fn no_false_positive_on_bare_name_without_paren() {
        // `let r = req_for_relay;` (no call) followed by an unrelated line.
        let n = run(&["    let f = req_for_relay;", "    foo();"]);
        assert_eq!(n, 0, "bare name with no following paren must not fire");
    }
}
