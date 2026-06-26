//! Unit tests for the RFC 3986 query-value percent-encoder.

use crate::percent_encode_query_value;

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
    assert_eq!(percent_encode_query_value("a=b&c?d#e"), "a%3Db%26c%3Fd%23e");
}
