//! Rung 4.5 — NIP-AD candidate recognizer (#2927). PURE + SYNC, no IO.
//!
//! Recognizes an ordinary `http(s)://<host>/<path>` web URL as a NIP-AD
//! candidate. The `.well-known/nostr.json?ad=<path>` fetch happens strictly
//! later, in the dispatch layer — classify never does IO, preserving the
//! cascade's purity invariant (issue #1804).
//!
//! Positioned after the NIP-05 shape check (a `name@domain` is never a URL) and
//! before the free-text fall-through: a recognized URL is emitted as an AD
//! candidate ALONGSIDE the free-text candidates for the same input, so the app
//! can search in parallel (D1) while an AD resolution is attempted.

/// Return the AD-candidate URL for `input`, or `None` if it is not an
/// `http(s)://` URL with a plausible (dotted) host. PURE.
///
/// Shape-only: this is a syntactic recognizer, not a validation of whether the
/// domain actually implements NIP-AD (only the fetch can know that). Kept
/// deliberately conservative — anything that isn't clearly a web URL falls
/// through to free text.
pub(super) fn ad_candidate_url(input: &str) -> Option<String> {
    // A URL never contains whitespace; a "URL with a space" is free text.
    if input.chars().any(char::is_whitespace) {
        return None;
    }
    // Case-insensitive scheme match (`HTTPS://` is still a URL); the returned
    // value preserves the original casing (the path is case-sensitive).
    let lower = input.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))?;
    // The host is everything up to the first path / query / fragment delimiter.
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let host = &rest[..host_end];
    // Require a dotted host (a bare hostname is not a resolvable AD domain — the
    // same shape rule NIP-05 uses). This also rejects `http://` with no host.
    if host.is_empty() || !host.contains('.') {
        return None;
    }
    // Reject a host with a leading/trailing dot or empty label (`http://.com/`,
    // `http://foo..com/`) — clearly not a real host.
    let host_only = host.split(['@', ':']).next_back().unwrap_or(host);
    if host_only.starts_with('.')
        || host_only.ends_with('.')
        || host_only.split('.').any(str::is_empty)
    {
        return None;
    }
    Some(input.to_string())
}

#[cfg(test)]
mod tests {
    use super::ad_candidate_url;

    #[test]
    fn recognizes_https_and_http_urls() {
        assert_eq!(
            ad_candidate_url("https://trellis.rs/legible").as_deref(),
            Some("https://trellis.rs/legible")
        );
        assert_eq!(
            ad_candidate_url("http://golf.com/highlights").as_deref(),
            Some("http://golf.com/highlights")
        );
        // Bare host (no path) is still a candidate.
        assert_eq!(
            ad_candidate_url("https://example.com").as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn preserves_path_casing() {
        assert_eq!(
            ad_candidate_url("https://Example.com/MixedCasePath").as_deref(),
            Some("https://Example.com/MixedCasePath")
        );
    }

    #[test]
    fn rejects_non_urls() {
        // No scheme, wrong scheme, whitespace, bare hostname, no host.
        for s in [
            "trellis.rs/legible",
            "wss://relay.example.com",
            "ftp://example.com/x",
            "nostr:npub1foo",
            "hello world",
            "https://localhost/x",
            "https://foo..com/x",
            "https://.com/x",
            "https:// example.com",
            "just some text",
            "alice@example.com",
        ] {
            assert_eq!(ad_candidate_url(s), None, "{s} must not be an AD candidate");
        }
    }
}
