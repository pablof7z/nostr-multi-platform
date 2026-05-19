//! String-aware brace counter shared between the walker (cfg-test tracker)
//! and the D8 hot-path tracker. Counts `{` / `}` while skipping contents of:
//!
//!   - `"..."` strings (honours `\"` and `\\` escapes)
//!   - `'...'` char literals (distinguished from lifetimes by looking ahead
//!     for a closing `'` within 4 bytes)
//!   - `//` line-comments (everything after `//` to EOL)
//!
//! Does NOT precisely handle raw strings `r#"..."#` — they're rare and the
//! worst case is over-counting braces, which only causes the cfg-test scope
//! to pop early (a false-positive D6 emitted from inside a `mod tests`).
//! Such failures show up loud-and-clear in the zero-FP sweep.

pub fn count_braces_ignoring_strings(line: &str) -> (usize, usize) {
    let bytes = line.as_bytes();
    let mut opens = 0usize;
    let mut closes = 0usize;
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
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // rest of line is a line-comment
        }
        if b == b'"' {
            in_str = true;
            i += 1;
            continue;
        }
        if b == b'\'' {
            // Only treat as char-literal if a closing `'` lives within 4
            // bytes (otherwise it's a lifetime annotation; ignore).
            if let Some(close) = find_char_close(&bytes[i + 1..]) {
                i += close + 2;
                continue;
            }
        }
        if b == b'{' {
            opens += 1;
        } else if b == b'}' {
            closes += 1;
        }
        i += 1;
    }
    (opens, closes)
}

fn find_char_close(rest: &[u8]) -> Option<usize> {
    let mut i = 0;
    let mut saw_escape = false;
    while i < rest.len() && i < 4 {
        let b = rest[i];
        if saw_escape {
            saw_escape = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            saw_escape = true;
            i += 1;
            continue;
        }
        if b == b'\'' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::count_braces_ignoring_strings as cb;

    #[test]
    fn strings_are_ignored() {
        assert_eq!(cb("    let s = \"{ not real }\";"), (0, 0));
    }

    #[test]
    fn line_comments_are_ignored() {
        assert_eq!(cb("    fn x() { // }} not counted"), (1, 0));
    }

    #[test]
    fn real_braces_counted() {
        assert_eq!(cb("    fn x() { let s = \"}}}\"; }"), (1, 1));
    }

    #[test]
    fn lifetimes_not_misread_as_char_lit() {
        // The `'a` here is a lifetime, not a char literal.
        assert_eq!(cb("    fn x<'a>() { 0 }"), (1, 1));
    }
}
