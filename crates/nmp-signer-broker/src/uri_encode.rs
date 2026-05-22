//! RFC 3986 query-value percent-encoder, shared by `ffi.rs` (callback scheme)
//! and `broker/nostrconnect.rs` (relay URL in the generated `nostrconnect://`
//! URI). Pulled out of the two call sites so a future change to the
//! unreserved-set policy has a single source of truth.
//!
//! Keeping a hand-rolled six-line helper avoids pulling `percent-encoding`
//! into the broker's dependency closure (D8 — minimal deps in protocol
//! crates).

/// Percent-encode a URI query-value byte-for-byte using the RFC 3986
/// unreserved set (`ALPHA / DIGIT / "-" / "_" / "." / "~"`). Everything else
/// is emitted as `%XX`.
pub(crate) fn percent_encode_query_value(value: &str) -> String {
    value
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => vec![b as char],
            _ => format!("%{b:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::percent_encode_query_value;

    #[test]
    fn passes_unreserved_chars_verbatim() {
        // The full RFC 3986 unreserved set must round-trip identical.
        assert_eq!(percent_encode_query_value("AZaz09-_.~"), "AZaz09-_.~");
    }

    #[test]
    fn percent_encodes_reserved_chars() {
        // `:` `/` are reserved; `%3A%2F%2F` is the standard encoding of `://`.
        assert_eq!(
            percent_encode_query_value("chirp://nip46"),
            "chirp%3A%2F%2Fnip46"
        );
    }

    #[test]
    fn handles_empty_input() {
        assert_eq!(percent_encode_query_value(""), "");
    }

    #[test]
    fn percent_encodes_query_separators() {
        // `=` `&` `?` `#` must all be encoded so a caller can't accidentally
        // append extra params by sneaking them through a value.
        assert_eq!(
            percent_encode_query_value("a=b&c?d#e"),
            "a%3Db%26c%3Fd%23e"
        );
    }
}
