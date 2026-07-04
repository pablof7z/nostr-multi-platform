//! Bounded blocking HTTP GET → JSON (native only).
//!
//! Runs on a spawned worker thread — blocking I/O is acceptable here precisely
//! because we are NOT on the actor thread (D8). A bounded timeout and a hard,
//! caller-supplied response-body cap keep a hostile / runaway endpoint a
//! bounded error, not an OOM event.
//!
//! Consolidates the two forked copies that used to live in
//! `nmp-nip05::http::http_get_json` (100 KiB cap) and
//! `nmp-nip57::lnurl::roundtrip::http_get_json` (64 KiB cap) into one
//! canonical helper parameterized by the cap (#2927).

use std::io::Read;

/// Total budget for a single `.well-known` GET. Conservative — keeps a stuck
/// domain from accumulating worker threads even though each thread is
/// independent of the actor loop.
const HTTP_TIMEOUT_SECS: u64 = 10;

/// One-shot HTTP GET → JSON. Bounded by [`HTTP_TIMEOUT_SECS`] and the
/// caller-supplied `max_bytes`. The result is a `serde_json::Value` because
/// `.well-known` documents carry optional sibling objects each caller models
/// differently.
///
/// The request does NOT follow redirects (`redirects(0)`): the host was vetted
/// by [`crate::assert_host_is_public`] (when the caller guards it), and a `3xx`
/// could otherwise bounce the request to an un-vetted (internal) host. With
/// `redirects(0)` ureq returns the `3xx` verbatim, which the `status() != 200`
/// check turns into a bounded error rather than a silent follow.
///
/// `max_bytes` is the hard response-body cap; a runaway / hostile endpoint that
/// streams past it is truncated at the cap and parsed (or fails to parse) —
/// never buffered unboundedly.
pub fn http_get_json(url: &str, max_bytes: usize) -> Result<serde_json::Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        // SSRF guard — DO NOT follow redirects (see doc comment above).
        .redirects(0)
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("HTTP GET {url} failed: {e}"))?;
    if response.status() != 200 {
        return Err(format!(
            "HTTP GET {url} returned status {} {}",
            response.status(),
            response.status_text()
        ));
    }
    // Bound the response so a runaway / hostile endpoint can't OOM us.
    let mut body = Vec::with_capacity(1024);
    response
        .into_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("read response body from {url}: {e}"))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| format!("parse JSON from {url}: {e}"))
}
