//! RFC 3986 query-value percent-encoder, shared by the app adapter
//! (`nmp-ffi`) and `broker/nostrconnect.rs` (relay URL in the generated
//! `nostrconnect://` URI). Pulled out of the two call sites so a future
//! change to the unreserved-set policy has a single source of truth.
//!
//! Keeping a hand-rolled six-line helper avoids pulling `percent-encoding`
//! into the broker's dependency closure (D8 — minimal deps in protocol
//! crates).

/// Percent-encode a URI query-value byte-for-byte using the RFC 3986
/// unreserved set (`ALPHA / DIGIT / "-" / "_" / "." / "~"`). Everything else
/// is emitted as `%XX`.
pub fn percent_encode_query_value(value: &str) -> String {
    value
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => vec![b as char],
            _ => format!("%{b:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}
