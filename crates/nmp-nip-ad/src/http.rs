//! Blocking NIP-AD `.well-known/nostr.json?ad=<path>` HTTP round-trip (native).
//!
//! Runs on the spawned worker thread — blocking I/O is acceptable here
//! precisely because we are NOT on the actor thread (D8). The SSRF host guard
//! and the bounded GET are shared with nip05 through `nmp-wellknown-http`
//! (#2927) — one canonical, security-critical path, never forked.

use nmp_wellknown_http::{assert_host_is_public, http_get_json};
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

use crate::parse::{parse_ad_wellknown, AdResolution};

/// Maximum response body the worker will accept. A NIP-AD document is a JSON
/// object mapping paths to `{filter, relays}` entries; 100 KiB is far over any
/// sane size. The cap makes a hostile / runaway endpoint a bounded error, not
/// an OOM event.
const AD_MAX_RESPONSE_BYTES: usize = 100 * 1024;

/// Resolve a NIP-AD web URL into its `{filter, relays}` collection query.
///
/// Parses `url` into (host, path); vets the host with the shared SSRF guard;
/// fetches `https://<host>/.well-known/nostr.json?ad=<percent-encoded-path>`
/// through the shared bounded GET; then selects the entry keyed by `path` and
/// parses it.
///
/// Returns `Err` when the URL is not an `http(s)` URL with a host, the host
/// fails the SSRF guard, the GET fails, or the document has no matching
/// `{filter, relays}` entry. The reason is human-readable for a diagnostic
/// token; it never echoes the response body verbatim.
pub fn resolve_ad_url_blocking(url: &str) -> Result<AdResolution, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("NIP-AD URL `{url}` is not a URL: {e}"))?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err(format!(
            "NIP-AD URL `{url}` is not an http(s) URL (scheme `{}`)",
            parsed.scheme()
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("NIP-AD URL `{url}` has no host"))?
        .to_ascii_lowercase();
    // The path to resolve, with its leading `/` (e.g. `/legible`). This is both
    // the `?ad=` query value (percent-encoded) and the key we select from the
    // response — they must be the same string.
    let path = parsed.path().to_string();

    // SSRF guard — reject IP-literal hosts and hosts that resolve to a
    // non-public address BEFORE the fetch (shared with nip05). Runs here on the
    // blocking worker because DNS resolution is itself blocking IO (D8).
    assert_host_is_public(&host)?;

    // Percent-encode the path for the query value (never hand-rolled). The
    // `.well-known` host is always fetched over https regardless of the
    // original URL scheme — the `.well-known` endpoint is a secure control
    // surface.
    let encoded_path = utf8_percent_encode(&path, NON_ALPHANUMERIC).to_string();
    let well_known_url = format!("https://{host}/.well-known/nostr.json?ad={encoded_path}");
    let document = http_get_json(&well_known_url, AD_MAX_RESPONSE_BYTES)?;
    parse_ad_wellknown(&document, &path)
}
